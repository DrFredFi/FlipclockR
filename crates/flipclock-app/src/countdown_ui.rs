//! Countdown screen controls drawn with egui around the tiles: ▲▼ arrows that
//! set each digit, the outline of the tile the keyboard edits, the description
//! box and the Start / Pause / Reset buttons. Each control lives in its own
//! small area, so clicks elsewhere still reach the window (drag, double-click).

use egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Id, Order, Pos2, RichText, Sense, Shape,
    Stroke, StrokeKind, Vec2, pos2, vec2,
};
use flipclock_core::countdown::{CONTROLS_HEIGHT, CountdownLayout, CountdownState};
use flipclock_core::digits::TILE_COUNT;

use crate::strings::Strings;

/// Longest description, in characters.
pub const DESCRIPTION_MAX_CHARS: usize = 80;
/// egui font family holding the date font, registered by the app.
pub const DATE_FAMILY: &str = "date";
/// Hold an arrow this long before it starts repeating, then repeat at this interval.
const REPEAT_DELAY: f64 = 0.4;
const REPEAT_INTERVAL: f64 = 0.11;

/// What the countdown screen shows this frame.
pub struct CountdownView<'a> {
    pub layout: &'a CountdownLayout,
    pub state: CountdownState,
    pub can_start: bool,
    /// The tile the arrow keys edit, outlined while setting.
    pub selected: Option<usize>,
    pub description: &'a mut String,
    /// The date color, used for arrows, outline and text.
    pub color: Color32,
    /// Whether the date font is registered with egui.
    pub date_font: bool,
}

#[derive(Default)]
pub struct CountdownActions {
    pub start: bool,
    pub pause_resume: bool,
    pub reset: bool,
    /// Step a tile's digit up (+1) or down (-1).
    pub step: Option<(usize, i32)>,
}

/// The arrow being held down, and when it next repeats.
#[derive(Default)]
pub struct ArrowRepeat {
    held: Option<(usize, i32, f64)>,
}

pub fn draw(
    ctx: &egui::Context,
    mut view: CountdownView<'_>,
    repeat: &mut ArrowRepeat,
    area_width: f32,
    strings: &Strings,
) -> CountdownActions {
    let mut out = CountdownActions::default();
    let l = view.layout;
    let idle = view.state == CountdownState::Idle;

    if idle && l.arrows_fit {
        arrows(ctx, l, view.color, repeat, &mut out);
    } else {
        repeat.held = None;
    }
    if idle && let Some(i) = view.selected {
        let tile = l.clock.tiles[i];
        let pad = 5.0;
        ctx.layer_painter(egui::LayerId::background()).rect_stroke(
            egui::Rect::from_min_size(
                pos2(tile.x - pad, tile.y - pad),
                vec2(tile.w + 2.0 * pad, tile.h + 2.0 * pad),
            ),
            CornerRadius::same(6),
            Stroke::new(2.0, view.color.gamma_multiply(160.0 / 255.0)),
            StrokeKind::Middle,
        );
    }

    let center_x = l.clock.clock.x + l.clock.clock.w / 2.0;
    if idle {
        description_box(ctx, &mut view, strings, center_x, area_width, &mut out);
    }
    controls(
        ctx,
        &view,
        strings,
        pos2(center_x, l.controls_center_y),
        &mut out,
    );
    out
}

fn arrows(
    ctx: &egui::Context,
    l: &CountdownLayout,
    color: Color32,
    repeat: &mut ArrowRepeat,
    out: &mut CountdownActions,
) {
    let now = ctx.input(|i| i.time);
    let size = l.arrow_size;
    let mut any_held = false;
    for i in 0..TILE_COUNT {
        let tile = l.clock.tiles[i];
        let cx = tile.x + tile.w / 2.0;
        for (delta, cy) in [(1, tile.y - size * 0.75), (-1, tile.bottom() + size * 0.75)] {
            let id = Id::unique(("countdown-arrow", i, delta));
            let response = egui::Area::new(id)
                .order(Order::Middle)
                .pivot(Align2::CENTER_CENTER)
                .fixed_pos(pos2(cx, cy))
                .show(ctx, |ui| arrow(ui, size, delta, color))
                .inner;
            response.surrender_focus();
            if !response.is_pointer_button_down_on() {
                continue;
            }
            any_held = true;
            match repeat.held {
                Some((ti, d, next)) if ti == i && d == delta => {
                    if now >= next {
                        out.step = Some((i, delta));
                        repeat.held = Some((i, delta, next + REPEAT_INTERVAL));
                    }
                }
                _ => {
                    out.step = Some((i, delta));
                    repeat.held = Some((i, delta, now + REPEAT_DELAY));
                }
            }
            ctx.request_repaint_after_secs(REPEAT_INTERVAL as f32 / 2.0);
        }
    }
    if !any_held {
        repeat.held = None;
    }
}

/// A transparent button holding a triangle pointing up (`delta` > 0) or down.
fn arrow(ui: &mut egui::Ui, size: f32, delta: i32, color: Color32) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(size * 1.8, size * 1.3), Sense::click_and_drag());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(6), Color32::from_white_alpha(24));
    }
    let (w, h) = (size / 2.0, size * 0.3);
    let c = rect.center();
    let points: Vec<Pos2> = if delta > 0 {
        vec![c + vec2(-w, h), c + vec2(0.0, -h), c + vec2(w, h)]
    } else {
        vec![c + vec2(-w, -h), c + vec2(w, -h), c + vec2(0.0, h)]
    };
    painter.add(Shape::convex_polygon(
        points,
        color.gamma_multiply(200.0 / 255.0),
        Stroke::NONE,
    ));
    response
}

fn description_box(
    ctx: &egui::Context,
    view: &mut CountdownView<'_>,
    strings: &Strings,
    center_x: f32,
    area_width: f32,
    out: &mut CountdownActions,
) {
    let l = view.layout;
    let family = if view.date_font {
        FontFamily::Name(DATE_FAMILY.into())
    } else {
        FontFamily::Proportional
    };
    let font = FontId::new(l.text_size, family);
    // Keep clear of the round buttons in the top-right corner.
    let width = (l.text_size * 12.0).min(area_width - 2.0 * 110.0).max(80.0);
    let description = &mut *view.description;
    egui::Area::new(Id::unique("countdown-description"))
        .order(Order::Middle)
        .pivot(Align2::CENTER_CENTER)
        .fixed_pos(pos2(center_x, l.description_center_y))
        .show(ctx, |ui| {
            let response = ui.add(
                egui::TextEdit::singleline(description)
                    .hint_text(RichText::new(strings.description_hint).font(font.clone()))
                    .font(font)
                    .text_color(view.color)
                    .background_color(Color32::from_white_alpha(32))
                    .align(Align2::CENTER_CENTER)
                    .char_limit(DESCRIPTION_MAX_CHARS)
                    .desired_width(width),
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                out.start = true;
            }
        });
}

fn controls(
    ctx: &egui::Context,
    view: &CountdownView<'_>,
    strings: &Strings,
    center: Pos2,
    out: &mut CountdownActions,
) {
    let button = |icon: &str, label: &str| {
        egui::Button::new(
            RichText::new(format!("{icon}  {label}"))
                .size(16.0)
                .color(Color32::WHITE),
        )
        .fill(Color32::from_white_alpha(64))
        .corner_radius(CONTROLS_HEIGHT / 2.0)
        .min_size(Vec2::new(0.0, CONTROLS_HEIGHT))
    };
    egui::Area::new(Id::unique("countdown-controls"))
        .order(Order::Middle)
        .pivot(Align2::CENTER_CENTER)
        .fixed_pos(center)
        .show(ctx, |ui| {
            ui.spacing_mut().button_padding = vec2(18.0, 0.0);
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.horizontal(|ui| match view.state {
                CountdownState::Idle => {
                    let start = ui.add_enabled(view.can_start, button("▶", strings.start));
                    if tapped(&start) {
                        out.start = true;
                    }
                }
                state => {
                    if tapped(&ui.add(button("⟲", strings.countdown_reset))) {
                        out.reset = true;
                    }
                    let pause = match state {
                        CountdownState::Running => Some(("⏸", strings.pause)),
                        CountdownState::Paused => Some(("▶", strings.resume)),
                        _ => None,
                    };
                    if let Some((icon, label)) = pause
                        && tapped(&ui.add(button(icon, label)))
                    {
                        out.pause_resume = true;
                    }
                }
            });
        });
}

/// Whether a clicked control was activated. The control gives up keyboard focus at
/// once, so the arrow keys keep editing the digits instead of moving egui's focus.
pub fn tapped(response: &egui::Response) -> bool {
    if response.has_focus() {
        response.surrender_focus();
    }
    response.clicked()
}
