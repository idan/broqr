//! Driver-free image printing for the Brother QL-810W with 62 mm continuous tape.
//!
//! No image content is cropped or regenerated. The shorter edge fills the
//! printable width; landscape images rotate clockwise to keep the long edge
//! along the roll. Preview and printer commands use the same binary pixels.

mod label;
mod network;
mod raster;
mod status;

pub use label::{
    prepare_file, prepare_image, prepare_rgba, LabelLayout, PreparedLabel, DPI, FEED_MARGIN_DOTS,
    MAX_INPUT_BYTES, MAX_INPUT_PIXELS, MAX_PRINT_ROWS, PRINTABLE_WIDTH,
};
pub use network::{
    check_printer, print_label, NetworkOptions, PrintOutcome, PrinterAddress, PrinterCheck,
};
pub use status::PrinterStatus;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    InvalidImage(String),
    #[error("Could not decode the image: {0}")]
    Decode(#[from] image::ImageError),
    #[error("File operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Network(String),
    #[error("Invalid printer response: {0}")]
    Protocol(String),
    #[error("Printer: {0}")]
    Printer(String),
}

pub type Result<T> = std::result::Result<T, Error>;
