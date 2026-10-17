//! The agent cursor inside returned screenshots (spec 10 step 3, Sky `mouse_size_px`).
//!
//! The driver draws its agent cursor in a separate overlay window, so a raw window capture never
//! contains it, and a model reading only the screenshot cannot see where its last click landed.
//! Sky's full-desktop client has `mouse_size_px` (default 12; "Resize mouse pointer in screenshots
//! to this size in pixels. Set 0 to disable."). This module draws the pointer into the PNG the
//! layer hands back, at the pixel the last action targeted, at that size. The layer remembers the
//! last pixel per window ([`crate::Layer`]); a capture taken before any pointer action has none
//! and is returned untouched.
//!
//! The sprite is our own arrow (black outline, white fill, tip at the target pixel), not Sky's:
//! only the size contract is Sky's. Output keeps the input's dimensions, so every geometry check
//! downstream (logical size, `pixel_scale`) is unaffected.
use base64::Engine;
use image::{ImageFormat, Rgba, RgbaImage};

/// Arrow outline in unit coordinates, tip at the origin, pointing up-left like a system cursor.
const ARROW: [(f64, f64); 7] = [(0.0, 0.0), (0.0, 0.84), (0.2, 0.66), (0.34, 1.0), (0.5, 0.93), (0.36, 0.6), (0.62, 0.6)];

fn inside(poly: &[(f64, f64)], p: (f64, f64)) -> bool {
    let mut hit = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 { hit = !hit; }
        j = i;
    }
    hit
}

fn edge_distance(poly: &[(f64, f64)], p: (f64, f64)) -> f64 {
    let mut best = f64::MAX;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[j], poly[i]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dy * dy;
        let t = if len2 == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) };
        best = best.min(((p.0 - (a.0 + t * dx)).powi(2) + (p.1 - (a.1 + t * dy)).powi(2)).sqrt());
        j = i;
    }
    best
}

/// Draw the pointer into `img` with its tip at `(x, y)` pixels, `size_px` tall. Parts outside the
/// image are clipped. Returns the bounding box that was painted `(x0, y0, x1, y1)`, half-open, or
/// None when nothing landed on the image.
pub fn draw(img: &mut RgbaImage, x: f64, y: f64, size_px: u32) -> Option<(u32, u32, u32, u32)> {
    if size_px == 0 || !x.is_finite() || !y.is_finite() { return None; }
    let s = size_px as f64;
    let poly: Vec<(f64, f64)> = ARROW.iter().map(|(px, py)| (x + px * s, y + py * s)).collect();
    let outline = (s / 10.0).max(1.0);
    let (w, h) = (img.width() as i64, img.height() as i64);
    let (bx0, by0) = ((x.floor() as i64 - 1).max(0), (y.floor() as i64 - 1).max(0));
    let (bx1, by1) = (((x + 0.65 * s).ceil() as i64 + 2).min(w), ((y + s).ceil() as i64 + 2).min(h));
    if bx0 >= bx1 || by0 >= by1 { return None; }
    const SS: i64 = 3;
    let mut painted = false;
    for py in by0..by1 {
        for px in bx0..bx1 {
            let (mut black, mut white) = (0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let p = (px as f64 + (sx as f64 + 0.5) / SS as f64, py as f64 + (sy as f64 + 0.5) / SS as f64);
                    if !inside(&poly, p) { continue; }
                    if edge_distance(&poly, p) < outline { black += 1 } else { white += 1 }
                }
            }
            let covered = black + white;
            if covered == 0 { continue; }
            let alpha = covered as f64 / (SS * SS) as f64;
            let tone = if covered == 0 { 0.0 } else { 255.0 * white as f64 / covered as f64 };
            let dst = img.get_pixel_mut(px as u32, py as u32);
            for c in 0..3 { dst[c] = (dst[c] as f64 * (1.0 - alpha) + tone * alpha).round() as u8; }
            dst[3] = 255;
            painted = true;
        }
    }
    painted.then_some((bx0 as u32, by0 as u32, bx1 as u32, by1 as u32))
}

/// Composite the pointer into a base64 PNG and return the new base64 PNG, same dimensions.
/// `None` when the pointer falls entirely outside the image or `size_px` is 0.
pub fn composite_png_b64(png_b64: &str, x: f64, y: f64, size_px: u32) -> Result<Option<String>, String> {
    if size_px == 0 { return Ok(None); }
    let bytes = base64::engine::general_purpose::STANDARD.decode(png_b64).map_err(|e| format!("screenshot base64: {e}"))?;
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384); limits.max_image_height = Some(16384); limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut img = reader.decode().map_err(|e| format!("screenshot PNG: {e}"))?.to_rgba8();
    if draw(&mut img, x, y, size_px).is_none() { return Ok(None); }
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png).map_err(|e| format!("encode PNG: {e}"))?;
    Ok(Some(base64::engine::general_purpose::STANDARD.encode(out)))
}

/// A solid background image, for tests and fixtures.
pub fn solid_png_b64(w: u32, h: u32, rgb: [u8; 3]) -> String {
    let img = RgbaImage::from_pixel(w, h, Rgba([rgb[0], rgb[1], rgb[2], 255]));
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png).unwrap();
    base64::engine::general_purpose::STANDARD.encode(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(b64: &str) -> RgbaImage {
        image::load_from_memory_with_format(&base64::engine::general_purpose::STANDARD.decode(b64).unwrap(), ImageFormat::Png).unwrap().to_rgba8()
    }

    #[test]
    fn the_pointer_lands_on_the_target_pixel_at_the_requested_size() {
        let src = solid_png_b64(100, 80, [30, 90, 160]);
        let out = decode(&composite_png_b64(&src, 40.0, 20.0, 12).unwrap().expect("drawn"));
        assert_eq!((out.width(), out.height()), (100, 80), "same dimensions as the capture");
        let base = decode(&src);
        let changed: Vec<(u32, u32)> = (0..80).flat_map(|y| (0..100).map(move |x| (x, y))).filter(|&(x, y)| out.get_pixel(x, y) != base.get_pixel(x, y)).collect();
        assert!(!changed.is_empty());
        let (min_x, max_x) = (changed.iter().map(|p| p.0).min().unwrap(), changed.iter().map(|p| p.0).max().unwrap());
        let (min_y, max_y) = (changed.iter().map(|p| p.1).min().unwrap(), changed.iter().map(|p| p.1).max().unwrap());
        assert!(min_x >= 39 && min_y >= 19, "nothing drawn up-left of the tip: {min_x},{min_y}");
        assert!(max_y - min_y <= 13 && max_x - min_x <= 9, "bounded by the size: {}x{}", max_x - min_x + 1, max_y - min_y + 1);
        assert!(max_y - min_y >= 9, "about size_px tall: {}", max_y - min_y + 1);
        // nothing outside the sprite's box changed
        for &(x, y) in &changed { assert!((39..=50).contains(&x) && (19..=33).contains(&y), "stray pixel {x},{y}"); }
    }

    #[test]
    fn size_scales_the_sprite_and_zero_disables_it() {
        let src = solid_png_b64(200, 200, [255, 255, 255]);
        let height = |size| {
            let out = decode(&composite_png_b64(&src, 50.0, 50.0, size).unwrap().unwrap());
            let base = decode(&src);
            let ys: Vec<u32> = (0..200).flat_map(|y| (0..200).map(move |x| (x, y))).filter(|&(x, y)| out.get_pixel(x, y) != base.get_pixel(x, y)).map(|p| p.1).collect();
            ys.iter().max().unwrap() - ys.iter().min().unwrap() + 1
        };
        assert!(height(40) > 2 * height(12), "40px is much taller than 12px: {} vs {}", height(40), height(12));
        assert_eq!(composite_png_b64(&src, 50.0, 50.0, 0).unwrap(), None, "mouse_size_px 0 draws nothing");
    }

    #[test]
    fn a_pointer_off_the_image_or_at_an_edge_is_clipped_not_a_panic() {
        let src = solid_png_b64(30, 30, [0, 0, 0]);
        assert_eq!(composite_png_b64(&src, 500.0, 500.0, 12).unwrap(), None);
        assert_eq!(composite_png_b64(&src, -50.0, 5.0, 12).unwrap(), None);
        assert_eq!(composite_png_b64(&src, f64::NAN, 5.0, 12).unwrap(), None);
        assert!(composite_png_b64(&src, 29.0, 29.0, 12).unwrap().is_some(), "the tip pixel is inside, the rest is clipped");
        assert!(composite_png_b64(&src, 0.0, 0.0, 12).unwrap().is_some());
    }

    #[test]
    fn garbage_is_an_error_not_a_blank_screenshot() {
        assert!(composite_png_b64("not base64 !!", 1.0, 1.0, 12).is_err());
        assert!(composite_png_b64("AAAA", 1.0, 1.0, 12).is_err());
    }
}
