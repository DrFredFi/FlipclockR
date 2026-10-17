//! egui overlay: the cog button, the slide-in settings panel and the warning banner.

use std::collections::HashMap;

use egui::{Align2, Color32, ComboBox, Id, RichText, ScrollArea, Slider};
use flipclock_core::config::{Config, DatePosition};
use flipclock_core::date::DateLanguage;
use flipclock_render::fonts::FontCatalog;

use crate::strings::Strings;

const PANEL_WIDTH: f32 = 340.0;
const COG_IDLE_SECONDS: f32 = 3.0;
const COG_FADE_SECONDS: f32 = 0.4;
const STANDARD_WEIGHTS: [u16; 9] = [100, 200, 300, 400, 500, 600, 700, 800, 900];

#[derive(Default)]
pub struct UiState {
    pub panel_open: bool,
    pub banner: Option<String>,
    confirm_reset: bool,
    weights: HashMap<String, Vec<u16>>,
}

/// What the user asked for during this frame, beyond plain setting changes.
#[derive(Default)]
pub struct UiOutput {
    pub test_tick: bool,
    pub test_flip: bool,
    pub reset: bool,
    pub pick_background: bool,
    pub clear_background: bool,
    pub pick_sound: bool,
    pub clear_sound: bool,
    pub toggle_fullscreen: bool,
    pub exit: bool,
}

pub struct UiEnv<'a> {
    pub strings: &'static Strings,
    pub fonts: &'a FontCatalog,
    /// Whether to offer an Exit button (when there is no tray menu).
    pub show_exit: bool,
    pub fullscreen: bool,
    /// Seconds since the pointer last moved over the window.
    pub idle: f32,
}

pub fn draw(
    ctx: &egui::Context,
    state: &mut UiState,
    config: &mut Config,
    env: &UiEnv<'_>,
) -> UiOutput {
    let mut out = UiOutput::default();
    cog(ctx, state, env);
    panel(ctx, state, config, env, &mut out);
    banner(ctx, state, env);
    out
}

fn cog(ctx: &egui::Context, state: &mut UiState, env: &UiEnv<'_>) {
    let opacity = if state.panel_open || env.idle < COG_IDLE_SECONDS {
        1.0
    } else {
        1.0 - ((env.idle - COG_IDLE_SECONDS) / COG_FADE_SECONDS).clamp(0.0, 1.0)
    };
    if env.idle < COG_IDLE_SECONDS {
        ctx.request_repaint_after_secs(COG_IDLE_SECONDS - env.idle);
    } else if opacity > 0.0 {
        ctx.request_repaint();
    }
    if opacity <= 0.0 || state.panel_open {
        return;
    }
    egui::Area::new(Id::unique("cog"))
        .anchor(Align2::RIGHT_TOP, [-14.0, 14.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(opacity);
            let button = egui::Button::new(RichText::new("⚙").size(22.0))
                .fill(Color32::from_black_alpha(110))
                .corner_radius(18.0)
                .min_size([36.0, 36.0].into());
            if ui.add(button).clicked() {
                state.panel_open = true;
            }
        });
}

fn banner(ctx: &egui::Context, state: &mut UiState, env: &UiEnv<'_>) {
    let Some(message) = state.banner.clone() else {
        return;
    };
    egui::Area::new(Id::unique("banner"))
        .anchor(Align2::CENTER_TOP, [0.0, 14.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(Color32::from_rgb(0x5a, 0x45, 0x10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("⚠ {message}")).color(Color32::WHITE));
                        if ui.button(env.strings.close).clicked() {
                            state.banner = None;
                        }
                    });
                });
        });
}

fn panel(
    ctx: &egui::Context,
    state: &mut UiState,
    config: &mut Config,
    env: &UiEnv<'_>,
    out: &mut UiOutput,
) {
    let t = ctx.animate_bool_with_time(Id::unique("panel-open"), state.panel_open, 0.22);
    if t <= 0.0 {
        return;
    }
    let screen = ctx.content_rect();
    let s = env.strings;
    egui::Area::new(Id::unique("panel"))
        .fixed_pos([screen.right() - PANEL_WIDTH * t, screen.top()])
        .order(egui::Order::Foreground)
        .constrain(false)
        .show(ctx, |ui| {
            egui::Frame::window(ui.style())
                .corner_radius(0.0)
                .show(ui, |ui| {
                    ui.set_width(PANEL_WIDTH - 28.0);
                    ui.set_height(screen.height() - 28.0);
                    ui.horizontal(|ui| {
                        ui.heading(s.settings_title);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(RichText::new("×").size(18.0)).clicked() {
                                state.panel_open = false;
                                state.confirm_reset = false;
                            }
                        });
                    });
                    ui.separator();
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            sections(ui, state, config, env, out);
                        });
                });
        });
}

fn sections(
    ui: &mut egui::Ui,
    state: &mut UiState,
    c: &mut Config,
    env: &UiEnv<'_>,
    out: &mut UiOutput,
) {
    let s = env.strings;

    section(ui, s.section_clock, |ui| {
        ui.add(
            Slider::new(&mut c.clock.seconds_scale, 0.3..=1.0)
                .text(s.seconds_size)
                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                .custom_parser(|t| {
                    t.trim_end_matches('%')
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .map(|v| v / 100.0)
                }),
        );
    });

    section(ui, s.section_animation, |ui| {
        ui.add(
            Slider::new(&mut c.animation.flip_duration_ms, 150..=1000)
                .text(s.flip_duration)
                .suffix(" ms"),
        );
        ui.add(
            Slider::new(&mut c.animation.cascade_delay_ms, 0..=200)
                .text(s.cascade_delay)
                .suffix(" ms"),
        );
        ui.add(Slider::new(&mut c.animation.bounce, 0.0..=1.0).text(s.bounce));
        if ui.button(s.test_flip).clicked() {
            out.test_flip = true;
        }
    });

    section(ui, s.section_lighting, |ui| {
        ui.add(Slider::new(&mut c.lighting.finish, 0.0..=1.0).text(s.finish));
        ui.add(
            Slider::new(&mut c.lighting.light_angle_deg, 0.0..=359.0)
                .text(s.light_angle)
                .suffix("°"),
        );
        ui.add(Slider::new(&mut c.lighting.light_intensity, 0.0..=2.0).text(s.light_intensity));
    });

    section(ui, s.section_colors, |ui| {
        color_row(ui, s.color_tile, &mut c.colors.tile.0);
        color_row(ui, s.color_digits, &mut c.colors.digits.0);
        color_row(ui, s.color_date, &mut c.colors.date.0);
        color_row(ui, s.color_background, &mut c.colors.background.0);
    });

    section(ui, s.section_fonts, |ui| {
        font_picker(
            ui,
            state,
            env,
            "digits",
            s.font_digits,
            &mut c.fonts.digits_family,
            &mut c.fonts.digits_weight,
        );
        font_picker(
            ui,
            state,
            env,
            "date",
            s.font_date,
            &mut c.fonts.date_family,
            &mut c.fonts.date_weight,
        );
    });

    section(ui, s.section_background, |ui| {
        let label = c
            .background
            .image_path
            .as_deref()
            .map(file_name)
            .unwrap_or_else(|| s.no_image.to_owned());
        ui.label(label);
        ui.horizontal(|ui| {
            if ui.button(s.choose_image).clicked() {
                out.pick_background = true;
            }
            if c.background.image_path.is_some() && ui.button(s.remove).clicked() {
                out.clear_background = true;
            }
        });
        ui.add(
            Slider::new(&mut c.background.dim, 0.0..=0.9)
                .text(s.dim)
                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
        );
    });

    section(ui, s.section_date, |ui| {
        ui.checkbox(&mut c.date.visible, s.show_date);
        ComboBox::from_label(s.date_language)
            .selected_text(c.date.language.native_name())
            .show_ui(ui, |ui| {
                for lang in DateLanguage::ALL {
                    ui.selectable_value(&mut c.date.language, lang, lang.native_name());
                }
            });
        ui.horizontal(|ui| {
            ui.label(s.date_position);
            for (pos, name) in DatePosition::ALL.into_iter().zip([
                s.position_left,
                s.position_center,
                s.position_right,
            ]) {
                ui.selectable_value(&mut c.date.position, pos, name);
            }
        });
        ui.add(
            Slider::new(&mut c.date.size, 0.5..=2.0)
                .text(s.date_size)
                .suffix("×"),
        );
    });

    section(ui, s.section_layout, |ui| {
        ui.add(
            Slider::new(&mut c.layout.margin, 0.0..=500.0)
                .text(s.margin)
                .integer(),
        );
    });

    section(ui, s.section_sound, |ui| {
        ui.checkbox(&mut c.sound.enabled, s.tick_enabled);
        ui.add(
            Slider::new(&mut c.sound.volume, 0.0..=1.0)
                .text(s.volume)
                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
        );
        let label = c
            .sound
            .custom_path
            .as_deref()
            .map(file_name)
            .unwrap_or_else(|| s.default_sound.to_owned());
        ui.label(label);
        ui.horizontal(|ui| {
            if ui.button(s.choose_sound).clicked() {
                out.pick_sound = true;
            }
            if c.sound.custom_path.is_some() && ui.button(s.remove).clicked() {
                out.clear_sound = true;
            }
            if ui.button(s.test_tick).clicked() {
                out.test_tick = true;
            }
        });
    });

    section(ui, s.section_window, |ui| {
        ui.checkbox(&mut c.window.borderless, s.borderless);
        ui.checkbox(&mut c.window.always_on_top, s.always_on_top);
        let mut fullscreen = env.fullscreen;
        if ui.checkbox(&mut fullscreen, s.fullscreen).changed() {
            out.toggle_fullscreen = true;
        }
        if env.show_exit && ui.button(s.exit).clicked() {
            out.exit = true;
        }
    });

    ui.add_space(12.0);
    if state.confirm_reset {
        ui.label(s.reset_confirm);
        ui.horizontal(|ui| {
            if ui.button(s.reset_yes).clicked() {
                out.reset = true;
                state.confirm_reset = false;
            }
            if ui.button(s.cancel).clicked() {
                state.confirm_reset = false;
            }
        });
    } else if ui.button(s.reset).clicked() {
        state.confirm_reset = true;
    }
    ui.add_space(12.0);
}

fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).strong());
    ui.indent(title, body);
}

fn color_row(ui: &mut egui::Ui, label: &str, rgb: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.color_edit_button_srgb(rgb);
        ui.label(label);
    });
}

fn font_picker(
    ui: &mut egui::Ui,
    state: &mut UiState,
    env: &UiEnv<'_>,
    id: &str,
    label: &str,
    family: &mut String,
    weight: &mut u16,
) {
    let s = env.strings;
    ui.label(label);
    let shown = if family.is_empty() {
        s.system_default
    } else {
        family.as_str()
    };
    ComboBox::from_id_salt(("family", id))
        .selected_text(shown)
        .width(PANEL_WIDTH - 80.0)
        .height(360.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(family, String::new(), s.system_default);
            for f in env.fonts.families() {
                ui.selectable_value(family, f.clone(), f);
            }
        });
    let weights = if family.is_empty() {
        STANDARD_WEIGHTS.to_vec()
    } else {
        state
            .weights
            .entry(family.clone())
            .or_insert_with(|| env.fonts.weights(family))
            .clone()
    };
    if !weights.is_empty() && !weights.contains(weight) {
        *weight = nearest(&weights, *weight);
    }
    ComboBox::from_id_salt(("weight", id))
        .selected_text(format!("{} {}", s.weight, weight))
        .show_ui(ui, |ui| {
            for w in weights {
                ui.selectable_value(weight, w, w.to_string());
            }
        });
}

fn nearest(weights: &[u16], target: u16) -> u16 {
    *weights
        .iter()
        .min_by_key(|w| w.abs_diff(target))
        .unwrap_or(&target)
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}
