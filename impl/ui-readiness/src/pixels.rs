//! Bounded image equality for readiness only. Never changes delivered captures
//! or grants permission to send an image to a model.
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Cursor;

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_777_216;

pub(super) fn data_url(value: &Value) -> Option<Value> {
    let url = value.as_str()?;
    let (format, encoded) = if let Some(encoded) = url.strip_prefix("data:image/png;base64,") {
        (image::ImageFormat::Png, encoded)
    } else {
        (image::ImageFormat::Jpeg, url.strip_prefix("data:image/jpeg;base64,")?)
    };
    fingerprint(encoded, format)
}

pub(super) fn png(value: &Value) -> Option<Value> {
    fingerprint(value.as_str()?, image::ImageFormat::Png)
}

fn fingerprint(encoded: &str, format: image::ImageFormat) -> Option<Value> {
    if encoded.len() > MAX_BYTES.div_ceil(3) * 4 { return None; }
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES { return None; }
    let reader = image::ImageReader::with_format(Cursor::new(&bytes), format);
    let (width, height) = reader.into_dimensions().ok()?;
    if width == 0 || height == 0 || width > 8192 || height > 8192
        || u64::from(width) * u64::from(height) > MAX_PIXELS { return None; }
    let mut reader = image::ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(MAX_PIXELS * 8);
    reader.limits(limits);
    let pixels = reader.decode().ok()?.into_rgba8();
    Some(json!({"width":width,"height":height,
        "rgba_sha256":format!("{:x}", Sha256::digest(pixels.as_raw()))}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{semantic_snapshot, Policy, Readiness, Status};
    use std::time::Duration;
    const A: &[u8] = include_bytes!("../tests/fixtures/gnome-still-a.png");
    const B: &[u8] = include_bytes!("../tests/fixtures/gnome-still-b.png");
    fn encoded(bytes: &[u8]) -> String { base64::engine::general_purpose::STANDARD.encode(bytes) }
    fn state(bytes: &[u8]) -> Value {
        json!({"elements":[{"role":"text","value":"unchanged"}],
            "screenshot_data_url":format!("data:image/png;base64,{}",encoded(bytes))})
    }
    #[test]
    fn native_png_creation_time_is_not_visual_change() {
        assert_ne!(A, B);
        assert_eq!(semantic_snapshot(&state(A)), semantic_snapshot(&state(B)));
        let mut readiness = Readiness::new(Policy::default());
        assert_eq!(readiness.advance(Duration::from_millis(400), &state(A), false), Status::Waiting);
        assert_eq!(readiness.advance(Duration::from_millis(1100), &state(B), false), Status::Ready);
        // The decision still receives its original capture, not this digest.
        assert_eq!(state(B)["screenshot_data_url"], format!("data:image/png;base64,{}",encoded(B)));
    }
    #[test]
    fn a_single_changed_pixel_restarts_quiet_wait() {
        let mut image = image::load_from_memory(A).unwrap().into_rgba8();
        image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let changed = state(bytes.get_ref());
        assert_ne!(semantic_snapshot(&state(A)), semantic_snapshot(&changed));
        let mut readiness = Readiness::new(Policy::default());
        readiness.advance(Duration::from_millis(400), &state(A), false);
        assert_eq!(readiness.advance(Duration::from_millis(1100), &changed, false), Status::Waiting);
        assert_eq!(readiness.advance(Duration::from_millis(1500), &changed, false), Status::Ready);
    }
    #[test]
    fn equivalent_jpeg_comments_and_png_forms_compare_by_pixels() {
        let image = image::DynamicImage::new_rgb8(3, 2);
        let mut bytes = Cursor::new(Vec::new());image.write_to(&mut bytes, image::ImageFormat::Jpeg).unwrap();
        let original = bytes.into_inner();let mut comment = original[..2].to_vec();
        comment.extend_from_slice(b"\xff\xfe\x00\x05one");comment.extend_from_slice(&original[2..]);
        let jpg = |b: &[u8]| json!(format!("data:image/jpeg;base64,{}",encoded(b)));
        assert_eq!(data_url(&jpg(&original)), data_url(&jpg(&comment)));
        assert!(data_url(&jpg(&original)).is_some());
        assert_eq!(png(&json!(encoded(A))), png(&json!(encoded(B))));
        assert_eq!(semantic_snapshot(&json!({"screenshots":[{"png_b64":encoded(A)}]})),
                   semantic_snapshot(&json!({"screenshots":[{"png_b64":encoded(B)}]})));
    }
    #[test]
    fn invalid_and_oversized_images_are_not_erased() {
        for value in ["bad".to_string(), "A".repeat(MAX_BYTES.div_ceil(3)*4+1)] {
            let state = json!({"screenshot_png_b64":value});
            assert_eq!(semantic_snapshot(&state), state);
        }
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(8193,1).write_to(&mut encoded,image::ImageFormat::Png).unwrap();
        assert!(png(&json!(self::encoded(encoded.get_ref()))).is_none());
        assert!(data_url(&json!("https://example.invalid/frame.png")).is_none());
    }
}
