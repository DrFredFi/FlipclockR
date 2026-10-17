//! Single instance: the first instance listens on a local socket; later launches
//! connect, ask it to show its window, and exit.

use std::io::{Read, Write};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions, Name, Stream,
};
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

const SOCKET_NAME: &str = "flipclock-single-instance.sock";
const SHOW: &[u8] = b"show";

pub enum Instance {
    /// This is the first instance; keep the listener alive.
    Primary(LocalSocketListener),
    /// Another instance was told to show itself.
    AlreadyRunning,
    /// Single-instance signalling is unavailable; run anyway.
    Unavailable,
}

fn name() -> std::io::Result<Name<'static>> {
    if GenericNamespaced::is_supported() {
        SOCKET_NAME.to_ns_name::<GenericNamespaced>()
    } else {
        socket_path().to_fs_name::<GenericFilePath>()
    }
}

fn socket_path() -> std::path::PathBuf {
    std::env::temp_dir().join(SOCKET_NAME)
}

pub fn acquire() -> Instance {
    let Ok(n) = name() else {
        return Instance::Unavailable;
    };
    if let Ok(mut stream) = Stream::connect(n.clone()) {
        let _ = stream.write_all(SHOW);
        return Instance::AlreadyRunning;
    }
    // A stale socket file can remain after a crash where names are file paths.
    if !GenericNamespaced::is_supported() {
        let _ = std::fs::remove_file(socket_path());
    }
    match ListenerOptions::new().name(n).create_sync() {
        Ok(listener) => Instance::Primary(listener),
        Err(e) => {
            log::warn!("single-instance listener unavailable: {e}");
            Instance::Unavailable
        }
    }
}

/// Forward "show" requests from later launches into the event loop.
pub fn serve(listener: LocalSocketListener, proxy: EventLoopProxy<UserEvent>) {
    std::thread::Builder::new()
        .name("single-instance".into())
        .spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let mut buf = [0u8; 16];
                let n = conn.read(&mut buf).unwrap_or(0);
                if &buf[..n] == SHOW && proxy.send_event(UserEvent::ShowWindow).is_err() {
                    break;
                }
            }
        })
        .expect("spawn single-instance thread");
}
