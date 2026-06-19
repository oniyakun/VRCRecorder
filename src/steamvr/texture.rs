//! In-memory PNG texture generator for the SteamVR overlay panel.
//! Uses the `image` crate (already in deps) — no external font rendering.

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};

const W: u32 = 512;
const H: u32 = 256;
const BG: [u8; 3] = [24, 24, 28];
const GREEN: [u8; 3] = [76, 175, 80];
const RED: [u8; 3] = [255, 59, 48];
const WHITE: [u8; 3] = [220, 220, 224];
const DIM: [u8; 3] = [100, 100, 108];

/// Generate a 512x256 RGBA PNG for the dashboard overlay.
/// `is_recording` controls the status indicator color and text.
/// `language` selects translated strings (currently ASCII-only due to pixel font limitations).
pub fn generate(is_recording: bool, language: &str) -> Vec<u8> {
    let mut img = RgbaImage::from_pixel(W, H, image::Rgba([BG[0], BG[1], BG[2], 255]));

    // Status dot (left side, vertically centered)
    let color = if is_recording { RED } else { GREEN };
    filled_circle(&mut img, 72, 100, 40, color);

    // Select strings based on language
    // NOTE: The pixel font only supports A-Z, 0-9, and space.
    // CJK characters cannot be rendered. If a CJK pixel font is added later,
    // update this match to return native-language strings.
    let (app_label, status_text, hint) = match language {
        // All languages use ASCII for now due to pixel font limitation
        _ => (
            "VRCRecorder",
            if is_recording { "RECORDING" } else { "READY" },
            "Bind toggle_record action to a controller button",
        ),
    };

    // App label
    draw_text(&mut img, 140, 60, app_label, WHITE, 2);

    // Status text
    draw_text(&mut img, 140, 130, status_text, color, 3);

    // Hint text
    draw_text(&mut img, 24, 210, hint, DIM, 1);

    // Encode PNG
    let mut buf = Vec::new();
    let enc = PngEncoder::new(&mut buf);
    enc.write_image(&img, W, H, ExtendedColorType::Rgba8)
        .expect("PNG encode failed");
    buf
}

/// Draw a filled circle (point-in-radius check).
fn filled_circle(img: &mut RgbaImage, cx: u32, cy: u32, r: u32, color: [u8; 3]) {
    let (x0, y0) = (cx.saturating_sub(r), cy.saturating_sub(r));
    let (x1, y1) = ((cx + r).min(W - 1), (cy + r).min(H - 1));
    let r2 = (r * r) as i32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as i32 - cx as i32;
            let dy = y as i32 - cy as i32;
            if dx * dx + dy * dy <= r2 {
                img.put_pixel(x, y, image::Rgba([color[0], color[1], color[2], 255]));
            }
        }
    }
}

/// Minimal 5x7 pixel font for uppercase letters, digits, and space.
/// Each glyph is 7 rows of 5 bits packed into a u8 per row (MSB = leftmost pixel).
/// A=0, B=1, ..., Z=25, 0=26, ..., 9=35, space=36
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110, 0b00000],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110, 0b00000],
        'E' => [0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111, 0b00000],
        'F' => [0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000, 0b00000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
        'H' => [0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001, 0b00000],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110, 0b00000],
        'J' => [0b00111, 0b00001, 0b00001, 0b00001, 0b10001, 0b01110, 0b00000],
        'K' => [0b10001, 0b10010, 0b11100, 0b10010, 0b10001, 0b10001, 0b00000],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111, 0b00000],
        'M' => [0b10001, 0b11011, 0b10101, 0b10001, 0b10001, 0b10001, 0b00000],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b00000],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110, 0b00000],
        'P' => [0b11110, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000, 0b00000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101, 0b00000],
        'R' => [0b11110, 0b10001, 0b11110, 0b10010, 0b10001, 0b10001, 0b00000],
        'S' => [0b01111, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110, 0b00000],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110, 0b00000],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100, 0b00000],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b11011, 0b10001, 0b00000],
        'X' => [0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001, 0b00000],
        'Y' => [0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000],
        'Z' => [0b11111, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111, 0b00000],
        '0' => [0b01110, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110, 0b00000],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110, 0b00000],
        '2' => [0b01110, 0b10001, 0b00010, 0b00100, 0b01000, 0b11111, 0b00000],
        '3' => [0b01110, 0b10001, 0b00110, 0b00001, 0b10001, 0b01110, 0b00000],
        '4' => [0b00010, 0b00110, 0b01010, 0b11111, 0b00010, 0b00010, 0b00000],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110, 0b00000],
        '6' => [0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110, 0b00000],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b00000],
        '8' => [0b01110, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110, 0b00000],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110, 0b00000],
        _ => [0; 7], // space and unknown = blank
    }
}

/// Draw a text string at (x, y) with given scale (1=5x7, 2=10x14, 3=15x21).
fn draw_text(img: &mut RgbaImage, x: u32, y: u32, text: &str, color: [u8; 3], scale: u32) {
    let mut cx = x;
    for c in text.chars() {
        let g = glyph(c);
        for row in 0..7 {
            let bits = g[row];
            for col in 0..6 {
                if bits & (1 << (6 - col)) != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let px = cx + col * scale + sx;
                            let py = y + row as u32 * scale + sy;
                            if px < W && py < H {
                                img.put_pixel(px, py, image::Rgba([color[0], color[1], color[2], 255]));
                            }
                        }
                    }
                }
            }
        }
        cx += 6 * scale + scale; // glyph width + gap
    }
}
