//! QR rendering (§8.2): the code link as an SVG. Setup path (one per code), so it may allocate.

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
