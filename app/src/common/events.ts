import {DialogItem, TipsItem} from "~/common/types.ts";
import {WebviewWindow} from "@tauri-apps/api/window";
import {emit} from "@tauri-apps/api/event";
import mitt, {Handler} from "mitt";
import {checkUpdate, installUpdate, UpdateManifest, UpdateResult} from "@tauri-apps/api/updater";
import {_useSettings} from "~/common/store.ts";
import {relaunch} from "@tauri-apps/api/process";
import {writeText} from "@tauri-apps/api/clipboard";

const localEvents = mitt();

export enum EventName {
    LOADING = 'loading',
    DIALOG = 'dialog',
    TIP = 'tip',
    CLOSE_TAB = 'closeTab',
    NEW_CONNECTION = 'newConnection',
    SETTING_UPDATE = 'settingUpdate',
    CONNECTION_IMPORTED = 'connectionImported',
    SNAPSHOT_STATE = 'snapshot_state',
    SNAPSHOT_CREATE = 'snapshotCreate',
    CONFIRM_EXIT = 'confirm_exit'
}

export function _listenLocal(type: EventName, handler: Handler<any>) {
    localEvents.on(type, handler)
}

export function _emitLocal(eventName: EventName, eventPayload?: any) {
    localEvents.emit(eventName, eventPayload)
}

export function _emitGlobal(eventName: EventName, eventPayload?: any) {
    emit(eventName, eventPayload).then(() => {
    }).catch(e => {
        console.error(e)
    })
}

export function _emitWindow(windowLabel: EventName, eventName: string, eventPayload?: any) {
    let window = WebviewWindow.getByLabel(windowLabel);
    if (!window) {
        window = new WebviewWindow(windowLabel)
    }

    window.emit(eventName, eventPayload).then(() => {
    }).catch(e => {
        console.error(e)
    })
}


export function _loading(state: boolean, text?: string) {
    _emitLocal(EventName.LOADING, {
        state,
        text
    })
}

export function _confirm(title: string, text: string,): Promise<undefined> {
    return new Promise((resolve, reject) => {
        let dialog: DialogItem = {
            value: true,
            content: text,
            title,
            icon: 'mdi-alert-circle-outline',
            iconColor: 'yellow-darken-4',
            buttons: [
                {
                    text: "Cancel",
                    callback: (item: DialogItem) => {
                        item.value = false
                        reject()
                    }
                },
                {
                    text: "Confirm",
                    variant: "elevated",
                    color: 'primary',
                    callback: (item: DialogItem) => {
                        item.value = false
                        resolve(undefined)
                    }
                }
            ]
        }

        _emitLocal(EventName.DIALOG, dialog)
    })

}

export function _confirmSystem(text: string): Promise<undefined> {
    return _confirm('System', text)
}

export function _confirmUpdateApp(text: string): Promise<undefined> {
    return new Promise((resolve, reject) => {
        let dialog: DialogItem = {
            value: true,
            content: text,
            title: "Install Update",
            icon: 'mdi-update',
            iconColor: 'green',
            buttons: [
                {
                    text: "Cancel",
                    callback: (item: DialogItem) => {
                        item.value = false
                        reject()
                    }
                },
                {
                    text: "Install",
                    variant: "elevated",
                    color: 'primary',
                    callback: (item: DialogItem) => {
                        item.value = false
                        resolve(undefined)
                    }
                }
            ]
        }

        _emitLocal(EventName.DIALOG, dialog)
    })
}

export function _dialogContent(content: string) {
    let dialog: DialogItem = {
        value: true,
        title: 'Content',
        content: content,
        maxWidth: 1200,
        closeBtn: true
    }

    _emitLocal(EventName.DIALOG, dialog)
}

export function _alertError(text: string) {
    let dialog: DialogItem = {
        value: true,
        title: "Error",
        content: text,
        icon: 'mdi-alert-circle-outline',
        iconColor: "red",
        buttons: [
            {
                text: "Close",
                callback: (item: DialogItem) => {
                    item.value = false
                }
            }
        ]
    }

    _emitLocal(EventName.DIALOG, dialog)
}

export function _tipError(text: string) {
    let tip: TipsItem = {
        value: true,
        content: text,
        timeout: 4000,
        icon: 'mdi-alert-circle-outline',
        class: 'bg-red-lighten-1'
    }

    _emitLocal(EventName.TIP, tip)
}

export function _tipWarn(text: string) {
    let tip: TipsItem = {
        value: true,
        content: text,
        timeout: 4000,
        icon: 'mdi-alert-circle',
        class: 'bg-orange-darken-1'
    }

    _emitLocal(EventName.TIP, tip)
}

export function _tipSuccess(text: string) {
    let tip: TipsItem = {
        value: true,
        content: text,
        timeout: 4000,
        icon: 'mdi-check',
        class: 'bg-green-lighten-1'
    }

    _emitLocal(EventName.TIP, tip)
}

export function _tipInfo(text: string) {
    let tip: TipsItem = {
        value: true,
        content: text,
        timeout: 4000,
        icon: 'mdi-lightbulb-on-40',
        class: 'bg-secondary'
    }

    _emitLocal(EventName.TIP, tip)
}


export function _checkUpdate(): Promise<UpdateManifest> {
    return new Promise((resolve, reject) => {
        checkUpdate().then((res: UpdateResult) => {
            const {shouldUpdate, manifest} = res;
            if (shouldUpdate) {
                resolve(manifest!)
            } else {
                reject()
            }
        }).catch(e => {
            reject(e)
        })
    })
}

export function _checkUpdateAndInstall() {
    _loading(true, "Checking for updates...")
    _checkUpdate().then(manifest => {
        _loading(false)
        _confirmUpdateApp(
            `Etcd workbench <span class="text-green font-weight-bold">${manifest.version}</span> is now available.</br></br>Do you want to download and install it now?`,
        ).then(() => {
            _loading(true, "Installing package...")
            installUpdate().then(async () => {
                relaunch().catch((e:string) => {
                    console.error(e)
                    _alertError("Unable to relaunch, please relaunch manually.")
                })
            }).catch(e => {
                console.error(e)
                _alertError("Unable to download: " + e)
            }).finally(() => {
                _loading(false)
            })
        }).catch(() => {

        })
    }).catch((e) => {
        _loading(false)
        if (e == undefined) {
            _tipSuccess('You are already the latest version')
        } else {
            _tipError(e)
        }
    })
}

export function _copyToClipboard(content: string) {
    writeText(content).then(() => {
        _tipSuccess("Copied")
    }).catch(e => {
        _tipError("Can not write to clipboard")
        console.error(e)
    })
}
