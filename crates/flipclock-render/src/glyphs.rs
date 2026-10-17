//! Glyph rasterization with swash into single-channel coverage masks.

use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source, image::Image};
use swash::zeno::Format;

use crate::fonts::LoadedFont;

/// A single-channel coverage mask.
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Mask {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width * height) as usize],
        }
    }

    /// Composite `image` with its top-left corner at (`x`, `y`), clipping at the edges.
    fn blit(&mut self, image: &Image, x: i32, y: i32) {
        let (w, h) = (image.placement.width as i32, image.placement.height as i32);
        for row in 0..h {
            let ty = y + row;
            if ty < 0 || ty >= self.height as i32 {
                continue;
            }
            for col in 0..w {
                let tx = x + col;
                if tx < 0 || tx >= self.width as i32 {
                    continue;
                }
                let src = image.data[(row * w + col) as usize];
                let dst = &mut self.data[(ty as u32 * self.width + tx as u32) as usize];
                *dst = (*dst).max(src);
            }
        }
    }
}

/// Fraction of the cell height taken by the digit's height.
const DIGIT_HEIGHT_FRACTION: f32 = 0.62;
/// Maximum fraction of the cell width any digit may take.
const DIGIT_MAX_WIDTH_FRACTION: f32 = 0.84;

struct Rasterizer {
    ctx: ScaleContext,
}

impl Rasterizer {
    fn new() -> Self {
        Self {
            ctx: ScaleContext::new(),
        }
    }

    fn render(&mut self, font: FontRef<'_>, weight: u16, ppem: f32, ch: char) -> Option<Image> {
        let gid = font.charmap().map(ch);
        if gid == 0 {
            return None;
        }
        let mut scaler = self
            .ctx
            .builder(font)
            .size(ppem)
            .hint(false)
            .variations([("wght", f32::from(weight))])
            .build();
        Render::new(&[Source::Outline])
            .format(Format::Alpha)
            .render(&mut scaler, gid)
    }
}

/// Rasterize digits 0–9 into an atlas of ten `cell_w × cell_h` cells side by side,
/// each digit centered in its cell.
pub fn digit_atlas(font: &LoadedFont, cell_w: u32, cell_h: u32) -> Mask {
    let mut atlas = Mask::new(cell_w * 10, cell_h);
    let Some(font_ref) = font.font_ref() else {
        log::warn!("cannot parse digit font {font:?}");
        return atlas;
    };
    let mut r = Rasterizer::new();
    let probe = cell_h as f32;
    let Some(zero) = r.render(font_ref, font.weight, probe, '0') else {
        log::warn!("digit font {font:?} has no digits");
        return atlas;
    };
    let zero_h = zero.placement.height.max(1) as f32;
    let mut ppem = probe * DIGIT_HEIGHT_FRACTION * cell_h as f32 / zero_h;

    let widest = |r: &mut Rasterizer, ppem: f32| {
        ('0'..='9')
            .filter_map(|c| r.render(font_ref, font.weight, ppem, c))
            .map(|img| img.placement.width)
            .max()
            .unwrap_or(0) as f32
    };
    let max_w = widest(&mut r, ppem);
    let limit = cell_w as f32 * DIGIT_MAX_WIDTH_FRACTION;
    if max_w > limit {
        ppem *= limit / max_w;
    }

    let Some(zero) = r.render(font_ref, font.weight, ppem, '0') else {
        return atlas;
    };
    let digit_top = (cell_h as i32 - zero.placement.height as i32) / 2;
    let baseline = digit_top + zero.placement.top;
    for (i, c) in ('0'..='9').enumerate() {
        if let Some(img) = r.render(font_ref, font.weight, ppem, c) {
            let x = i as i32 * cell_w as i32 + (cell_w as i32 - img.placement.width as i32) / 2;
            let y = baseline - img.placement.top;
            atlas.blit(&img, x, y);
        }
    }
    atlas
}

/// Rasterize a single line of text whose line height is `height` pixels.
pub fn text_line(font: &LoadedFont, text: &str, height: f32) -> Mask {
    let Some(font_ref) = font.font_ref() else {
        return Mask::new(1, 1);
    };
    let ppem = height * 0.8;
    let metrics = font_ref.metrics(&[]).scale(ppem);
    let glyph_metrics = font_ref.glyph_metrics(&[]).scale(ppem);
    let charmap = font_ref.charmap();
    let pad = (ppem * 0.1).ceil();
    let advance: f32 = text
        .chars()
        .map(|c| glyph_metrics.advance_width(charmap.map(c)))
        .sum();
    let line_h = (metrics.ascent + metrics.descent).ceil().max(1.0);
    let mut mask = Mask::new(
        (advance + 2.0 * pad).ceil().max(1.0) as u32,
        (line_h + 2.0 * pad) as u32,
    );
    let baseline = pad + metrics.ascent;
    let mut r = Rasterizer::new();
    let mut pen = pad;
    for c in text.chars() {
        if let Some(img) = r.render(font_ref, font.weight, ppem, c) {
            mask.blit(
                &img,
                (pen + img.placement.left as f32).round() as i32,
                (baseline - img.placement.top as f32).round() as i32,
            );
        }
        pen += glyph_metrics.advance_width(charmap.map(c));
    }
    mask
}
