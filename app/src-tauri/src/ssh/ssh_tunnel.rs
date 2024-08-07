use std::{fs, thread};
use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use log::{debug, info, warn};
use ssh2::Session;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::select;
use tokio::sync::{oneshot, watch};

use crate::error::LogicError;
use crate::transport::connection::ConnectionSsh;
use crate::utils::file_util;

const BUFFER_SIZE: usize = 2048;

pub struct SshTunnel {
    session: Arc<Session>,
    proxy_port: u16,
    send_abort: watch::Sender<()>,
}

impl SshTunnel {
    pub async fn new(remote: ConnectionSsh, forward_host: &'static str, forward_port: u16) -> Result<Self, LogicError> {
        let mut session = Session::new()?;
        let addr = format!("{}:{}", remote.host, remote.port);
        let tcp = TcpStream::connect(addr.clone())?;
        session.set_tcp_stream(tcp);
        session.handshake()?;

        session.set_keepalive(true, 5);
        session.set_timeout(10 * 1000);

        if let Some(identity) = remote.identity {
            if let Some(key) = identity.key {
                let file_name = file_util::create_temp_file(key.key.as_slice())?;

                debug!("Temporarily create an ssh private key file {}", file_name);

                let passphrase = if let Some(ref p) = key.passphrase {
                    Some(p.as_str())
                } else {
                    None
                };

                let res = session.userauth_pubkey_file(remote.user.as_str(), None, Path::new(&file_name), passphrase);

                fs::remove_file(file_name.clone())?;
                debug!("Deleted temp file {}", file_name);

                if let Err(e) = res {
                    return Err(LogicError::from(e));
                }
            } else if let Some(password) = identity.password {
                session.userauth_password(remote.user.as_str(), password.as_str())?;
            }
        }

        let session = Arc::new(session);
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let proxy_port = listener.local_addr()?.port();

        let (send_abort, rcv_abort) = watch::channel(());

        debug!("Create ssh[{}] forward accept handler.  {}:{} -> {}", addr, forward_host, forward_port, proxy_port);

        Self::handle_tcp_proxy(addr, listener, Arc::clone(&session), forward_host, forward_port, rcv_abort).await?;

        Ok(SshTunnel {
            session,
            proxy_port,
            send_abort,
        })
    }

    pub fn get_proxy_port(&self) -> u16 {
        self.proxy_port
    }

    async fn handle_tcp_proxy(
        ssh_addr: String,
        listener: TcpListener,
        ssh_session: Arc<Session>,
        forward_host: &'static str,
        forward_port: u16,
        rcv_abort: watch::Receiver<()>,
    ) -> Result<(), LogicError> {
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            debug!("Ssh[{}] proxy accept task started", ssh_addr);
            let mut rcv_abort1 = rcv_abort.clone();
            let rcv_abort2 = rcv_abort.clone();

            let ssh_addr1 = Arc::new(ssh_addr);
            let ssh_addr2 = Arc::clone(&ssh_addr1);

            let accept_task = async move {
                {
                    sender.send(()).unwrap();
                }
                loop {
                    let accept_result = listener.accept().await;
                    match accept_result {
                        Ok((mut stream, _)) => {
                            let mut rcv_abort3 = rcv_abort2.clone();
                            let ssh_session = Arc::clone(&ssh_session);
                            debug!("Ssh[{}] proxy stream task started", ssh_addr2);
                            let ssh_addr3 = Arc::clone(&ssh_addr2);
                            let mut channel = ssh_session.channel_direct_tcpip(forward_host, forward_port, None).unwrap();
                            let stream_write_task = async move {
                                info!("Created ssh[{}] proxy stream {}:{}", ssh_addr3, forward_host, forward_port);
                                loop {
                                    let (request, size) = read_stream(&mut stream).await;
                                    if size <= 0 {
                                        break;
                                    }

                                    channel.write_all(&request[..size]).unwrap();
                                    channel.flush().unwrap();

                                    let (response, size) = read_channel(&mut channel);
                                    if size <= 0 {
                                        break;
                                    }

                                    let r = stream.write_all(&response[..size]).await;
                                    if let Err(e) = r {
                                        warn!("Ssh[{}] stream write error {e}", ssh_addr3);
                                        break;
                                    }
                                    let r = stream.flush().await;
                                    if let Err(e) = r {
                                        warn!("Ssh[{}] stream flush error {e}", ssh_addr3);
                                        break;
                                    }
                                }
                                let _ = channel.close();
                                debug!("Ssh[{}] proxy stream task loop finished", ssh_addr3)
                            };

                            let ssh_addr4 = Arc::clone(&ssh_addr2);
                            tokio::spawn(async move {
                                select! {
                                    _stream_handle = stream_write_task => {
                                        debug!("Ssh[{}] proxy stream task finished", ssh_addr4)
                                    }
                                    _abort = rcv_abort3.changed() => {
                                        debug!("Ssh[{}] proxy stream task received abort event", ssh_addr4);
                                    }
                                }
                                debug!("Ssh[{}] stream future finished", ssh_addr4);
                            });
                        }
                        Err(e) => {
                            warn!("ssh listener error: {e}");
                            break;
                        }
                    }
                };
                debug!("Ssh[{}] proxy accept loop finished", ssh_addr2);
            };
            select! {
                _accept = accept_task => {
                    debug!("Ssh[{}] proxy accept task finished", ssh_addr1)
                }
                _abort = rcv_abort1.changed() => {
                    debug!("Ssh[{}] proxy accept task received abort event", ssh_addr1);
                }
            }
            debug!("Ssh[{}] accept future finished", ssh_addr1);
        });

        let _ = receiver.await?;
        Ok(())
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        match self.send_abort.send(()) {
            Ok(_) => {
                debug!("Ssh send abort success")
            }
            Err(e) => {
                warn!("Ssh send abort error: {e}")
            }
        }
        self.session.disconnect(None, "close", None)
            .unwrap_or_else(|e| warn!("Ssh session disconnect error: {e}"));
        debug!("Ssh tunnel dropped");
    }
}

async fn read_stream<R: AsyncRead + Unpin>(mut stream: R) -> (Vec<u8>, usize) {
    let mut request_buffer = vec![];
    let mut request_len = 0usize;
    loop {
        let mut buffer = vec![0; BUFFER_SIZE];

        match stream.read(&mut buffer).await {
            Ok(n) => {
                if !read_buf_bytes(&mut request_len, &mut request_buffer, n, buffer) {
                    break;
                }
            }
            Err(e) => {
                warn!("Error in reading request data: {:?}", e);
                break;
            }
        }
    }

    (request_buffer, request_len)
}

fn read_channel<R: Read>(channel: &mut R) -> (Vec<u8>, usize) {
    let mut response_buffer = vec![];
    let mut response_len = 0usize;
    loop {
        let mut buffer = vec![0; BUFFER_SIZE];
        let future_stream = channel.read(&mut buffer);
        thread::sleep(Duration::from_millis(10));

        match future_stream {
            Ok(n) => {
                if !read_buf_bytes(&mut response_len, &mut response_buffer, n, buffer) {
                    break;
                }
            }
            Err(e) => {
                if e.kind() == ErrorKind::Other {
                    debug!("Error in reading response data: {:?}", e);
                } else {
                    warn!("Error in reading response data: {:?}", e);
                }
                break;
            }
        }
    }

    (response_buffer, response_len)
}

fn read_buf_bytes(
    full_req_len: &mut usize,
    full_req_buf: &mut Vec<u8>,
    reader_buf_len: usize,
    mut reader_buf: Vec<u8>,
) -> bool {
    if reader_buf_len == 0 {
        false
    } else {
        *full_req_len += reader_buf_len;
        if reader_buf_len < BUFFER_SIZE {
            full_req_buf.append(&mut reader_buf[..reader_buf_len].to_vec());
            false
        } else {
            full_req_buf.append(&mut reader_buf);
            true
        }
    }
}