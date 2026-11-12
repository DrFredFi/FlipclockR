//! Tray icon (menu bar icon on macOS), its menu and notifications.

use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;
use crate::strings::Strings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    ShowHide,
    Countdown,
    Settings,
    AlwaysOnTop,
    Borderless,
    Mute,
    Exit,
}

pub struct Tray {
    _icon: TrayIcon,
    items: Vec<(MenuId, TrayAction)>,
    always_on_top: CheckMenuItem,
    borderless: CheckMenuItem,
    mute: CheckMenuItem,
}

/// Route tray and menu events into the winit event loop.
pub fn forward_events(proxy: EventLoopProxy<UserEvent>) {
    let p = proxy.clone();
    TrayIconEvent::set_event_handler(Some(move |e| {
        let _ = p.send_event(UserEvent::Tray(e));
    }));
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = proxy.send_event(UserEvent::Menu(e.id));
    }));
}

impl Tray {
    pub fn new(
        strings: &Strings,
        always_on_top: bool,
        borderless: bool,
        muted: bool,
    ) -> Option<Self> {
        let show_hide = MenuItem::new(strings.tray_show_hide, true, None);
        let countdown = MenuItem::new(strings.countdown, true, None);
        let settings = MenuItem::new(strings.tray_settings, true, None);
        let on_top = CheckMenuItem::new(strings.always_on_top, true, always_on_top, None);
        let no_border = CheckMenuItem::new(strings.borderless, true, borderless, None);
        let mute = CheckMenuItem::new(strings.tray_mute, true, muted, None);
        let exit = MenuItem::new(strings.exit, true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &show_hide,
            &countdown,
            &settings,
            &PredefinedMenuItem::separator(),
            &on_top,
            &no_border,
            &mute,
            &PredefinedMenuItem::separator(),
            &exit,
        ])
        .inspect_err(|e| log::warn!("cannot build tray menu: {e}"))
        .ok()?;

        let size = 32;
        let icon = Icon::from_rgba(crate::icon_art::icon_rgba(size), size, size).ok()?;
        let tray = TrayIconBuilder::new()
            .with_tooltip("Flipclock")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(cfg!(target_os = "macos"))
            .build()
            .inspect_err(|e| log::warn!("cannot create tray icon: {e}"))
            .ok()?;

        let items = vec![
            (show_hide.id().clone(), TrayAction::ShowHide),
            (countdown.id().clone(), TrayAction::Countdown),
            (settings.id().clone(), TrayAction::Settings),
            (on_top.id().clone(), TrayAction::AlwaysOnTop),
            (no_border.id().clone(), TrayAction::Borderless),
            (mute.id().clone(), TrayAction::Mute),
            (exit.id().clone(), TrayAction::Exit),
        ];
        Some(Self {
            _icon: tray,
            items,
            always_on_top: on_top,
            borderless: no_border,
            mute,
        })
    }

    pub fn action(&self, id: &MenuId) -> Option<TrayAction> {
        self.items.iter().find(|(i, _)| i == id).map(|(_, a)| *a)
    }

    /// Keep the check marks in sync with the current settings.
    pub fn sync(&self, always_on_top: bool, borderless: bool, muted: bool) {
        self.always_on_top.set_checked(always_on_top);
        self.borderless.set_checked(borderless);
        self.mute.set_checked(muted);
    }
}

/// Show a system notification. Runs on its own thread, since some platforms block while showing it.
pub fn notify(title: &str, body: &str) {
    let (title, body) = (title.to_owned(), body.to_owned());
    let spawned = std::thread::Builder::new()
        .name("notification".into())
        .spawn(move || {
            if let Err(e) = notify_rust::Notification::new()
                .appname("Flipclock")
                .summary(&title)
                .body(&body)
                .show()
            {
                log::warn!("cannot show notification: {e}");
            }
        });
    if let Err(e) = spawned {
        log::warn!("cannot start the notification thread: {e}");
    }
}
