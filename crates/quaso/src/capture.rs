use base64::{Engine, prelude::BASE64_STANDARD};
use image::{DynamicImage, ImageFormat, RgbaImage, imageops::FilterType};
use serde_json::{Value, json};
use spitfire_glow::graphics::CapturedFrame;
use std::io::Cursor;

pub const GAME_CAPTURE_TARGET: &str = "game";

pub fn encode_png(
    target: &str,
    frame: CapturedFrame,
    max_size: Option<u32>,
) -> Result<Value, String> {
    let image = RgbaImage::from_raw(frame.width, frame.height, frame.pixels)
        .ok_or_else(|| "Captured frame does not hold enough pixels".to_owned())?;
    let mut image = DynamicImage::ImageRgba8(image);
    if let Some(max_size) = max_size
        && max_size > 0
        && (frame.width > max_size || frame.height > max_size)
    {
        let scale = max_size as f32 / frame.width.max(frame.height) as f32;
        let width = ((frame.width as f32 * scale).round() as u32).max(1);
        let height = ((frame.height as f32 * scale).round() as u32).max(1);
        image = image.resize(width, height, FilterType::Triangle);
    }
    let mut encoded = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut encoded), ImageFormat::Png)
        .map_err(|error| format!("Could not encode the captured frame: {error}"))?;
    Ok(json!({
        "target": target,
        "width": image.width(),
        "height": image.height(),
        "source_width": frame.width,
        "source_height": frame.height,
        "mime_type": "image/png",
        "data": BASE64_STANDARD.encode(&encoded),
    }))
}
