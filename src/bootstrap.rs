#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use log::{error, info};
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use services::config::get_app_root_dir;

/// Linux 单实例检测：通知已有实例激活窗口，返回 true 表示应退出
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub(crate) fn notify_running_instance() -> bool {
    use std::os::unix::net::UnixDatagram;

    let sock_path = get_app_root_dir().join("lumen.sock");
    if let Ok(socket) = UnixDatagram::unbound() {
        if socket.connect(&sock_path).is_ok() {
            if socket.send(b"activate").is_ok() {
                info!("已有 Lumen 实例在运行，已通知其激活窗口");
                return true;
            }
        } else if sock_path.exists() {
            // 连接失败但 socket 文件存在，说明是旧实例遗留的脏文件
            let _ = std::fs::remove_file(&sock_path);
        }
    }
    false
}

/// Linux 单实例监听：在后台线程监听 socket，收到激活信号时通知 Sender
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub(crate) fn start_socket_listener(tx: std::sync::mpsc::Sender<()>) {
    use std::os::unix::net::UnixDatagram;

    let sock_path = get_app_root_dir().join("lumen.sock");
    // 移除可能的旧 socket 文件
    let _ = std::fs::remove_file(&sock_path);

    if let Ok(listener) = UnixDatagram::bind(&sock_path) {
        std::thread::spawn(move || {
            let mut buf = [0u8; 64];
            while let Ok(len) = listener.recv(&mut buf) {
                let msg = String::from_utf8_lossy(&buf[..len]);
                if msg.trim_end_matches('\0') == "activate" {
                    let _ = tx.send(());
                }
            }
        });
        info!("单实例 socket 监听已启动: {sock_path:?}");
    } else {
        error!("无法绑定单实例 socket: {sock_path:?}");
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn set_mac_app_name() {
    use objc::{class, msg_send, runtime::Object, sel, sel_impl};
    use std::ffi::CString;
    unsafe {
        let info: *mut Object = msg_send![class!(NSProcessInfo), processInfo];
        let name = CString::new("Lumen").expect("app name contains no nul byte");
        let ns_string: *mut Object =
            msg_send![class!(NSString), stringWithUTF8String: name.as_ptr()];
        let _: () = msg_send![info, setProcessName: ns_string];
    }
}
