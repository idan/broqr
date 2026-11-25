use std::{io::Cursor, path::Path};

use image::{
    imageops::FilterType, DynamicImage, GenericImageView, GrayImage, ImageDecoder, ImageFormat,
    ImageReader, Limits, Luma, RgbaImage,
};
use serde::Serialize;

use crate::{raster, Error, Result};

pub const DPI: u32 = 300;
pub const PRINTABLE_WIDTH: u32 = 696;
pub const FEED_MARGIN_DOTS: u32 = 35;
pub const MAX_PRINT_ROWS: u32 = 11_811;
pub const MAX_INPUT_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_INPUT_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_DECODE_DIMENSION: u32 = 32_768;
const BLACK_THRESHOLD: u8 = 128;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LabelLayout {
    pub source_width: u32,
    pub source_height: u32,
    pub width_dots: u32,
    pub height_dots: u32,
    pub rotated: bool,
    pub artwork_width_mm: f64,
    pub artwork_length_mm: f64,
    /// Estimated total length, including the minimum feed margin at each end.
    pub label_length_mm: f64,
}

impl LabelLayout {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        validate_dimensions(width, height)?;
        let short = width.min(height);
        let long = width.max(height);
        // Integer arithmetic gives round-to-nearest without floating-point drift.
        let rows = (u64::from(long) * u64::from(PRINTABLE_WIDTH) + u64::from(short) / 2)
            / u64::from(short);
        if rows > u64::from(MAX_PRINT_ROWS) {
            return Err(Error::InvalidImage(format!(
                "This image would require {rows} print rows; the QL-810W supports at most                  {MAX_PRINT_ROWS}. Choose a less elongated image. Nothing was cropped."
            )));
        }
        let height_dots = rows as u32;
        let mm = |dots: u32| f64::from(dots) * 25.4 / f64::from(DPI);
        Ok(Self {
            source_width: width,
            source_height: height,
            width_dots: PRINTABLE_WIDTH,
            height_dots,
            rotated: width > height,
            artwork_width_mm: mm(PRINTABLE_WIDTH),
            artwork_length_mm: mm(height_dots),
            label_length_mm: mm(height_dots + 2 * FEED_MARGIN_DOTS),
        })
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(Error::InvalidImage("The image is empty.".into()));
    }
    if u64::from(width) * u64::from(height) > MAX_INPUT_PIXELS {
        return Err(Error::InvalidImage(
            "The image is too large. The limit is 64 megapixels.".into(),
        ));
    }
    Ok(())
}

/// A prepared, immutable label. Both preview and printer data derive from this
/// exact pixel buffer, so changing or deleting the source file cannot change a job.
#[derive(Clone, Debug)]
pub struct PreparedLabel {
    layout: LabelLayout,
    pixels: GrayImage,
}

impl PreparedLabel {
    pub fn layout(&self) -> &LabelLayout {
        &self.layout
    }

    /// The 696-dot-wide black/white artwork, without mechanical margins.
    pub fn pixels(&self) -> &GrayImage {
        &self.pixels
    }

    pub fn preview_png(&self) -> Result<Vec<u8>> {
        let mut output = Cursor::new(Vec::new());
        self.pixels.write_to(&mut output, ImageFormat::Png)?;
        Ok(output.into_inner())
    }

    pub fn raster_bytes(&self) -> Vec<u8> {
        raster::encode(self)
    }
}

pub fn prepare_file(path: impl AsRef<Path>) -> Result<PreparedLabel> {
    let path = path.as_ref();
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(Error::InvalidImage(
            "Select an image file, not a directory.".into(),
        ));
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err(Error::InvalidImage(
            "The image file is too large. The limit is 32 MiB.".into(),
        ));
    }
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_DIMENSION);
    limits.max_image_height = Some(MAX_DECODE_DIMENSION);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    validate_dimensions(width, height)?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    prepare_image(image)
}

pub fn prepare_rgba(width: u32, height: u32, bytes: Vec<u8>) -> Result<PreparedLabel> {
    validate_dimensions(width, height)?;
    let image = RgbaImage::from_raw(width, height, bytes)
        .ok_or_else(|| Error::InvalidImage("The clipboard image has invalid pixel data.".into()))?;
    prepare_image(DynamicImage::ImageRgba8(image))
}

pub fn prepare_image(image: DynamicImage) -> Result<PreparedLabel> {
    let (width, height) = image.dimensions();
    let layout = LabelLayout::new(width, height)?;
    let oriented = if layout.rotated {
        image.rotate90()
    } else {
        image
    };
    let scaled = oriented.resize_exact(layout.width_dots, layout.height_dots, FilterType::Nearest);
    // Composite transparency against white before thresholding. Transparent
    // black pixels must not turn into printed black backgrounds.
    let rgba = scaled.into_rgba8();
    let pixels = GrayImage::from_fn(layout.width_dots, layout.height_dots, |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let composite = |channel: u8| -> u32 {
            (u32::from(channel) * u32::from(a) + 255 * (255 - u32::from(a)) + 127) / 255
        };
        let luminance = (299 * composite(r) + 587 * composite(g) + 114 * composite(b) + 500) / 1000;
        Luma([if luminance < u32::from(BLACK_THRESHOLD) {
            0
        } else {
            255
        }])
    });
    Ok(PreparedLabel { layout, pixels })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use qrcodegen::{QrCode, QrCodeEcc};

    #[test]
    fn square_and_portrait_fill_short_edge() {
        let square = LabelLayout::new(800, 800).unwrap();
        assert_eq!((square.width_dots, square.height_dots), (696, 696));
        assert!(!square.rotated);
        let portrait = LabelLayout::new(800, 1000).unwrap();
        assert_eq!((portrait.width_dots, portrait.height_dots), (696, 870));
        assert!(!portrait.rotated);
        let landscape = LabelLayout::new(1000, 800).unwrap();
        assert_eq!((landscape.width_dots, landscape.height_dots), (696, 870));
        assert!(landscape.rotated);
    }

    #[test]
    fn lengths_include_only_mechanical_feed() {
        let layout = LabelLayout::new(1, 1).unwrap();
        assert!((layout.artwork_width_mm - 58.928).abs() < 0.001);
        assert!((layout.label_length_mm - 64.854_666).abs() < 0.001);
    }

    #[test]
    fn rejects_empty_oversized_and_overlong_images() {
        assert!(LabelLayout::new(0, 10).is_err());
        assert!(LabelLayout::new(u32::MAX, u32::MAX).is_err());
        assert!(LabelLayout::new(1, 100).is_err());
        assert!(LabelLayout::new(696, MAX_PRINT_ROWS).is_ok());
        assert!(LabelLayout::new(696, MAX_PRINT_ROWS + 1).is_err());
    }

    #[test]
    fn preserves_entire_portrait_image_including_bottom_content() {
        let mut image = RgbaImage::from_pixel(4, 5, Rgba([255, 255, 255, 255]));
        image.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        image.put_pixel(3, 4, Rgba([0, 0, 0, 255]));
        let label = prepare_image(image.into()).unwrap();
        assert_eq!(label.pixels.get_pixel(0, 0).0, [0]);
        assert_eq!(label.pixels.get_pixel(695, 869).0, [0]);
        assert!(label.pixels.pixels().all(|p| p.0[0] == 0 || p.0[0] == 255));
    }

    #[test]
    fn rotates_landscape_clockwise_without_mirroring() {
        let mut image = RgbaImage::from_pixel(5, 4, Rgba([255, 255, 255, 255]));
        image.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        let label = prepare_image(image.into()).unwrap();
        assert!(label.layout.rotated);
        assert_eq!(label.pixels.get_pixel(695, 0).0, [0]);
        assert_eq!(label.pixels.get_pixel(0, 0).0, [255]);
    }

    #[test]
    fn alpha_is_composited_onto_white_and_no_dithering_is_used() {
        let mut image = RgbaImage::from_pixel(3, 3, Rgba([0, 0, 0, 0]));
        image.put_pixel(1, 0, Rgba([127, 127, 127, 255]));
        image.put_pixel(2, 0, Rgba([128, 128, 128, 255]));
        let label = prepare_image(image.into()).unwrap();
        assert_eq!(label.pixels.get_pixel(0, 0).0, [255]);
        assert_eq!(label.pixels.get_pixel(300, 0).0, [0]);
        assert_eq!(label.pixels.get_pixel(695, 0).0, [255]);
    }

    #[test]
    fn preview_contains_the_exact_print_pixels() {
        let image = RgbaImage::from_pixel(2, 2, Rgba([0, 0, 0, 255]));
        let label = prepare_image(image.into()).unwrap();
        let decoded = image::load_from_memory(&label.preview_png().unwrap())
            .unwrap()
            .into_luma8();
        assert_eq!(decoded, label.pixels);
    }

    #[test]
    fn nearest_neighbor_output_retains_decodable_qr_with_bottom_content() {
        let payload = "https://example.test/amazon-return/12345";
        let qr = QrCode::encode_text(payload, QrCodeEcc::Medium).unwrap();
        let modules = qr.size() as u32 + 8; // retain a four-module quiet zone
        let scale = 5;
        let side = modules * scale;
        let mut image = RgbaImage::from_pixel(side, side + 30, Rgba([255, 255, 255, 255]));
        for y in 0..side {
            for x in 0..side {
                if qr.get_module((x / scale) as i32 - 4, (y / scale) as i32 - 4) {
                    image.put_pixel(x, y, Rgba([0, 0, 0, 255]));
                }
            }
        }
        // Stand-in for a text line, separate from the QR's own quiet zone.
        for x in 20..side - 20 {
            image.put_pixel(x, side + 15, Rgba([0, 0, 0, 255]));
        }
        let label = prepare_image(image.into()).unwrap();
        assert!(label.layout.height_dots > PRINTABLE_WIDTH);
        let mut prepared = rqrr::PreparedImage::prepare(label.pixels.clone());
        let grids = prepared.detect_grids();
        assert_eq!(grids.len(), 1);
        assert_eq!(grids[0].decode().unwrap().1, payload);
    }
}
