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
pub fn scan_rgba(px: &[u8], width: usize, height: usize) -> String {
    debug_assert_eq!(px.len(), width * height * 4);
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| {
        let i = (y * width + x) * 4;
        // Integer BT.601 luma.
        ((px[i] as u32 * 77 + px[i + 1] as u32 * 150 + px[i + 2] as u32 * 29) >> 8) as u8
    });
    for grid in img.detect_grids() {
        if let Ok((_, text)) = grid.decode() {
            return text;
        }
    }
    String::new()
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
