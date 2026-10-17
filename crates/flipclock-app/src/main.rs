#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod icon_art;
mod platform;
mod single_instance;
mod strings;
mod tray;
mod ui;

use flipclock_core::config::{self, LoadOutcome};
use winit::event_loop::EventLoop;

use crate::app::{App, UserEvent};
use crate::single_instance::Instance;
use crate::strings::Strings;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("off")).init();
    let strings = strings::for_system();
    install_panic_hook(strings);

    let instance = single_instance::acquire();
    if matches!(instance, Instance::AlreadyRunning) {
        log::info!("another instance is running; asked it to show itself");
        return;
    }

    let config_dir = config_dir();
    let (config, outcome) = config::load(&config_dir);
    match &outcome {
        LoadOutcome::Missing => log::info!("no config in {}; using defaults", config_dir.display()),
        LoadOutcome::Loaded { invalid_keys } if !invalid_keys.is_empty() => {
            log::warn!(
                "invalid config values reset to defaults: {}",
                invalid_keys.join(", ")
            );
        }
        LoadOutcome::Loaded { .. } => {}
        LoadOutcome::Corrupt { backup } => {
            log::warn!("corrupt config, defaults used; backup: {backup:?}")
        }
        LoadOutcome::Unreadable(e) => log::warn!("cannot read config: {e}"),
    }

    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // ADR-0002: always X11 (XWayland under Wayland sessions).
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    let event_loop = match builder.build() {
        Ok(el) => el,
        Err(e) => {
            show_fatal_error(strings, &e.to_string());
            return;
        }
    };
    let proxy = event_loop.create_proxy();
    if let Instance::Primary(listener) = instance {
        single_instance::serve(listener, proxy.clone());
    }

    let mut app = App::new(proxy, config_dir, config, strings);
    if let Err(e) = event_loop.run_app(&mut app) {
        show_fatal_error(strings, &e.to_string());
    }
}

/// The folder holding `config.toml` (ADR-0003).
fn config_dir() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let exe = exe.canonicalize().unwrap_or(exe);
    let appimage = std::env::var_os("APPIMAGE").map(std::path::PathBuf::from);
    config::config_dir(&exe, appimage.as_deref())
}

pub fn show_fatal_error(strings: &Strings, message: &str) {
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Flipclock")
        .set_description(format!("{}\n\n{message}", strings.fatal_error))
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

fn install_panic_hook(strings: &'static Strings) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        show_fatal_error(strings, &info.to_string());
    }));
}
