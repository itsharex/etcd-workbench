package org.beifengtz.etcd.server.service;

import io.netty.bootstrap.ServerBootstrap;
import io.netty.channel.Channel;
import io.netty.channel.ChannelFuture;
import io.netty.channel.EventLoopGroup;
import org.beifengtz.etcd.server.config.Configuration;
import org.beifengtz.etcd.server.handler.HttpHandlerProvider;
import org.beifengtz.jvmm.convey.channel.ChannelInitializers;
import org.beifengtz.jvmm.convey.channel.HttpServerChannelInitializer;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * description: TODO
 * date: 15:00 2023/5/23
 *
 * @author beifengtz
 */
public class HttpService {

    protected Channel channel;

    private static final Logger logger = LoggerFactory.getLogger(HttpService.class);

    public void start(int port) {
        long st = System.currentTimeMillis();
        EventLoopGroup group = ChannelInitializers.newEventLoopGroup(1);
        ChannelFuture future = new ServerBootstrap()
                .group(group)
                .channel(ChannelInitializers.serverChannelClass(group))
                .childHandler(new HttpServerChannelInitializer(new HttpHandlerProvider(5, group)))
                .bind(port)
                .syncUninterruptibly();

        logger.info("Http server service started on {}, use {} ms", port, System.currentTimeMillis() - st);
        channel = future.channel();
    }
}
