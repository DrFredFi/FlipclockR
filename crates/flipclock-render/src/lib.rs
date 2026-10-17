//! 3D rendering of the clock face with wgpu.

pub mod fonts;
pub mod glyphs;
pub mod mesh;
mod renderer;

pub use renderer::{FrameInput, FrameStatus, Renderer, Style, fill_uv};
pub use wgpu;

/// Load a background image, downscaled so neither side exceeds `max_side`.
pub fn load_background(path: &std::path::Path, max_side: u32) -> Option<image::RgbaImage> {
    let img = image::open(path)
        .inspect_err(|e| log::warn!("cannot load background {}: {e}", path.display()))
        .ok()?;
    let img = if img.width() > max_side || img.height() > max_side {
        img.resize(max_side, max_side, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    Some(img.to_rgba8())
}
