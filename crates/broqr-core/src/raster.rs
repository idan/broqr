//! QL-810W monochrome, 300 dpi raster commands.
//!
//! Protocol source: Brother QL-800/810W/820NWB Raster Command Reference v1.01:
//! https://download.brother.com/welcome/docp100278/cv_ql800_eng_raster_101.pdf
//! Sections 2.1, 2.3.3–2.3.5 and 4. No compression or two-color mode is used.

use crate::{PreparedLabel, FEED_MARGIN_DOTS, PRINTABLE_WIDTH};

const HEAD_DOTS: u32 = 720;
const SIDE_MARGIN: u32 = 12;
const BYTES_PER_ROW: usize = 90;
const HEADER_LENGTH: usize = 442;

pub(crate) const SWITCH_RASTER: &[u8] = &[0x1b, 0x69, 0x61, 0x01];
pub(crate) const STATUS_REQUEST: &[u8] = &[0x1b, 0x69, 0x53];

pub(crate) fn encode(label: &PreparedLabel) -> Vec<u8> {
    let rows = label.layout().height_dots;
    let mut out = Vec::with_capacity(HEADER_LENGTH + rows as usize * 93 + 1);
    out.resize(400, 0); // Invalidate command buffer.
    out.extend_from_slice(&[0x1b, 0x40]); // Initialize.
    out.extend_from_slice(SWITCH_RASTER);
    out.extend_from_slice(&[0x1b, 0x69, 0x21, 0x00]); // Automatic status notification on.
                                                      // Media type/width valid; quality priority and recovery on. Continuous
                                                      // tape has no fixed media length, so its length-valid bit stays off.
    out.extend_from_slice(&[0x1b, 0x69, 0x7a, 0xc6, 0x0a, 62, 0]);
    out.extend_from_slice(&rows.to_le_bytes());
    out.extend_from_slice(&[0, 0]); // First page; reserved.
    out.extend_from_slice(&[0x1b, 0x69, 0x4d, 0x40]); // Auto cut.
    out.extend_from_slice(&[0x1b, 0x69, 0x41, 0x01]); // Cut each label.
    out.extend_from_slice(&[0x1b, 0x69, 0x4b, 0x08]); // Cut at end; mono; 300 dpi.
    out.extend_from_slice(&[0x1b, 0x69, 0x64]);
    out.extend_from_slice(&(FEED_MARGIN_DOTS as u16).to_le_bytes());
    out.extend_from_slice(&[0x4d, 0x00]); // No compression.
    debug_assert_eq!(out.len(), HEADER_LENGTH);

    for y in 0..rows {
        let mut packed = [0u8; BYTES_PER_ROW];
        for x in 0..PRINTABLE_WIDTH {
            if label.pixels().get_pixel(x, y).0[0] == 0 {
                // The first transmitted pin is at the right-hand side of the
                // artwork. Reverse X on the wire, not in the preview.
                let pin = SIDE_MARGIN + PRINTABLE_WIDTH - 1 - x;
                debug_assert!(pin < HEAD_DOTS - SIDE_MARGIN);
                packed[(pin / 8) as usize] |= 1 << (7 - pin % 8);
            }
        }
        out.extend_from_slice(&[0x67, 0x00, BYTES_PER_ROW as u8]);
        out.extend_from_slice(&packed);
    }
    out.push(0x1a); // Print last page with feeding.
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepare_image;
    use image::{GrayImage, Luma};

    #[test]
    fn header_matches_documented_single_label_commands() {
        let label = prepare_image(GrayImage::from_pixel(696, 696, Luma([255])).into()).unwrap();
        let bytes = encode(&label);
        assert!(bytes[..400].iter().all(|b| *b == 0));
        assert_eq!(
            &bytes[400..410],
            &[0x1b, 0x40, 0x1b, 0x69, 0x61, 1, 0x1b, 0x69, 0x21, 0]
        );
        assert_eq!(
            &bytes[410..423],
            &[0x1b, 0x69, 0x7a, 0xc6, 0x0a, 62, 0, 0xb8, 0x02, 0, 0, 0, 0]
        );
        assert_eq!(
            &bytes[423..442],
            &[
                0x1b, 0x69, 0x4d, 0x40, 0x1b, 0x69, 0x41, 1, 0x1b, 0x69, 0x4b, 8, 0x1b, 0x69, 0x64,
                35, 0, 0x4d, 0
            ]
        );
        assert_eq!(bytes.len(), HEADER_LENGTH + 696 * 93 + 1);
        assert_eq!(bytes.last(), Some(&0x1a));
    }

    #[test]
    fn white_rasters_are_zero_and_have_exactly_ninety_bytes() {
        let label = prepare_image(GrayImage::from_pixel(696, 696, Luma([255])).into()).unwrap();
        let bytes = encode(&label);
        for row in bytes[HEADER_LENGTH..bytes.len() - 1].as_chunks::<93>().0 {
            assert_eq!(&row[..3], &[0x67, 0, 90]);
            assert!(row[3..].iter().all(|b| *b == 0));
        }
    }

    #[test]
    fn packs_edges_msb_first_with_twelve_unused_pins_each_side() {
        let mut image = GrayImage::from_pixel(696, 696, Luma([255]));
        image.put_pixel(0, 0, Luma([0]));
        image.put_pixel(695, 1, Luma([0]));
        let label = prepare_image(image.into()).unwrap();
        let bytes = encode(&label);
        let first = &bytes[HEADER_LENGTH + 3..HEADER_LENGTH + 93];
        let second = &bytes[HEADER_LENGTH + 96..HEADER_LENGTH + 186];
        assert_eq!(first[88], 0x10);
        assert_eq!(second[1], 0x08);
        assert_eq!(first.iter().map(|b| b.count_ones()).sum::<u32>(), 1);
        assert_eq!(second.iter().map(|b| b.count_ones()).sum::<u32>(), 1);
        for row in [first, second] {
            assert_eq!(row[0], 0);
            assert_eq!(row[1] & 0xf0, 0);
            assert_eq!(row[88] & 0x0f, 0);
            assert_eq!(row[89], 0);
        }
    }

    #[test]
    fn recovers_preview_pixels_from_every_transmitted_bit() {
        let image = GrayImage::from_fn(696, 731, |x, y| {
            Luma([if (x * 7 + y * 11) % 31 < 9 { 0 } else { 255 }])
        });
        let label = prepare_image(image.into()).unwrap();
        let bytes = encode(&label);
        for y in 0..label.layout().height_dots {
            let start = HEADER_LENGTH + y as usize * 93 + 3;
            let row = &bytes[start..start + 90];
            for x in 0..PRINTABLE_WIDTH {
                let pin = SIDE_MARGIN + PRINTABLE_WIDTH - 1 - x;
                let black = row[(pin / 8) as usize] & (1 << (7 - pin % 8)) != 0;
                assert_eq!(black, label.pixels().get_pixel(x, y).0[0] == 0);
            }
        }
    }
}
