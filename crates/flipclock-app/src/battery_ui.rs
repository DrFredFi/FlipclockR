//! The battery indicator: an outline in the date color whose fill follows the
//! charge (green, yellow, red), a bolt while plugged in, the percentage below.
//! While charging the fill breathes so the charge reads as flowing in.

use std::f32::consts::TAU;

use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2,
};
use flipclock_core::battery::{self, BatteryLevel, BatteryStatus};
use flipclock_core::config::{BatteryPosition, Config};

const GREEN: Color32 = Color32::from_rgb(0x34, 0xC7, 0x59);
const YELLOW: Color32 = Color32::from_rgb(0xFF, 0xCC, 0x00);
const RED: Color32 = Color32::from_rgb(0xFF, 0x3B, 0x30);
/// One full breath of the charging pulse, in seconds; opacity goes 1 → 0.35 → 1.
const PULSE_PERIOD: f32 = 2.2;
/// Frame interval while the pulse runs.
const PULSE_FRAME: f32 = 1.0 / 30.0;

/// The bolt outline (x right, y down, 10 × 16 units), and the same shape as two
/// convex halves for filling.
const BOLT: [[f32; 2]; 7] = [
    [6.0, 0.0],
    [0.0, 9.0],
    [4.5, 9.0],
    [3.5, 16.0],
    [10.0, 6.5],
    [5.5, 6.5],
    [7.0, 0.0],
];
const BOLT_TOP: [[f32; 2]; 5] = [[7.0, 0.0], [6.0, 0.0], [0.0, 9.0], [4.5, 9.0], [5.5, 6.5]];
const BOLT_BOTTOM: [[f32; 2]; 4] = [[5.5, 6.5], [10.0, 6.5], [3.5, 16.0], [4.5, 9.0]];

/// Draw the indicator in a corner of `area` (the clock area, in points).
/// `top_right_clearance` keeps the top-right corner free for the buttons there.
pub fn draw(
    ctx: &egui::Context,
    status: Option<BatteryStatus>,
    config: &Config,
    area: Rect,
    top_right_clearance: f32,
    time: f32,
) {
    let Some(status) = status.filter(|_| config.battery.visible) else {
        return;
    };
    let l = battery::layout(area.width(), area.height(), config.battery.size);
    let [r, g, b] = config.colors.date.0;
    let color = Color32::from_rgb(r, g, b);
    let gap = l.font_size * 0.2;
    let cap_gap = l.stroke * 0.6;
    let icon_width = l.body_width + cap_gap + l.cap_width;
    let size = vec2(icon_width, l.body_height + gap + l.font_size * 1.2);

    let pad = l.edge_padding;
    let (x, y) = match config.battery.position {
        BatteryPosition::TopLeft => (area.left() + pad, area.top() + pad),
        BatteryPosition::TopRight => (
            area.right() - pad - size.x,
            area.top() + pad.max(top_right_clearance),
        ),
        BatteryPosition::BottomLeft => (area.left() + pad, area.bottom() - pad - size.y),
        BatteryPosition::BottomRight => (area.right() - pad - size.x, area.bottom() - pad - size.y),
    };
    let painter = ctx.layer_painter(egui::LayerId::background());

    // Outline and cap.
    let body = Rect::from_min_size(pos2(x, y), vec2(l.body_width, l.body_height));
    let half = l.stroke / 2.0;
    painter.rect_stroke(
        body.shrink(half),
        CornerRadius::same((l.stroke * 2.5).round() as u8),
        Stroke::new(l.stroke, color),
        StrokeKind::Middle,
    );
    let cap = Rect::from_min_size(
        pos2(body.right() + cap_gap, body.center().y - l.cap_height / 2.0),
        vec2(l.cap_width, l.cap_height),
    );
    let cap_radius = (l.cap_width / 2.0).round() as u8;
    painter.rect_filled(
        cap,
        CornerRadius {
            nw: 0,
            ne: cap_radius,
            sw: 0,
            se: cap_radius,
        },
        color,
    );

    // Fill, pulsing while charging.
    let inner = body.shrink(l.stroke * 2.0);
    let fill_width = l.inner_width() * battery::fill_fraction(status.percent);
    if fill_width > 0.0 {
        let alpha = if status.charging {
            ctx.request_repaint_after_secs(PULSE_FRAME);
            0.675 + 0.325 * (TAU * time / PULSE_PERIOD).cos()
        } else {
            1.0
        };
        let fill = match battery::level_for(status.percent) {
            BatteryLevel::Low => RED,
            BatteryLevel::Medium => YELLOW,
            BatteryLevel::High => GREEN,
        };
        painter.rect_filled(
            Rect::from_min_size(inner.min, vec2(fill_width, inner.height())),
            CornerRadius::same(l.stroke.round() as u8),
            fill.gamma_multiply(alpha),
        );
    }

    if status.plugged_in {
        let scale = inner.height() / 16.0;
        let origin = inner.center() - vec2(5.0, 8.0) * scale;
        let map = |pts: &[[f32; 2]]| -> Vec<Pos2> {
            pts.iter()
                .map(|[px, py]| origin + vec2(*px, *py) * scale)
                .collect()
        };
        for half in [&BOLT_TOP[..], &BOLT_BOTTOM[..]] {
            painter.add(Shape::convex_polygon(map(half), color, Stroke::NONE));
        }
        painter.add(Shape::closed_line(
            map(&BOLT),
            Stroke::new((l.stroke * 0.6).max(1.0), Color32::from_black_alpha(0xB0)),
        ));
    }

    // A plain face, not the date font: decorative date fonts make small digits unreadable.
    painter.text(
        pos2(x + l.body_width / 2.0, body.bottom() + gap),
        Align2::CENTER_TOP,
        format!("{}%", status.percent),
        FontId::proportional(l.font_size),
        color,
    );
}
