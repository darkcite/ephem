//! QR codes (§8.2): rendering the code link as SVG, and decoding camera frames with `rqrr`
//! (Safari has no `BarcodeDetector`). Setup path (one per code / scan attempt): may allocate.

use core::fmt::Write;
use qrcode::{Color, EcLevel, QrCode};
use wasm_bindgen::prelude::*;

/// SVG (quiet zone 4, black on white) for `text`, or an empty string if it does not fit.
#[wasm_bindgen]
pub fn qr_svg_path(text: &str) -> String {
    let Ok(code) = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M) else {
        return String::new();
    };
    let w = code.width();
    let colors = code.to_colors();
    let mut s = String::with_capacity(160 + w * w * 6);
    let _ = write!(
        s,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"-4 -4 {v} {v}\" shape-rendering=\"crispEdges\">\
         <rect x=\"-4\" y=\"-4\" width=\"{v}\" height=\"{v}\" fill=\"#fff\"/><path fill=\"#000\" d=\"",
        v = w + 8
    );
    for y in 0..w {
        let row = &colors[y * w..(y + 1) * w];
        for (x, c) in row.iter().enumerate() {
            if *c == Color::Dark {
                let _ = write!(s, "M{x} {y}h1v1h-1z");
            }
        }
    }
    s.push_str("\"/></svg>");
    s
}

/// Decodes the first QR code in an RGBA frame (`width × height × 4` bytes); empty if none.
/// Frames come from the camera through the preallocated scan buffer (one documented copy, §11.6).
///
/// rqrr (a quirc port) keeps at most 251 flood-filled regions: on real camera frames of a
/// screen (blur, noise, glare, light falling off to one side) the speckle along every module
/// edge evicts the finder patterns and nothing is found. So the frame is cleaned first:
/// 3×3 box denoise → local adaptive threshold (window ≈ 1/8 of the short side) → 3×3 majority
/// filter that removes isolated specks. The same without the denoise (crisp frames), and the raw
/// greyscale, are tried as well.
/// Scan path, one frame every 200 ms: allocates per frame.
pub fn scan_rgba(px: &[u8], width: usize, height: usize) -> String {
    debug_assert_eq!(px.len(), width * height * 4);
    let mut grey = vec![0u8; width * height];
    for (i, g) in grey.iter_mut().enumerate() {
        let p = &px[i * 4..i * 4 + 3];
        // Integer BT.601 luma.
        *g = ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8) as u8;
    }
    let clean = majority(&adaptive_threshold(&box3(&grey, width, height), width, height), width, height);
    let sharp = majority(&adaptive_threshold(&grey, width, height), width, height);
    for img in [&clean, &sharp, &grey] {
        let text = decode(img, width, height);
        if !text.is_empty() {
            return text;
        }
    }
    String::new()
}

fn decode(grey: &[u8], width: usize, height: usize) -> String {
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| grey[y * width + x]);
    for grid in img.detect_grids() {
        if let Ok((_, text)) = grid.decode() {
            return text;
        }
    }
    String::new()
}

/// 3×3 box filter (borders copied).
fn box3(g: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = g.to_vec();
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let mut s = 0u32;
            for dy in 0..3 {
                let row = (y + dy - 1) * w + x - 1;
                s += g[row] as u32 + g[row + 1] as u32 + g[row + 2] as u32;
            }
            out[y * w + x] = (s / 9) as u8;
        }
    }
    out
}

/// 3×3 majority vote on a binary image: removes isolated specks and fills pinholes.
fn majority(b: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = b.to_vec();
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let mut dark = 0;
            for dy in 0..3 {
                let row = (y + dy - 1) * w + x - 1;
                dark += (b[row] == 0) as u32 + (b[row + 1] == 0) as u32 + (b[row + 2] == 0) as u32;
            }
            out[y * w + x] = if dark >= 5 { 0 } else { 255 };
        }
    }
    out
}

/// Dark (0) where a pixel is darker than 90 % of its neighbourhood mean, light (255) elsewhere.
fn adaptive_threshold(grey: &[u8], w: usize, h: usize) -> Vec<u8> {
    let stride = w + 1;
    let mut sum = vec![0u32; stride * (h + 1)];
    for y in 0..h {
        let mut row = 0u32;
        for x in 0..w {
            row += grey[y * w + x] as u32;
            sum[(y + 1) * stride + x + 1] = sum[y * stride + x + 1] + row;
        }
    }
    let r = (w.min(h) / 16).max(8);
    let mut out = vec![255u8; w * h];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let total = sum[y1 * stride + x1] + sum[y0 * stride + x0] - sum[y0 * stride + x1] - sum[y1 * stride + x0];
            let area = ((y1 - y0) * (x1 - x0)) as u32;
            // grey < 0.9 × mean  ⇔  grey × area × 10 < total × 9
            if (grey[y * w + x] as u32) * area * 10 < total * 9 {
                out[y * w + x] = 0;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders a code with `qrcode`, rasterizes it, and reads it back with `rqrr`.
    #[test]
    fn scan_roundtrip() {
        let text = "https://darkcite.github.io/ephem/app/#i=AQEAAgcHBwcHBwcHBwcHBwcHBwcJCQkJ";
        let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).unwrap();
        let (w, scale, quiet) = (code.width(), 4, 4);
        let side = (w + 2 * quiet) * scale;
        let colors = code.to_colors();
        let mut px = vec![255u8; side * side * 4];
        for y in 0..side {
            for x in 0..side {
                let (mx, my) = ((x / scale) as isize - quiet as isize, (y / scale) as isize - quiet as isize);
                let dark = mx >= 0 && my >= 0 && (mx as usize) < w && (my as usize) < w && colors[my as usize * w + mx as usize] == Color::Dark;
                if dark {
                    let i = (y * side + x) * 4;
                    px[i..i + 3].fill(0);
                }
            }
        }
        assert_eq!(scan_rgba(&px, side, side), text);
        assert_eq!(scan_rgba(&vec![255u8; 64 * 64 * 4], 64, 64), "");
    }
}

#[cfg(test)]
mod camera_like {
    use super::*;

    /// A camera-like frame: real-size invite link, `px_per_module` scale, box blur, low contrast,
    /// uneven lighting, noise, QR off-centre in a larger frame.
    fn frame(text: &str, px_per_module: usize, blur: usize, dark: u8, light: u8) -> (Vec<u8>, usize, usize) {
        let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).unwrap();
        let (n, colors) = (code.width(), code.to_colors());
        let (w, h) = (1080usize, 1440usize);
        let side = (n + 8) * px_per_module;
        let (x0, y0) = ((w - side) / 3, (h - side) / 2);
        let mut g = vec![light as f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let (mx, my) = (((x as isize - x0 as isize) / px_per_module as isize) - 4, ((y as isize - y0 as isize) / px_per_module as isize) - 4);
                if x >= x0 && y >= y0 && mx >= 0 && my >= 0 && (mx as usize) < n && (my as usize) < n && colors[my as usize * n + mx as usize] == Color::Dark {
                    g[y * w + x] = dark as f32;
                }
            }
        }
        for _ in 0..blur {
            let src = g.clone();
            for y in 1..h - 1 {
                for x in 1..w - 1 {
                    let mut s = 0.0;
                    for dy in 0..3 {
                        for dx in 0..3 {
                            s += src[(y + dy - 1) * w + x + dx - 1];
                        }
                    }
                    g[y * w + x] = s / 9.0;
                }
            }
        }
        let mut seed = 12345u32;
        let mut px = vec![255u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let noise = ((seed >> 24) as f32 - 128.0) / 8.0;
                let shade = 0.75 + 0.25 * (x as f32 / w as f32); // light falls off to one side
                let v = (g[y * w + x] * shade + noise).clamp(0.0, 255.0) as u8;
                let i = (y * w + x) * 4;
                px[i..i + 3].fill(v);
            }
        }
        (px, w, h)
    }

    const LINK: &str = "https://abc-def-ghi.trycloudflare.com/app/#i=AQEAAxYH3kK1TQhGbKzYl8aQkS2vTj7Gd1a5wRRmXkQ1Bv4oH1Zt0c9rXk2sY5Wq8LkQ9vG3J7nD2pP6hB1mT4xR0aE8uYFz9cK3qW6oL5iN2gS7dV1bM4hJ8tA0yE3rU6wQ9nX2kC5fZ8vH1pB4sG7jD0lT3mR6aY9eK2cN5wV8oU1iF4qS7xL0hZ3gM6tJ9bD2rP5yE8kW1nA4vC7fH0sQ3uX6";

    #[test]
    fn decodes_degraded_camera_frames() {
        // Regression: rqrr alone fails on half of these (its 251-region cache overflows with speckle).
        for (ppm, blur, dark, light) in [(6, 0, 20, 235), (5, 1, 60, 200), (6, 1, 20, 235), (6, 2, 60, 200), (8, 1, 90, 170), (8, 0, 60, 200)] {
            let (px, w, h) = frame(LINK, ppm, blur, dark, light);
            assert_eq!(scan_rgba(&px, w, h), LINK, "ppm={ppm} blur={blur} contrast={dark}..{light}");
        }
    }
}
