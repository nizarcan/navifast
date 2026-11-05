//! Regenerate packaged icons from the same SVG the application uses.
//! Run `cargo run --locked --example app_icon` from the repository root.

use std::io::Cursor;
use std::path::Path;

fn raster(size: u32) -> image::RgbaImage {
    image::RgbaImage::from_raw(size, size, navifast::util::app_icon_rgba(size as usize))
        .expect("square app icon")
}

fn png(image: image::RgbaImage) -> image::ImageResult<Vec<u8>> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png)?;
    Ok(bytes.into_inner())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // Match the previous circular macOS icon's inset within its 1024px canvas.
    let mut mac = image::RgbaImage::new(1024, 1024);
    image::imageops::replace(&mut mac, &raster(852), 86, 86);
    std::fs::write(root.join("packaging/macos/icon-1024.png"), png(mac)?)?;

    // Modern Windows ICO files contain PNG images. Write that simple container
    // directly, using the existing PNG encoder instead of another dependency.
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let frames = sizes
        .map(|size| png(raster(size)))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let mut ico = vec![0, 0, 1, 0];
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + sizes.len() as u32 * 16;
    for (&size, frame) in sizes.iter().zip(&frames) {
        ico.extend_from_slice(&[size as u8, size as u8, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += frame.len() as u32;
    }
    for frame in frames {
        ico.extend_from_slice(&frame);
    }
    std::fs::write(root.join("packaging/windows/navifast.ico"), ico)?;
    Ok(())
}
