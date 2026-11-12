//! The winit application: window, input, clock ticking, settings and tray.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{Local, Timelike};
use flipclock_core::battery::BatteryStatus;
use flipclock_core::config::{self, Config, DatePosition};
use flipclock_core::countdown::{self, Countdown, CountdownLayout, CountdownState, Countdowns};
use flipclock_core::date::format_date;
use flipclock_core::digits::{TILE_COUNT, digits_of};
use flipclock_core::layout::{self, LayoutInput};
use flipclock_core::motion::{Motion, MotionSettings};
use flipclock_core::placement::{self, Monitor, SavedWindow};
use flipclock_render::fonts::{FontCatalog, LoadedFont};
use flipclock_render::{FrameInput, Renderer, Style, wgpu};
use tray_icon::TrayIconEvent;
use tray_icon::menu::MenuId;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{CursorIcon, Fullscreen, ResizeDirection, Window, WindowId, WindowLevel};

use crate::audio::Audio;
use crate::countdown_ui::{CountdownActions, CountdownView, DATE_FAMILY};
use crate::platform;
use crate::strings::Strings;
use crate::tray::{Tray, TrayAction};
use crate::ui::{self, UiEnv, UiState};

/// Width of the invisible resize zones of a borderless window, in logical pixels.
const RESIZE_EDGE: f64 = 8.0;
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
const MAX_BACKGROUND_SIDE: u32 = 4096;
/// Touchpad scrolling distance that counts as one wheel notch, in physical pixels.
const WHEEL_PIXELS_PER_LINE: f64 = 40.0;

pub enum UserEvent {
    ShowWindow,
    Tray(TrayIconEvent),
    Menu(MenuId),
    /// The battery status changed; `None` when the computer has no battery.
    Battery(Option<BatteryStatus>),
}

struct Gfx {
    renderer: Renderer,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
}

#[derive(Default)]
struct Pointer {
    position: PhysicalPosition<f64>,
    press: Option<PhysicalPosition<f64>>,
    last_click: Option<(Instant, PhysicalPosition<f64>)>,
    resize: Option<ResizeDirection>,
}

/// What has already been pushed to the renderer and audio, to apply only changes.
#[derive(Default)]
struct Applied {
    digit_font: Option<(String, u16)>,
    date_font: Option<(String, u16)>,
    background: Option<Option<String>>,
    /// Whether egui has the date font, for the countdown description box.
    egui_date_font: bool,
}

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    config_dir: PathBuf,
    config: Config,
    saved_config: Config,
    save_deadline: Option<Instant>,
    save_warned: bool,
    strings: &'static Strings,
    fonts: FontCatalog,
    audio: Audio,
    motion: Motion,
    start: Instant,
    last_second: Option<i64>,
    date: Option<chrono::NaiveDate>,
    date_text: String,
    instance: wgpu::Instance,
    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    tray: Option<Tray>,
    tray_available: bool,
    hidden: bool,
    minimized: bool,
    modifiers: ModifiersState,
    pointer: Pointer,
    double_click: Duration,
    drag_threshold: f64,
    last_activity: Instant,
    ui: UiState,
    egui_repaint_at: Option<Instant>,
    applied: Applied,
    battery: Option<BatteryStatus>,
    /// Countdowns live for the session only: nothing about them is saved.
    countdowns: Countdowns,
    /// The countdown shown on the countdown screen, created on first use.
    current_countdown: Option<usize>,
    countdown_mode: bool,
    /// The tile the arrow keys edit while setting a countdown.
    selected_tile: Option<usize>,
    /// Mouse wheel movement not yet turned into digit steps, in lines.
    wheel: f64,
    /// Half-second phase last drawn while a finished countdown blinks.
    blink_drawn: Option<i64>,
}

impl App {
    pub fn new(
        proxy: EventLoopProxy<UserEvent>,
        config_dir: PathBuf,
        config: Config,
        strings: &'static Strings,
    ) -> Self {
        let now = Local::now();
        let motion = Motion::new(
            digits_of(now.hour(), now.minute(), now.second()),
            motion_settings(&config),
        );
        let backends = if cfg!(windows) {
            wgpu::Backends::DX12
        } else {
            wgpu::Backends::PRIMARY
        };
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor {
                backends,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            }
            .with_env(),
        );
        Self {
            proxy,
            config_dir,
            saved_config: config.clone(),
            config,
            save_deadline: None,
            save_warned: false,
            strings,
            fonts: FontCatalog::load_system(),
            audio: Audio::new(),
            motion,
            start: Instant::now(),
            last_second: None,
            date: None,
            date_text: String::new(),
            instance,
            window: None,
            gfx: None,
            tray: None,
            tray_available: platform::tray_available(),
            hidden: false,
            minimized: false,
            modifiers: ModifiersState::default(),
            pointer: Pointer::default(),
            double_click: platform::double_click_interval(),
            drag_threshold: platform::drag_threshold(),
            last_activity: Instant::now(),
            ui: UiState::default(),
            egui_repaint_at: None,
            applied: Applied::default(),
            battery: None,
            countdowns: Countdowns::default(),
            current_countdown: None,
            countdown_mode: false,
            selected_tile: None,
            wheel: 0.0,
            blink_drawn: None,
        }
    }

    fn time(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn visible(&self) -> bool {
        !self.hidden && !self.minimized
    }

    fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let monitors: Vec<Monitor> = event_loop
            .available_monitors()
            .map(|m| to_monitor(&m))
            .collect();
        let primary = event_loop.primary_monitor().map(|m| to_monitor(&m));
        let w = &self.config.window;
        let saved = match (w.x, w.y, w.width, w.height) {
            (Some(x), Some(y), Some(width), Some(height)) => Some(SavedWindow {
                x,
                y,
                width,
                height,
            }),
            _ => None,
        };
        let place = placement::place(saved, &monitors, primary);

        let icon_size = 64;
        let icon = winit::window::Icon::from_rgba(
            crate::icon_art::icon_rgba(icon_size),
            icon_size,
            icon_size,
        )
        .ok();
        let mut attrs = Window::default_attributes()
            .with_title("Flipclock")
            .with_inner_size(LogicalSize::new(place.size.0, place.size.1))
            .with_min_inner_size(LogicalSize::new(
                config::MIN_WINDOW_WIDTH,
                config::MIN_WINDOW_HEIGHT,
            ))
            .with_decorations(!w.borderless)
            .with_maximized(w.maximized)
            .with_window_level(level(w.always_on_top))
            .with_window_icon(icon);
        if let Some((x, y)) = place.position {
            attrs = attrs.with_position(PhysicalPosition::new(x, y));
        }
        let window = Arc::new(event_loop.create_window(attrs).map_err(|e| e.to_string())?);

        let size = window.inner_size();
        let surface = self
            .instance
            .create_surface(window.clone())
            .map_err(|e| format!("cannot create the window surface: {e}"))?;
        let renderer = Renderer::new(&self.instance, surface, size.width, size.height)?;

        let egui_ctx = egui::Context::default();
        egui_ctx.set_visuals(egui::Visuals::dark());
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(renderer.max_texture_size() as usize),
        );
        let egui_renderer = egui_wgpu::Renderer::new(
            renderer.device(),
            renderer.overlay_format(),
            egui_wgpu::RendererOptions::default(),
        );
        self.gfx = Some(Gfx {
            renderer,
            egui_ctx,
            egui_state,
            egui_renderer,
        });
        self.window = Some(window);
        Ok(())
    }

    // --- Clock ---------------------------------------------------------------

    /// Follow the wall clock: retarget the tiles on each new second, advance the
    /// animation and play the ticks of the flaps that landed.
    /// In countdown mode the tiles follow the shown countdown instead, with single
    /// flips. Countdowns advance even while the window is hidden, so they can finish.
    fn advance_clock(&mut self) {
        let t = self.time();
        for finished in self.countdowns.update(t) {
            self.on_countdown_finished(finished);
        }
        if !self.visible() {
            return;
        }
        let now = Local::now();
        let second = now.timestamp();
        if self.last_second != Some(second) {
            self.last_second = Some(second);
            if !self.countdown_mode {
                self.motion
                    .set_target(digits_of(now.hour(), now.minute(), now.second()), t);
            }
            self.refresh_date(now.date_naive());
            self.request_redraw();
        }
        if self.countdown_mode
            && let Some(cd) = self.countdown()
        {
            let digits = cd.digits(t);
            self.motion.set_target_direct(digits, t);
        }
        let landed = self.motion.update(t);
        if landed > 0 && self.config.sound.enabled {
            self.audio.play_ticks(landed, self.config.sound.volume);
        }
    }

    fn refresh_date(&mut self, date: chrono::NaiveDate) {
        if self.date != Some(date) {
            self.date = Some(date);
            self.date_text = format_date(self.config.date.language, date);
        }
    }

    /// Jump to the current time (or countdown) without animation (startup, window shown again).
    fn snap_to_now(&mut self) {
        let now = Local::now();
        let digits = match self.countdown().filter(|_| self.countdown_mode) {
            Some(cd) => cd.digits(self.time()),
            None => digits_of(now.hour(), now.minute(), now.second()),
        };
        self.motion.snap(digits);
        self.last_second = Some(now.timestamp());
        self.refresh_date(now.date_naive());
    }

    // --- Countdown -------------------------------------------------------------

    fn countdown(&self) -> Option<&Countdown> {
        self.current_countdown.map(|i| self.countdowns.get(i))
    }

    fn countdown_mut(&mut self) -> Option<&mut Countdown> {
        self.current_countdown.map(|i| self.countdowns.get_mut(i))
    }

    /// The logical size of the window minus the open part of the settings panel.
    fn clock_area(&self, width: f32, height: f32) -> (f32, f32) {
        let panel = ui::PANEL_WIDTH * self.ui.panel_fraction;
        ((width - panel).max(1.0), height)
    }

    fn countdown_layout(&self, width: f32, height: f32) -> CountdownLayout {
        let c = &self.config;
        countdown::layout(
            width,
            height,
            c.layout.margin,
            c.clock.seconds_scale,
            c.date.size,
        )
    }

    fn enter_countdown(&mut self) {
        if self.countdown_mode {
            return;
        }
        self.countdown_mode = true;
        if self.current_countdown.is_none() {
            self.current_countdown = Some(self.countdowns.add());
        }
        self.retarget_countdown();
    }

    /// Back to the clock; running countdowns keep going in the background.
    fn exit_countdown(&mut self) {
        if !self.countdown_mode {
            return;
        }
        self.countdown_mode = false;
        self.selected_tile = None;
        let now = Local::now();
        let t = self.time();
        self.motion
            .set_target_direct(digits_of(now.hour(), now.minute(), now.second()), t);
        self.request_redraw();
    }

    /// Flip the tiles to the shown countdown's digits.
    fn retarget_countdown(&mut self) {
        let t = self.time();
        if let Some(digits) = self.countdown().map(|cd| cd.digits(t)) {
            self.motion.set_target_direct(digits, t);
        }
        self.request_redraw();
    }

    fn step_tile(&mut self, tile: usize, delta: i32) {
        if let Some(cd) = self.countdown_mut() {
            cd.step_digit(tile, delta);
        }
        self.retarget_countdown();
    }

    fn countdown_idle(&self) -> bool {
        self.countdown_mode
            && self
                .countdown()
                .is_some_and(|cd| cd.state() == CountdownState::Idle)
    }

    fn handle_countdown_actions(&mut self, a: CountdownActions) {
        if !(a.start || a.pause_resume || a.reset || a.step.is_some()) {
            return;
        }
        let t = self.time();
        let Some(cd) = self.countdown_mut() else {
            return;
        };
        if a.start {
            cd.start(t);
        }
        if a.pause_resume {
            match cd.state() {
                CountdownState::Running => cd.pause(t),
                CountdownState::Paused => cd.resume(t),
                _ => {}
            }
        }
        if a.reset {
            cd.reset();
        }
        if let Some((tile, delta)) = a.step {
            cd.step_digit(tile, delta);
        }
        self.retarget_countdown();
    }

    fn on_countdown_finished(&mut self, index: usize) {
        // The alarm ignores "mute tick": muting is about the ticking, not about missing the end.
        self.audio.play_alarm(self.config.sound.volume);
        if !self.visible() {
            let cd = self.countdowns.get(index);
            let body = if cd.description.trim().is_empty() {
                format_duration(cd.duration())
            } else {
                cd.description.clone()
            };
            crate::tray::notify(self.strings.countdown_finished, &body);
        } else if self.current_countdown == Some(index) {
            self.enter_countdown();
        }
        self.request_redraw();
    }

    /// The tile under a point (logical pixels), arrow bands included.
    fn tile_at(&self, x: f32, y: f32) -> Option<usize> {
        let window = self.window.as_ref()?;
        let logical = window.inner_size().to_logical::<f32>(window.scale_factor());
        let (w, h) = self.clock_area(logical.width, logical.height);
        let l = self.countdown_layout(w, h);
        let band = l.arrow_size * 1.5;
        l.clock
            .tiles
            .iter()
            .position(|t| x >= t.x && x <= t.right() && y >= t.y - band && y <= t.bottom() + band)
    }

    fn on_wheel(&mut self, lines: f64) {
        if !self.countdown_idle() {
            return;
        }
        let Some(window) = &self.window else { return };
        let p = self
            .pointer
            .position
            .to_logical::<f32>(window.scale_factor());
        let Some(tile) = self.tile_at(p.x, p.y) else {
            return;
        };
        self.selected_tile = Some(tile);
        self.wheel += lines;
        while self.wheel.abs() >= 1.0 {
            let step = self.wheel.signum();
            self.wheel -= step;
            self.step_tile(tile, step as i32);
        }
    }

    /// Arrow keys while setting a countdown: ←→ pick a tile, ↑↓ change it.
    fn on_countdown_key(&mut self, key: KeyCode) -> bool {
        if !self.countdown_idle() {
            return false;
        }
        let last = TILE_COUNT - 1;
        let selected = self.selected_tile;
        match key {
            KeyCode::ArrowLeft => {
                self.selected_tile = Some(selected.map_or(last, |i| i.saturating_sub(1)))
            }
            KeyCode::ArrowRight => {
                self.selected_tile = Some(selected.map_or(last, |i| (i + 1).min(last)))
            }
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                let tile = selected.unwrap_or(last);
                self.selected_tile = Some(tile);
                self.step_tile(tile, if key == KeyCode::ArrowUp { 1 } else { -1 });
            }
            _ => return false,
        }
        self.request_redraw();
        true
    }

    // --- Settings ------------------------------------------------------------

    /// Make `new` the current configuration and push every change to the
    /// renderer, audio, window and tray.
    fn apply_config(&mut self, new: Config) {
        let new = new.validated();
        let old = std::mem::replace(&mut self.config, new);
        let c = &self.config;
        self.motion.set_settings(motion_settings(c));

        if let Some(gfx) = self.gfx.as_mut() {
            let digit_font = (c.fonts.digits_family.clone(), c.fonts.digits_weight);
            if self.applied.digit_font.as_ref() != Some(&digit_font) {
                gfx.renderer
                    .set_digit_font(self.fonts.resolve(&digit_font.0, digit_font.1));
                self.applied.digit_font = Some(digit_font);
            }
            let date_font = (c.fonts.date_family.clone(), c.fonts.date_weight);
            if self.applied.date_font.as_ref() != Some(&date_font) {
                let font = self.fonts.resolve(&date_font.0, date_font.1);
                self.applied.egui_date_font = register_egui_date_font(&gfx.egui_ctx, font.as_ref());
                gfx.renderer.set_date_font(font);
                self.applied.date_font = Some(date_font);
            }
            if self.applied.background.as_ref() != Some(&c.background.image_path) {
                let max = gfx.renderer.max_texture_size().min(MAX_BACKGROUND_SIDE);
                let image =
                    c.background.image_path.as_deref().and_then(|p| {
                        flipclock_render::load_background(std::path::Path::new(p), max)
                    });
                gfx.renderer.set_background_image(image.as_ref());
                self.applied.background = Some(c.background.image_path.clone());
            }
        }
        self.audio.set_custom_tick(c.sound.custom_path.as_deref());

        let (borderless, on_top) = (c.window.borderless, c.window.always_on_top);
        if old.date.language != self.config.date.language
            && let Some(d) = self.date.take()
        {
            self.refresh_date(d);
        }
        if let Some(window) = self.window.clone() {
            if old.window.borderless != borderless {
                window.set_decorations(!borderless);
                self.reset_cursor();
            }
            if old.window.always_on_top != on_top {
                window.set_window_level(level(on_top));
            }
        }
        self.sync_tray();
        self.schedule_save();
        self.request_redraw();
    }

    fn sync_tray(&self) {
        if let Some(tray) = &self.tray {
            let w = &self.config.window;
            tray.sync(w.always_on_top, w.borderless, !self.config.sound.enabled);
        }
    }

    fn update_config(&mut self, f: impl FnOnce(&mut Config)) {
        let mut c = self.config.clone();
        f(&mut c);
        self.apply_config(c);
    }

    fn schedule_save(&mut self) {
        self.save_deadline = Some(Instant::now() + SAVE_DEBOUNCE);
    }

    fn save_now(&mut self) {
        self.save_deadline = None;
        if self.config == self.saved_config {
            return;
        }
        match config::save(&self.config_dir, &self.config) {
            Ok(()) => self.saved_config = self.config.clone(),
            Err(e) => {
                log::warn!("cannot save config in {}: {e}", self.config_dir.display());
                if !self.save_warned {
                    self.save_warned = true;
                    self.ui.banner = Some(self.strings.save_failed.to_owned());
                    self.request_redraw();
                }
            }
        }
    }

    /// Remember the window's normal geometry and maximized state.
    fn record_geometry(&mut self) {
        let Some(window) = &self.window else { return };
        if !self.visible() || window.fullscreen().is_some() || window.is_minimized() == Some(true) {
            return;
        }
        let maximized = window.is_maximized();
        let w = &mut self.config.window;
        w.maximized = maximized;
        if !maximized {
            if let Ok(pos) = window.outer_position() {
                w.x = Some(pos.x);
                w.y = Some(pos.y);
            }
            let size = window.inner_size().to_logical::<f64>(window.scale_factor());
            if size.width > 0.0 && size.height > 0.0 {
                w.width = Some(size.width);
                w.height = Some(size.height);
            }
        }
        self.schedule_save();
    }

    // --- Window visibility -----------------------------------------------------

    fn update_minimized(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let minimized = window.is_minimized().unwrap_or(false);
        if minimized && !self.minimized {
            self.minimized = true;
            if self.tray_available && self.tray.is_some() {
                window.set_visible(false);
                self.hidden = true;
                platform::set_dock_icon_visible(false);
            }
        } else if !minimized && self.minimized && !self.hidden {
            self.minimized = false;
            self.snap_to_now();
            self.request_redraw();
        }
    }

    fn show_window(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        platform::set_dock_icon_visible(true);
        window.set_visible(true);
        window.set_minimized(false);
        window.focus_window();
        if !self.visible() {
            self.hidden = false;
            self.minimized = false;
            self.snap_to_now();
        }
        self.request_redraw();
    }

    fn hide_window(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        window.set_visible(false);
        self.hidden = true;
        platform::set_dock_icon_visible(false);
    }

    fn toggle_show_hide(&mut self) {
        if self.visible() {
            self.hide_window();
        } else {
            self.show_window();
        }
    }

    fn toggle_fullscreen(&mut self) {
        let Some(window) = &self.window else { return };
        let fullscreen = window.fullscreen().is_some();
        window.set_fullscreen((!fullscreen).then_some(Fullscreen::Borderless(None)));
        self.pointer.press = None;
        self.reset_cursor();
    }

    fn exit(&mut self, event_loop: &ActiveEventLoop) {
        self.record_geometry();
        self.save_now();
        event_loop.exit();
    }

    // --- Input -----------------------------------------------------------------

    fn borderless_frame_active(&self) -> bool {
        let Some(window) = &self.window else {
            return false;
        };
        self.config.window.borderless && window.fullscreen().is_none() && !window.is_maximized()
    }

    fn reset_cursor(&mut self) {
        if self.pointer.resize.take().is_some()
            && let Some(w) = &self.window
        {
            w.set_cursor(CursorIcon::Default);
        }
    }

    fn on_cursor_moved(&mut self, position: PhysicalPosition<f64>, egui_wants_pointer: bool) {
        let Some(window) = self.window.clone() else {
            return;
        };
        if self.last_activity.elapsed().as_secs_f32() > 2.5 {
            window.request_redraw();
        }
        self.last_activity = Instant::now();
        self.pointer.position = position;

        let resize = if self.borderless_frame_active() && !egui_wants_pointer {
            let size = window.inner_size();
            hit_test(
                position,
                size.width as f64,
                size.height as f64,
                RESIZE_EDGE * window.scale_factor(),
            )
        } else {
            None
        };
        if resize != self.pointer.resize {
            self.pointer.resize = resize;
            window.set_cursor(resize.map_or(CursorIcon::Default, resize_cursor));
        }

        if let Some(press) = self.pointer.press {
            let (dx, dy) = (position.x - press.x, position.y - press.y);
            if (dx * dx + dy * dy).sqrt() > self.drag_threshold {
                self.pointer.press = None;
                self.pointer.last_click = None;
                if let Err(e) = window.drag_window() {
                    log::debug!("drag_window: {e}");
                }
            }
        }
    }

    fn on_left_button(&mut self, state: ElementState) {
        let Some(window) = self.window.clone() else {
            return;
        };
        if state == ElementState::Released {
            self.pointer.press = None;
            return;
        }
        if let Some(direction) = self
            .pointer
            .resize
            .filter(|_| self.borderless_frame_active())
        {
            if let Err(e) = window.drag_resize_window(direction) {
                log::debug!("drag_resize_window: {e}");
            }
            return;
        }
        let now = Instant::now();
        let pos = self.pointer.position;
        if let Some((at, p)) = self.pointer.last_click {
            let close = (pos.x - p.x).abs() <= self.drag_threshold * 2.0
                && (pos.y - p.y).abs() <= self.drag_threshold * 2.0;
            if now.duration_since(at) <= self.double_click && close {
                self.pointer.last_click = None;
                self.toggle_fullscreen();
                return;
            }
        }
        self.pointer.last_click = Some((now, pos));
        if self.borderless_frame_active() {
            self.pointer.press = Some(pos);
        }
    }

    fn on_key(&mut self, key: KeyCode, repeat: bool, event_loop: &ActiveEventLoop) {
        if self.on_countdown_key(key) || repeat {
            return;
        }
        let mac = cfg!(target_os = "macos");
        let command = if mac {
            self.modifiers.super_key()
        } else {
            self.modifiers.control_key()
        };
        match key {
            KeyCode::F11 => self.toggle_fullscreen(),
            KeyCode::KeyB if command => {
                self.update_config(|c| c.window.borderless = !c.window.borderless)
            }
            KeyCode::KeyF if mac && self.modifiers.super_key() && self.modifiers.control_key() => {
                self.toggle_fullscreen();
            }
            KeyCode::KeyQ if mac && self.modifiers.super_key() => self.exit(event_loop),
            _ => {}
        }
    }

    // --- Drawing ---------------------------------------------------------------

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        if !self.visible() {
            return;
        }
        self.advance_clock();
        let Some(window) = self.window.clone() else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let scale = window.scale_factor();
        let logical = size.to_logical::<f32>(scale);
        let t = self.start.elapsed().as_secs_f64();

        // The open settings panel pushes the clock left instead of covering it.
        // Its width comes from the previous egui pass; while it slides, egui repaints every frame.
        let (area_width, area_height) = self.clock_area(logical.width, logical.height);
        let c = &self.config;
        let countdown_layout = self
            .countdown_mode
            .then(|| self.countdown_layout(area_width, area_height));
        let mut layout = match &countdown_layout {
            Some(l) => l.clock.clone(),
            None => layout::compute(&LayoutInput {
                width: area_width,
                height: area_height,
                margin: c.layout.margin,
                seconds_scale: c.clock.seconds_scale,
                date_visible: c.date.visible,
                date_size: c.date.size,
                date_position: c.date.position,
            }),
        };

        // Countdown: once started, the description reads like the date, drawn by the
        // renderer's date line with the date font, weight and color.
        let mut text_line = c.date.visible.then(|| self.date_text.clone());
        let mut blink_off = false;
        let current = self.current_countdown.map(|i| self.countdowns.get(i));
        if let (Some(l), Some(cd)) = (&countdown_layout, current) {
            text_line = None;
            if cd.state() != CountdownState::Idle && !cd.description.trim().is_empty() {
                text_line = Some(cd.description.clone());
                layout.date = Some(layout::DateLayout {
                    anchor_x: l.clock.clock.x + l.clock.clock.w / 2.0,
                    center_y: l.description_center_y,
                    height: l.text_size / 0.8,
                    position: DatePosition::Center,
                });
            }
            // A finished countdown blinks by drawing its digits in the tile color.
            blink_off = cd.state() == CountdownState::Finished && blink_phase(t) % 2 == 1;
        }

        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let raw = gfx.egui_state.take_egui_input(&window);
        let env = UiEnv {
            strings: self.strings,
            fonts: &self.fonts,
            show_exit: self.tray.is_none(),
            fullscreen: window.fullscreen().is_some(),
            idle: self.last_activity.elapsed().as_secs_f32(),
            battery: self.battery,
            time: self.start.elapsed().as_secs_f32(),
        };
        let mut draft = self.config.clone();
        let mut actions = ui::UiOutput::default();
        let ui_state = &mut self.ui;
        let [r, g, b] = c.colors.date.0;
        let text_color = egui::Color32::from_rgb(r, g, b);
        let mut countdown = match (&countdown_layout, self.current_countdown) {
            (Some(l), Some(i)) => Some((l, self.countdowns.get_mut(i))),
            _ => None,
        };
        let (selected, date_font) = (self.selected_tile, self.applied.egui_date_font);
        let egui::FullOutput {
            platform_output,
            mut textures_delta,
            shapes,
            pixels_per_point,
            viewport_output,
        } = gfx.egui_ctx.run_pass(raw, |ctx| {
            let view = countdown.as_mut().map(|(l, cd)| CountdownView {
                layout: l,
                state: cd.state(),
                can_start: cd.can_start(),
                selected,
                description: &mut cd.description,
                color: text_color,
                date_font,
            });
            actions = ui::draw(ctx, ui_state, &mut draft, &env, view);
        });
        gfx.egui_state
            .handle_platform_output(&window, platform_output);
        let paint_jobs = gfx.egui_ctx.tessellate(shapes, pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point,
        };
        for (id, deltas) in &textures_delta.set {
            for delta in deltas {
                gfx.egui_renderer.update_texture(
                    gfx.renderer.device(),
                    gfx.renderer.queue(),
                    *id,
                    delta,
                );
            }
        }
        self.egui_repaint_at = viewport_output
            .get(&egui::ViewportId::ROOT)
            .and_then(|v| Instant::now().checked_add(v.repaint_delay));

        // 3D scene, then egui on top
        let tiles = std::array::from_fn(|i| self.motion.visual(i, t));
        let mut style = style(&self.config);
        if blink_off {
            style.digit_color = style.tile_color;
        }
        let input = FrameInput {
            layout: &layout,
            scale_factor: scale as f32,
            tiles,
            style: &style,
            date_text: text_line.as_deref(),
        };
        let egui_renderer = &mut gfx.egui_renderer;
        gfx.renderer.render(&input, |device, queue, encoder, view| {
            let buffers =
                egui_renderer.update_buffers(device, queue, encoder, &paint_jobs, &screen);
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            egui_renderer.render(&mut pass, &paint_jobs, &screen);
            buffers
        });
        for id in &textures_delta.free {
            gfx.egui_renderer.free_texture(id);
        }
        textures_delta.clear();

        if draft != self.config {
            self.apply_config(draft);
        }
        self.handle_ui_actions(actions, event_loop);
    }

    fn handle_ui_actions(&mut self, a: ui::UiOutput, event_loop: &ActiveEventLoop) {
        let s = self.strings;
        if a.toggle_countdown {
            if self.countdown_mode {
                self.exit_countdown();
            } else {
                self.enter_countdown();
            }
        }
        self.handle_countdown_actions(a.countdown);
        if a.test_tick {
            self.audio.play_ticks(1, self.config.sound.volume);
        }
        if a.test_flip {
            let t = self.time();
            self.motion.test_flip(t);
            self.request_redraw();
        }
        if a.reset {
            // Keep where the window is; only the look and behavior reset.
            let mut window = self.config.window.clone();
            window.borderless = false;
            window.always_on_top = false;
            self.apply_config(Config {
                window,
                ..Config::default()
            });
        }
        if a.pick_background
            && let Some(path) = rfd::FileDialog::new()
                .add_filter(
                    s.images_filter,
                    &["png", "jpg", "jpeg", "webp", "bmp", "gif"],
                )
                .pick_file()
        {
            let path = path.to_string_lossy().into_owned();
            self.update_config(|c| c.background.image_path = Some(path));
        }
        if a.clear_background {
            self.update_config(|c| c.background.image_path = None);
        }
        if a.pick_sound
            && let Some(path) = rfd::FileDialog::new()
                .add_filter(s.sound_filter, &["mp3"])
                .pick_file()
        {
            let path = path.to_string_lossy().into_owned();
            self.update_config(|c| c.sound.custom_path = Some(path));
        }
        if a.clear_sound {
            self.update_config(|c| c.sound.custom_path = None);
        }
        if a.toggle_fullscreen {
            self.toggle_fullscreen();
        }
        if a.exit {
            self.exit(event_loop);
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(e) = self.create_window(event_loop) {
            log::error!("{e}");
            crate::show_fatal_error(self.strings, &e);
            event_loop.exit();
            return;
        }
        crate::tray::forward_events(self.proxy.clone());
        crate::power::watch(self.proxy.clone());
        if self.tray_available {
            let w = &self.config.window;
            self.tray = Tray::new(
                self.strings,
                w.always_on_top,
                w.borderless,
                !self.config.sound.enabled,
            );
        }
        let config = self.config.clone();
        self.apply_config(config);
        self.save_deadline = None;
        self.snap_to_now();
        if cfg!(debug_assertions) && std::env::var_os("FLIPCLOCK_DEBUG_OPEN_SETTINGS").is_some() {
            self.ui.panel_open = true;
        }
        self.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let (consumed, egui_wants_pointer, egui_wants_keyboard) = match self.gfx.as_mut() {
            Some(gfx) => {
                let response = gfx.egui_state.on_window_event(&window, &event);
                if response.repaint {
                    window.request_redraw();
                }
                (
                    response.consumed,
                    gfx.egui_ctx.egui_wants_pointer_input(),
                    gfx.egui_ctx.egui_wants_keyboard_input(),
                )
            }
            None => (false, false, false),
        };

        match event {
            WindowEvent::CloseRequested => self.exit(event_loop),
            WindowEvent::Resized(size) => {
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.renderer.resize(size.width, size.height);
                }
                self.update_minimized();
                self.record_geometry();
                window.request_redraw();
            }
            WindowEvent::Moved(_) => self.record_geometry(),
            WindowEvent::ScaleFactorChanged { .. } => window.request_redraw(),
            WindowEvent::Occluded(_) | WindowEvent::Focused(_) => self.update_minimized(),
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::CursorMoved { position, .. } => {
                self.on_cursor_moved(position, egui_wants_pointer)
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if !consumed || state == ElementState::Released {
                    self.on_left_button(state);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed
                    && !egui_wants_keyboard
                    && let PhysicalKey::Code(code) = event.physical_key
                {
                    self.on_key(code, event.repeat, event_loop);
                }
            }
            WindowEvent::MouseWheel { delta, .. } if !egui_wants_pointer => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    MouseScrollDelta::PixelDelta(p) => p.y / WHEEL_PIXELS_PER_LINE,
                };
                self.on_wheel(lines);
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::ShowWindow => self.show_window(),
            UserEvent::Battery(status) => {
                self.battery = status;
                self.request_redraw();
            }
            UserEvent::Tray(e) => match e {
                TrayIconEvent::DoubleClick {
                    button: tray_icon::MouseButton::Left,
                    ..
                } => self.toggle_show_hide(),
                #[cfg(all(unix, not(target_os = "macos")))]
                TrayIconEvent::Click {
                    button: tray_icon::MouseButton::Left,
                    button_state: tray_icon::MouseButtonState::Up,
                    ..
                } => self.toggle_show_hide(),
                _ => {}
            },
            UserEvent::Menu(id) => {
                let Some(action) = self.tray.as_ref().and_then(|t| t.action(&id)) else {
                    return;
                };
                match action {
                    TrayAction::ShowHide => self.toggle_show_hide(),
                    TrayAction::Countdown => {
                        self.show_window();
                        self.enter_countdown();
                    }
                    TrayAction::Settings => {
                        self.show_window();
                        self.ui.panel_open = true;
                    }
                    TrayAction::AlwaysOnTop => {
                        self.update_config(|c| c.window.always_on_top = !c.window.always_on_top)
                    }
                    TrayAction::Borderless => {
                        self.update_config(|c| c.window.borderless = !c.window.borderless)
                    }
                    TrayAction::Mute => self.update_config(|c| c.sound.enabled = !c.sound.enabled),
                    TrayAction::Exit => self.exit(event_loop),
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.advance_clock();
        let now = Instant::now();
        if self.save_deadline.is_some_and(|d| now >= d) {
            self.save_now();
        }
        let mut wake = self.save_deadline;
        let t = self.time();
        // Countdowns keep counting while hidden: wake for their next change or end.
        if let Some(at) = self.countdowns.next_change(t) {
            let wait = Duration::from_secs_f64((at - t).max(0.0)) + Duration::from_millis(3);
            wake = earliest(wake, Some(now + wait));
        }
        if self.visible() {
            if self.motion.is_animating() {
                self.request_redraw();
            }
            let blinking = self.countdown_mode
                && self
                    .countdown()
                    .is_some_and(|cd| cd.state() == CountdownState::Finished);
            if blinking {
                let phase = blink_phase(t);
                if self.blink_drawn != Some(phase) {
                    self.blink_drawn = Some(phase);
                    self.request_redraw();
                }
                let next = (phase + 1) as f64 / 2.0;
                wake = earliest(wake, Some(now + Duration::from_secs_f64(next - t)));
            }
            if let Some(at) = self.egui_repaint_at {
                if at <= now {
                    self.egui_repaint_at = None;
                    self.request_redraw();
                } else {
                    wake = earliest(wake, Some(at));
                }
            }
            wake = earliest(wake, Some(next_second(now)));
        }
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

fn earliest(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

/// The instant just after the next wall-clock second boundary.
fn next_second(now: Instant) -> Instant {
    let nanos = Local::now().timestamp_subsec_nanos().min(999_999_999);
    now + Duration::from_nanos(u64::from(1_000_000_000 - nanos)) + Duration::from_millis(1)
}

/// Half-second slots since start: a finished countdown hides its digits in odd ones.
fn blink_phase(t: f64) -> i64 {
    (t * 2.0).floor() as i64
}

fn format_duration(seconds: u32) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// Make the date font available to egui as the `date` family (for the countdown
/// description box). Returns whether it was registered.
fn register_egui_date_font(ctx: &egui::Context, font: Option<&LoadedFont>) -> bool {
    let mut fonts = egui::FontDefinitions::default();
    let Some(font) = font else {
        ctx.set_fonts(fonts);
        return false;
    };
    let mut data = egui::FontData::from_owned(font.data.as_ref().clone());
    data.index = font.index as u32;
    fonts
        .font_data
        .insert(DATE_FAMILY.to_owned(), Arc::new(data));
    // Characters the date font lacks fall back to egui's own fonts.
    let mut family = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    family.insert(0, DATE_FAMILY.to_owned());
    fonts
        .families
        .insert(egui::FontFamily::Name(DATE_FAMILY.into()), family);
    ctx.set_fonts(fonts);
    true
}

fn motion_settings(c: &Config) -> MotionSettings {
    MotionSettings {
        flip_duration: f64::from(c.animation.flip_duration_ms) / 1000.0,
        cascade_delay: f64::from(c.animation.cascade_delay_ms) / 1000.0,
        bounce: c.animation.bounce,
    }
}

fn style(c: &Config) -> Style {
    Style {
        tile_color: c.colors.tile.to_linear(),
        digit_color: c.colors.digits.to_linear(),
        date_color: c.colors.date.to_linear(),
        background_color: c.colors.background.to_linear(),
        finish: c.lighting.finish,
        light_angle_deg: c.lighting.light_angle_deg,
        light_intensity: c.lighting.light_intensity,
        background_dim: c.background.dim,
    }
}

fn level(always_on_top: bool) -> WindowLevel {
    if always_on_top {
        WindowLevel::AlwaysOnTop
    } else {
        WindowLevel::Normal
    }
}

fn to_monitor(m: &winit::monitor::MonitorHandle) -> Monitor {
    let pos = m.position();
    let size = m.size();
    Monitor {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
        scale: m.scale_factor(),
    }
}

/// Which resize edge, if any, the pointer is over in a borderless window.
fn hit_test(
    p: PhysicalPosition<f64>,
    width: f64,
    height: f64,
    edge: f64,
) -> Option<ResizeDirection> {
    let left = p.x < edge;
    let right = p.x > width - edge;
    let top = p.y < edge;
    let bottom = p.y > height - edge;
    Some(match (left, right, top, bottom) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (_, true, true, _) => ResizeDirection::NorthEast,
        (true, _, _, true) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, ..) => ResizeDirection::West,
        (_, true, ..) => ResizeDirection::East,
        (_, _, true, _) => ResizeDirection::North,
        (_, _, _, true) => ResizeDirection::South,
        _ => return None,
    })
}

fn resize_cursor(d: ResizeDirection) -> CursorIcon {
    match d {
        ResizeDirection::East => CursorIcon::EResize,
        ResizeDirection::West => CursorIcon::WResize,
        ResizeDirection::North => CursorIcon::NResize,
        ResizeDirection::South => CursorIcon::SResize,
        ResizeDirection::NorthEast => CursorIcon::NeResize,
        ResizeDirection::NorthWest => CursorIcon::NwResize,
        ResizeDirection::SouthEast => CursorIcon::SeResize,
        ResizeDirection::SouthWest => CursorIcon::SwResize,
    }
}
