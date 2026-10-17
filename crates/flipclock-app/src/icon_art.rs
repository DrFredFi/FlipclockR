// App icon, drawn procedurally: a dark rounded square holding one split-flap tile.
// Shared by the binary (window and tray icons) and build.rs (Windows .ico).

/// RGBA pixels, row-major from the top, `size × size`.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
            let bg = rounded_rect_coverage(x, y, 0.0, 0.0, s, s, s * 0.22);
            let tile =
                rounded_rect_coverage(x, y, s * 0.24, s * 0.14, s * 0.76, s * 0.86, s * 0.08);
            let gap = (1.0 - ((y - s * 0.5).abs() - s * 0.025).clamp(0.0, 1.0)) * tile;
            let digit = digit_one_coverage(x, y, s) * tile;
            let mut rgb = [0x2a as f32, 0x2d as f32, 0x33 as f32];
            rgb = mix(rgb, [0x1c as f32, 0x1c as f32, 0x1e as f32], tile);
            rgb = mix(rgb, [0xf2 as f32, 0xf0 as f32, 0xea as f32], digit);
            rgb = mix(rgb, [0x0a as f32, 0x0a as f32, 0x0b as f32], gap);
            out.extend_from_slice(&[rgb[0] as u8, rgb[1] as u8, rgb[2] as u8, (bg * 255.0) as u8]);
        }
    }
    out
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Anti-aliased coverage of a rounded rectangle at pixel center (x, y).
fn rounded_rect_coverage(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// A stylized "1": a vertical bar with a short flag.
fn digit_one_coverage(x: f32, y: f32, s: f32) -> f32 {
    let bar = rounded_rect_coverage(x, y, s * 0.47, s * 0.27, s * 0.57, s * 0.73, s * 0.01);
    let flag_y = (y - s * 0.27) / (s * 0.12);
    let flag = if (0.0..=1.0).contains(&flag_y) {
        let left = s * 0.47 - (1.0 - flag_y) * s * 0.1;
        ((x - left).clamp(0.0, 1.0))
            * ((s * 0.5 - x).clamp(0.0, 1.0))
            * ((1.0 - flag_y) * 2.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    bar.max(flag)
}
