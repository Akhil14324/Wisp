//! Monitor enumeration, screenshots and Circle to Ask image prep
//! (section 7.3, FR-CTA-04/08/09). All geometry is in physical pixels.

use std::io::Cursor;

use image::{imageops::FilterType, DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub primary: bool,
}

fn info(m: &xcap::Monitor) -> Option<MonitorInfo> {
    Some(MonitorInfo {
        id: m.id().ok()?.to_string(),
        name: m.name().unwrap_or_default(),
        x: m.x().ok()?,
        y: m.y().ok()?,
        width: m.width().ok()?,
        height: m.height().ok()?,
        scale: m.scale_factor().unwrap_or(1.0),
        primary: m.is_primary().unwrap_or(false),
    })
}

pub fn monitors() -> Vec<MonitorInfo> {
    xcap::Monitor::all()
        .map(|all| all.iter().filter_map(info).collect())
        .unwrap_or_default()
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

/// The monitor containing a point (Circle to Ask: under the cursor).
pub fn monitor_at(x: i32, y: i32) -> Option<MonitorInfo> {
    let ms = monitors();
    ms.iter()
        .find(|m| x >= m.x && y >= m.y && x < m.x + m.width as i32 && y < m.y + m.height as i32)
        .or_else(|| ms.iter().find(|m| m.primary))
        .or_else(|| ms.first())
        .cloned()
}

/// "primary" or a monitor id from settings; falls back to primary.
pub fn monitor_for_setting(setting: &str) -> Option<MonitorInfo> {
    let ms = monitors();
    ms.iter()
        .find(|m| setting != "primary" && m.id == setting)
        .or_else(|| ms.iter().find(|m| m.primary))
        .or_else(|| ms.first())
        .cloned()
}

pub fn capture(m: &MonitorInfo) -> anyhow::Result<RgbaImage> {
    let mon = xcap::Monitor::all()?
        .into_iter()
        .find(|x| x.id().map(|i| i.to_string() == m.id).unwrap_or(false))
        .ok_or_else(|| anyhow::anyhow!("monitor disappeared"))?;
    Ok(mon.capture_image()?)
}

/// A fully black capture means DRM, the lock screen or the secure desktop (FR-CU-13).
pub fn is_black(img: &RgbaImage) -> bool {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return true;
    }
    let step_x = (w / 64).max(1);
    let step_y = (h / 64).max(1);
    for y in (0..h).step_by(step_y as usize) {
        for x in (0..w).step_by(step_x as usize) {
            let p = img.get_pixel(x, y);
            if p[0] > 8 || p[1] > 8 || p[2] > 8 {
                return false;
            }
        }
    }
    true
}

pub fn hash(img: &RgbaImage) -> [u8; 32] {
    use sha2::Digest;
    let small = image::imageops::thumbnail(img, 160, 90);
    sha2::Sha256::digest(small.as_raw()).into()
}

pub fn to_png(img: &RgbaImage) -> anyhow::Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png)?;
    Ok(out.into_inner())
}

pub fn to_jpeg(img: &RgbaImage, quality: u8) -> anyhow::Result<Vec<u8>> {
    let rgb = DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    let mut out = Cursor::new(Vec::new());
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    rgb.write_with_encoder(enc)?;
    Ok(out.into_inner())
}

/// Scales so the long edge is at most `max` pixels (never upscales).
pub fn fit_long_edge(img: &RgbaImage, max: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    let long = w.max(h);
    if long <= max {
        return img.clone();
    }
    let s = max as f64 / long as f64;
    image::imageops::resize(
        img,
        ((w as f64 * s).round() as u32).max(1),
        ((h as f64 * s).round() as u32).max(1),
        FilterType::Triangle,
    )
}

/// A freehand stroke in monitor-local physical pixels.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Stroke {
    pub points: Vec<(f32, f32)>,
}

/// Bounding box of all strokes plus padding, clamped to the image (FR-CTA-04).
pub fn strokes_bbox(strokes: &[Stroke], pad: f32, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let pts = strokes.iter().flat_map(|s| s.points.iter());
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let mut any = false;
    for (x, y) in pts {
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        any = true;
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }
    if !any {
        return None;
    }
    let cx0 = (x0 - pad).floor().clamp(0.0, w as f32) as u32;
    let cy0 = (y0 - pad).floor().clamp(0.0, h as f32) as u32;
    let cx1 = (x1 + pad).ceil().clamp(0.0, w as f32) as u32;
    let cy1 = (y1 + pad).ceil().clamp(0.0, h as f32) as u32;
    if cx1 <= cx0 || cy1 <= cy0 {
        return None;
    }
    Some((cx0, cy0, cx1 - cx0, cy1 - cy0))
}

fn dot(img: &mut RgbaImage, cx: f32, cy: f32, r: f32, color: Rgba<u8>) {
    let (w, h) = img.dimensions();
    let (x0, x1) = (
        (cx - r).floor().max(0.0) as u32,
        ((cx + r).ceil() as u32).min(w.saturating_sub(1)),
    );
    let (y0, y1) = (
        (cy - r).floor().max(0.0) as u32,
        ((cy + r).ceil() as u32).min(h.saturating_sub(1)),
    );
    if cx + r < 0.0 || cy + r < 0.0 {
        return;
    }
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            if dx * dx + dy * dy <= r * r {
                img.put_pixel(x, y, color);
            }
        }
    }
}

/// Draws strokes in red on a copy of the screenshot (FR-CTA-04).
pub fn draw_strokes(img: &RgbaImage, strokes: &[Stroke], width: f32) -> RgbaImage {
    let mut out = img.clone();
    let red = Rgba([235, 40, 40, 255]);
    let r = width / 2.0;
    for s in strokes {
        for pair in s.points.windows(2) {
            let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
            let dist = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
            let n = (dist / (r * 0.5).max(1.0)).ceil().max(1.0) as u32;
            for i in 0..=n {
                let t = i as f32 / n as f32;
                dot(&mut out, ax + (bx - ax) * t, ay + (by - ay) * t, r, red);
            }
        }
        if s.points.len() == 1 {
            dot(&mut out, s.points[0].0, s.points[0].1, r, red);
        }
    }
    out
}

pub const MAX_EDGE: u32 = 1568;
pub const PAYLOAD_LIMIT: usize = 4 * 1024 * 1024;

pub struct CirclePayload {
    pub context_jpeg: Vec<u8>,
    pub crop_png: Option<Vec<u8>>,
    /// Crop rectangle in monitor pixels (for the result card position and tests).
    pub crop_rect: Option<(u32, u32, u32, u32)>,
}

/// Builds the context image and detail crop, shrinking until under 4 MB (FR-CTA-09).
pub fn circle_payload(screenshot: &RgbaImage, strokes: &[Stroke]) -> anyhow::Result<CirclePayload> {
    let (w, h) = screenshot.dimensions();
    let stroke_w = (w.max(h) as f32 / 400.0).clamp(3.0, 8.0);
    let marked = if strokes.is_empty() {
        screenshot.clone()
    } else {
        draw_strokes(screenshot, strokes, stroke_w)
    };
    let rect = strokes_bbox(strokes, 24.0, w, h);
    let crop_img =
        rect.map(|(x, y, cw, ch)| image::imageops::crop_imm(screenshot, x, y, cw, ch).to_image());

    let mut edge = MAX_EDGE;
    let mut quality = 85u8;
    loop {
        let ctx = to_jpeg(&fit_long_edge(&marked, edge), quality)?;
        let crop = match &crop_img {
            Some(c) => Some(to_png(&fit_long_edge(c, edge))?),
            None => None,
        };
        let total = ctx.len() + crop.as_ref().map(|c| c.len()).unwrap_or(0);
        if total < PAYLOAD_LIMIT || edge <= 400 {
            return Ok(CirclePayload {
                context_jpeg: ctx,
                crop_png: crop,
                crop_rect: rect,
            });
        }
        edge = edge * 3 / 4;
        quality = quality.saturating_sub(5).max(60);
    }
}

/// Encodes a Computer Use screenshot under the request size budget.
/// Coordinates are normalized, so a smaller image does not change them.
pub fn encode_for_agent(img: &RgbaImage) -> anyhow::Result<(&'static str, Vec<u8>)> {
    let png = to_png(img)?;
    if png.len() < 3 * 1024 * 1024 {
        return Ok(("image/png", png));
    }
    let mut edge = img.width().max(img.height());
    loop {
        let j = to_jpeg(&fit_long_edge(img, edge), 85)?;
        if j.len() < 3 * 1024 * 1024 || edge <= 800 {
            return Ok(("image/jpeg", j));
        }
        edge = edge * 3 / 4;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(pts: &[(f32, f32)]) -> Stroke {
        Stroke {
            points: pts.to_vec(),
        }
    }

    #[test]
    fn bbox_pads_and_clamps() {
        let s = [
            stroke(&[(100.0, 100.0), (200.0, 150.0)]),
            stroke(&[(10.0, 300.0)]),
        ];
        assert_eq!(strokes_bbox(&s, 24.0, 1920, 1080), Some((0, 76, 224, 248)));
        let edge = [stroke(&[(1910.0, 1070.0), (1919.0, 1079.0)])];
        assert_eq!(
            strokes_bbox(&edge, 24.0, 1920, 1080),
            Some((1886, 1046, 34, 34))
        );
        assert_eq!(strokes_bbox(&[], 24.0, 100, 100), None);
    }

    #[test]
    fn scaling_respects_long_edge() {
        let img = RgbaImage::new(3840, 2160);
        let s = fit_long_edge(&img, 1568);
        assert_eq!(s.width(), 1568);
        assert_eq!(s.height(), 882);
        let small = RgbaImage::new(800, 600);
        assert_eq!(fit_long_edge(&small, 1568).dimensions(), (800, 600));
    }

    #[test]
    fn black_detection() {
        assert!(is_black(&RgbaImage::new(200, 100)));
        let mut img = RgbaImage::new(200, 100);
        for p in img.pixels_mut() {
            *p = Rgba([30, 30, 30, 255]);
        }
        assert!(!is_black(&img));
    }

    #[test]
    fn payload_stays_under_limit() {
        // Noise compresses badly, which exercises the shrink loop.
        let mut img = RgbaImage::new(2560, 1440);
        let mut seed = 1u32;
        for p in img.pixels_mut() {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *p = Rgba([
                (seed >> 24) as u8,
                (seed >> 16) as u8,
                (seed >> 8) as u8,
                255,
            ]);
        }
        let s = [stroke(&[(100.0, 100.0), (2400.0, 1300.0)])];
        let p = circle_payload(&img, &s).unwrap();
        assert!(p.context_jpeg.len() + p.crop_png.as_ref().unwrap().len() < PAYLOAD_LIMIT);
    }

    #[test]
    fn strokes_are_drawn_red() {
        let img = RgbaImage::new(100, 100);
        let out = draw_strokes(&img, &[stroke(&[(10.0, 10.0), (90.0, 10.0)])], 4.0);
        assert_eq!(out.get_pixel(50, 10)[0], 235);
        assert_eq!(out.get_pixel(50, 50)[0], 0);
    }
}
