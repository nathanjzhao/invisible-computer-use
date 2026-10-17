//! Live-frame HTTP serving for the `frame://` protocol.
//!
//! Three wins over the old path (fresh OS capture per GET, served as PNG, no
//! cache):
//!   * transcode the daemon's PNG to JPEG for the live feed, which is a
//!     fraction of the byte cost at a quality that still reads UI text. PNG is
//!     kept only where losslessness matters (history), which is a separate path.
//!   * dedupe repeat GETs for the same surface through a short-lived cache keyed
//!     on (pid, window, max_dim), so a static window is captured about once a
//!     second instead of on every poll of the ~3 Hz tile loop.
//!   * conditional GET via ETag, so a surface whose pixels have not changed
//!     transfers a 304 with no body.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// How long a captured live frame is reused for repeat GETs of the same
/// surface before a fresh capture is taken. The in-app loop polls each tile
/// roughly three times a second; at this TTL a static window drops from one
/// `get_window_state` per poll to about one per second.
pub const LIVE_FRAME_TTL: Duration = Duration::from_millis(1000);

/// JPEG quality for the live feed: high enough to read dense UI text, low
/// enough to stay well under the lossless PNG byte cost.
const LIVE_JPEG_QUALITY: u8 = 72;

/// What the protocol handler should put on the wire for one GET.
pub enum Served {
    /// A fresh body to send (200): JPEG where transcoding succeeded, else the
    /// original bytes and mime.
    Modified(Frame),
    /// The caller's `If-None-Match` already names the current frame (304).
    NotModified { etag: String },
    /// The driver's typed reason this surface has no frame right now (422).
    Refused(String),
    /// No frame and no typed reason (204).
    Empty,
}

pub struct Frame {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub etag: String,
    pub screenshot_id: Option<String>,
}

struct Entry {
    etag: String,
    jpeg: Arc<Vec<u8>>,
    mime: &'static str,
    screenshot_id: Option<String>,
    at: Instant,
}

/// Cloned view of a cache entry, so the lock is released before we build the
/// (potentially large) HTTP body.
struct Resolved {
    etag: String,
    jpeg: Arc<Vec<u8>>,
    mime: &'static str,
    screenshot_id: Option<String>,
}

static CACHE: LazyLock<Mutex<HashMap<String, Entry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A capture returns its raw bytes, its mime, and the driver's `screenshot_id`
/// for that capture when it issues one (the id binds a later pixel read).
pub type Capture<'a> = dyn FnOnce() -> Result<(Vec<u8>, String, Option<String>), String> + 'a;

/// Serve one surface. `key` is the canonical per-surface key (`win/{pid}/{wid}/{max}`,
/// `desktop/{max}`, `display/{n}/{max}`), stable across the cache-buster query.
pub fn serve(key: &str, if_none_match: Option<&str>, capture: Box<Capture<'_>>) -> Served {
    serve_at(key, if_none_match, Instant::now(), LIVE_FRAME_TTL, capture)
}

fn serve_at(
    key: &str,
    if_none_match: Option<&str>,
    now: Instant,
    ttl: Duration,
    capture: Box<Capture<'_>>,
) -> Served {
    // Reuse a still-fresh cached frame without re-capturing: this is what drops
    // the daemon call count for a static window.
    if let Some(hit) = fresh_entry(key, now, ttl) {
        return finish(hit, if_none_match);
    }
    let (raw, raw_mime, screenshot_id) = match capture() {
        Ok(value) => value,
        Err(reason) if reason.starts_with(crate::cua::CAPTURE_REFUSED) => {
            return Served::Refused(reason[crate::cua::CAPTURE_REFUSED.len()..].to_string());
        }
        Err(_) => return Served::Empty,
    };
    if raw.is_empty() {
        return Served::Empty;
    }
    let (bytes, mime) = to_live_image(raw, &raw_mime);
    let etag = match &screenshot_id {
        // The driver's id already changes exactly when the pixels do.
        Some(id) => format!("\"{id}\""),
        None => format!("\"{}\"", short_hash(&bytes)),
    };
    let resolved = store(key, etag, bytes, mime, screenshot_id, now);
    finish(resolved, if_none_match)
}

fn fresh_entry(key: &str, now: Instant, ttl: Duration) -> Option<Resolved> {
    let cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let entry = cache.get(key)?;
    if now.saturating_duration_since(entry.at) >= ttl {
        return None;
    }
    Some(Resolved {
        etag: entry.etag.clone(),
        jpeg: entry.jpeg.clone(),
        mime: entry.mime,
        screenshot_id: entry.screenshot_id.clone(),
    })
}

fn store(
    key: &str,
    etag: String,
    bytes: Vec<u8>,
    mime: &'static str,
    screenshot_id: Option<String>,
    now: Instant,
) -> Resolved {
    let jpeg = Arc::new(bytes);
    let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    cache.insert(
        key.to_string(),
        Entry {
            etag: etag.clone(),
            jpeg: jpeg.clone(),
            mime,
            screenshot_id: screenshot_id.clone(),
            at: now,
        },
    );
    Resolved { etag, jpeg, mime, screenshot_id }
}

fn finish(resolved: Resolved, if_none_match: Option<&str>) -> Served {
    if if_none_match == Some(resolved.etag.as_str()) {
        return Served::NotModified { etag: resolved.etag };
    }
    Served::Modified(Frame {
        bytes: (*resolved.jpeg).clone(),
        mime: resolved.mime,
        etag: resolved.etag,
        screenshot_id: resolved.screenshot_id,
    })
}

/// Transcode a live frame to JPEG. PNG that fails to decode (or is already
/// JPEG) is passed through unchanged with its original mime.
fn to_live_image(raw: Vec<u8>, raw_mime: &str) -> (Vec<u8>, &'static str) {
    if raw_mime.contains("jpeg") || raw_mime.contains("jpg") {
        return (raw, "image/jpeg");
    }
    match to_jpeg(&raw) {
        Some(jpeg) => (jpeg, "image/jpeg"),
        None => (raw, "image/png"),
    }
}

fn to_jpeg(encoded: &[u8]) -> Option<Vec<u8>> {
    let rgb = image::load_from_memory(encoded).ok()?.to_rgb8();
    let (width, height) = rgb.dimensions();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, LIVE_JPEG_QUALITY)
        .encode(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)
        .ok()?;
    Some(out)
}

fn short_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// Test-only: forget cached frames so each case starts cold.
#[cfg(test)]
pub(crate) fn clear_cache() {
    CACHE.lock().unwrap_or_else(|p| p.into_inner()).clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A photographic-style value-noise image: continuous tone with 2-D
    /// structure, which PNG must store losslessly but JPEG compresses hard.
    /// This is the regime the live feed actually serves (screenshots), so it is
    /// the honest place to measure the PNG-vs-JPEG byte win.
    fn noisy_png(w: u32, h: u32) -> Vec<u8> {
        // Coarse random lattice, bilinearly upsampled per channel.
        const GRID: u32 = 24;
        let mut seed: u64 = 0x9E3779B97F4A7C15;
        let mut rng = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 32) as u32
        };
        let cells = ((GRID + 1) * (GRID + 1) * 3) as usize;
        let lattice: Vec<u8> = (0..cells).map(|_| (rng() & 0xff) as u8).collect();
        let sample = |cx: u32, cy: u32, ch: u32| -> f32 {
            lattice[((cy * (GRID + 1) + cx) * 3 + ch) as usize] as f32
        };
        let mut img = image::RgbImage::new(w, h);
        for y in 0..h {
            let gy = (y as f32) * (GRID as f32) / (h as f32);
            let y0 = gy.floor() as u32;
            let fy = gy - y0 as f32;
            for x in 0..w {
                let gx = (x as f32) * (GRID as f32) / (w as f32);
                let x0 = gx.floor() as u32;
                let fx = gx - x0 as f32;
                let mut px = [0u8; 3];
                for ch in 0..3u32 {
                    let a = sample(x0, y0, ch);
                    let b = sample(x0 + 1, y0, ch);
                    let c = sample(x0, y0 + 1, ch);
                    let d = sample(x0 + 1, y0 + 1, ch);
                    let top = a + (b - a) * fx;
                    let bot = c + (d - c) * fx;
                    px[ch as usize] = (top + (bot - top) * fy).round().clamp(0.0, 255.0) as u8;
                }
                img.put_pixel(x, y, image::Rgb(px));
            }
        }
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn live_jpeg_is_well_under_forty_percent_of_the_png() {
        let png = noisy_png(640, 480);
        let jpeg = to_jpeg(&png).expect("png transcodes to jpeg");
        // JPEG magic (SOI).
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        let ratio = jpeg.len() as f64 / png.len() as f64;
        eprintln!("live frame: png {} bytes -> jpeg {} bytes ({:.0}%)", png.len(), jpeg.len(), ratio * 100.0);
        assert!(
            ratio < 0.40,
            "jpeg {} bytes is {:.0}% of png {} bytes, want < 40%",
            jpeg.len(),
            ratio * 100.0,
            png.len()
        );
    }

    #[test]
    fn repeat_gets_within_the_ttl_reuse_bytes_without_recapturing() {
        super::clear_cache();
        let png = noisy_png(320, 240);
        let captures = AtomicUsize::new(0);
        let ttl = Duration::from_millis(1000);
        let start = Instant::now();
        let take = |now: Instant, inm: Option<&str>| {
            let png = png.clone();
            let captures = &captures;
            serve_at(
                "win/1/2/640",
                inm,
                now,
                ttl,
                Box::new(move || {
                    captures.fetch_add(1, Ordering::SeqCst);
                    Ok((png, "image/png".into(), Some("sid-A".into())))
                }),
            )
        };

        // First GET captures and transcodes.
        let first = take(start, None);
        let etag = match first {
            Served::Modified(f) => {
                assert_eq!(f.mime, "image/jpeg");
                assert_eq!(f.screenshot_id.as_deref(), Some("sid-A"));
                f.etag
            }
            _ => panic!("first GET must return a frame"),
        };
        assert_eq!(captures.load(Ordering::SeqCst), 1);

        // Repeat GETs inside the TTL reuse cached bytes: no new capture.
        for ms in [100u64, 300, 600, 900] {
            match take(start + Duration::from_millis(ms), None) {
                Served::Modified(f) => assert_eq!(f.etag, etag),
                _ => panic!("cached GET must still return the frame"),
            }
        }
        assert_eq!(captures.load(Ordering::SeqCst), 1, "no recapture within TTL");

        // A conditional GET inside the TTL gets a 304 and transfers no body.
        match take(start + Duration::from_millis(500), Some(etag.as_str())) {
            Served::NotModified { etag: e } => assert_eq!(e, etag),
            _ => panic!("matching If-None-Match must be 304"),
        }
        assert_eq!(captures.load(Ordering::SeqCst), 1, "304 does not recapture");

        // Past the TTL, a fresh capture is taken.
        match take(start + Duration::from_millis(1001), None) {
            Served::Modified(_) => {}
            _ => panic!("expired GET must return a frame"),
        }
        assert_eq!(captures.load(Ordering::SeqCst), 2, "recapture once TTL elapsed");
    }

    #[test]
    fn a_typed_refusal_is_surfaced_and_not_cached() {
        super::clear_cache();
        let refusal = format!("{}window is minimized (window_minimized)", crate::cua::CAPTURE_REFUSED);
        let out = serve_at(
            "win/9/9/640",
            None,
            Instant::now(),
            Duration::from_millis(1000),
            Box::new(move || Err(refusal)),
        );
        match out {
            Served::Refused(reason) => assert_eq!(reason, "window is minimized (window_minimized)"),
            _ => panic!("typed refusal must surface as Refused"),
        }
    }

    #[test]
    fn frames_without_an_id_get_a_content_hashed_etag() {
        super::clear_cache();
        let png = noisy_png(64, 64);
        let out = serve_at(
            "desktop/1100",
            None,
            Instant::now(),
            Duration::from_millis(1000),
            Box::new(move || Ok((png, "image/png".into(), None))),
        );
        match out {
            Served::Modified(f) => {
                assert!(f.screenshot_id.is_none());
                assert!(f.etag.starts_with('"') && f.etag.ends_with('"'));
                assert!(f.etag.len() > 2, "content hash etag is non-empty");
            }
            _ => panic!("desktop frame must return bytes"),
        }
    }
}
