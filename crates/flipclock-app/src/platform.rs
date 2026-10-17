//! Small per-OS gaps winit does not cover.

use std::time::Duration;

/// The system double-click interval.
pub fn double_click_interval() -> Duration {
    imp::double_click_interval()
}

/// How far, in physical pixels, the pointer must move with the button held
/// before a press becomes a drag.
pub fn drag_threshold() -> f64 {
    imp::drag_threshold()
}

/// Whether the desktop can show a tray icon.
pub fn tray_available() -> bool {
    imp::tray_available()
}

/// Show or hide the app in the Dock (macOS only; no-op elsewhere).
pub fn set_dock_icon_visible(visible: bool) {
    imp::set_dock_icon_visible(visible);
}

#[cfg(windows)]
mod imp {
    use std::time::Duration;

    use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXDRAG};

    pub fn double_click_interval() -> Duration {
        // SAFETY: plain query with no arguments.
        Duration::from_millis(u64::from(unsafe { GetDoubleClickTime() }))
    }

    pub fn drag_threshold() -> f64 {
        // SAFETY: plain query with a valid metric index.
        f64::from(unsafe { GetSystemMetrics(SM_CXDRAG) }.max(1))
    }

    pub fn tray_available() -> bool {
        true
    }

    pub fn set_dock_icon_visible(_visible: bool) {}
}

#[cfg(target_os = "macos")]
mod imp {
    use std::time::Duration;

    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    pub fn double_click_interval() -> Duration {
        // SAFETY: class method returning an NSTimeInterval (f64).
        let seconds: f64 = unsafe { msg_send![class!(NSEvent), doubleClickInterval] };
        Duration::from_secs_f64(seconds.clamp(0.1, 2.0))
    }

    pub fn drag_threshold() -> f64 {
        4.0
    }

    pub fn tray_available() -> bool {
        true
    }

    pub fn set_dock_icon_visible(visible: bool) {
        // NSApplicationActivationPolicyRegular = 0, NSApplicationActivationPolicyAccessory = 1.
        let policy: isize = if visible { 0 } else { 1 };
        // SAFETY: called on the main thread (from the winit event loop); NSApp exists.
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let _: bool = msg_send![app, setActivationPolicy: policy];
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use std::process::Command;
    use std::time::Duration;

    pub fn double_click_interval() -> Duration {
        Duration::from_millis(400)
    }

    pub fn drag_threshold() -> f64 {
        4.0
    }

    /// A StatusNotifier host is running when `org.kde.StatusNotifierWatcher` has an
    /// owner on the session bus. If `dbus-send` is missing we cannot tell, so assume yes.
    pub fn tray_available() -> bool {
        let output = Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--dest=org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus.NameHasOwner",
                "string:org.kde.StatusNotifierWatcher",
            ])
            .output();
        match output {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout);
                out.status.success() && text.contains("boolean true")
            }
            Err(e) => {
                log::warn!("cannot run dbus-send ({e}); assuming a tray is available");
                true
            }
        }
    }

    pub fn set_dock_icon_visible(_visible: bool) {}
}
