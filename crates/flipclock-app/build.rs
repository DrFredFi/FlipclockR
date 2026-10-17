//! Windows only: embed the app icon and an application manifest (per-monitor DPI v2).

use std::path::Path;

mod icon_art {
    include!("src/icon_art.rs");
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon_art.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let out = Path::new(&out);
    let ico = out.join("flipclock.ico");
    let manifest = out.join("flipclock.manifest");
    let rc = out.join("flipclock.rc");
    std::fs::write(&ico, ico_file(&[16, 24, 32, 48, 64, 256])).expect("write icon");
    std::fs::write(&manifest, MANIFEST).expect("write manifest");
    let escape = |p: &Path| p.display().to_string().replace('\\', "\\\\");
    std::fs::write(
        &rc,
        format!(
            "1 ICON \"{}\"\n1 24 \"{}\"\n",
            escape(&ico),
            escape(&manifest)
        ),
    )
    .expect("write rc");
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_required()
        .expect("compile Windows resources");
}

/// A multi-size .ico with 32-bit BMP images.
fn ico_file(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&n| bmp_image(n)).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (n, img) in sizes.iter().zip(&images) {
        let dim = if *n >= 256 { 0 } else { *n as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        out.extend_from_slice(&img);
    }
    out
}

fn bmp_image(n: u32) -> Vec<u8> {
    let rgba = icon_art::icon_rgba(n);
    let mask_row = n.div_ceil(32) * 4;
    let mut out = Vec::new();
    for v in [40u32, n, n * 2] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    for v in [0u32, n * n * 4 + mask_row * n, 0, 0, 0, 0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for y in (0..n).rev() {
        for x in 0..n {
            let i = ((y * n + x) * 4) as usize;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    out.extend(std::iter::repeat_n(0u8, (mask_row * n) as usize));
    out
}
