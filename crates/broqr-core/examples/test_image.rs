//! Make a synthetic QR + text image for a physical printer smoke test.
//! cargo run -p broqr-core --example test_image -- /tmp/broqr-test.png

use std::path::PathBuf;

use image::{GrayImage, Luma};
use qrcodegen::{QrCode, QrCodeEcc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("broqr-test.png"));
    let qr = QrCode::encode_text("BROQR TEST - NOT AN AMAZON RETURN CODE", QrCodeEcc::Medium)?;
    let scale = 8u32;
    let side = (qr.size() as u32 + 8) * scale;
    let mut image = GrayImage::from_pixel(side, side + 56, Luma([255]));
    for y in 0..side {
        for x in 0..side {
            if qr.get_module((x / scale) as i32 - 4, (y / scale) as i32 - 4) {
                image.put_pixel(x, y, Luma([0]));
            }
        }
    }
    // Tiny original 5×7 bitmap lettering: "BROQR TEST".
    let glyphs: [[u8; 7]; 10] = [
        [30, 17, 17, 30, 17, 17, 30], // B
        [30, 17, 17, 30, 20, 18, 17], // R
        [14, 17, 17, 17, 17, 17, 14], // O
        [14, 17, 17, 17, 21, 18, 13], // Q
        [30, 17, 17, 30, 20, 18, 17], // R
        [0, 0, 0, 0, 0, 0, 0],
        [31, 4, 4, 4, 4, 4, 4],       // T
        [31, 16, 16, 30, 16, 16, 31], // E
        [15, 16, 16, 14, 1, 1, 30],   // S
        [31, 4, 4, 4, 4, 4, 4],       // T
    ];
    let glyph_scale = 4u32;
    let start_x = (side - 59 * glyph_scale) / 2;
    for (index, glyph) in glyphs.iter().enumerate() {
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5u32 {
                if bits & (1 << (4 - col)) != 0 {
                    for dy in 0..glyph_scale {
                        for dx in 0..glyph_scale {
                            image.put_pixel(
                                start_x + (index as u32 * 6 + col) * glyph_scale + dx,
                                side + 8 + row as u32 * glyph_scale + dy,
                                Luma([0]),
                            );
                        }
                    }
                }
            }
        }
    }
    image.save(&path)?;
    println!("Test image: {}", path.display());
    println!("This is a synthetic test code, not a return label.");
    Ok(())
}
