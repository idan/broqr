use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use broqr_core::{prepare_file, MAX_INPUT_BYTES};
use image::{GrayImage, Luma};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "broqr-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn reads_image_by_content_and_does_not_modify_original() {
    let dir = TestDir::new();
    let path = dir.0.join("image-without-extension");
    let mut png = std::io::Cursor::new(Vec::new());
    GrayImage::from_pixel(100, 125, Luma([255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let original = png.into_inner();
    std::fs::write(&path, &original).unwrap();
    let label = prepare_file(&path).unwrap();
    assert_eq!(label.layout().height_dots, 870);
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn rejects_directory_missing_corrupt_and_overlarge_files() {
    let dir = TestDir::new();
    assert!(prepare_file(&dir.0)
        .unwrap_err()
        .to_string()
        .contains("directory"));
    assert!(prepare_file(dir.0.join("missing.png")).is_err());
    let corrupt = dir.0.join("corrupt.png");
    std::fs::write(&corrupt, b"not an image").unwrap();
    assert!(prepare_file(&corrupt).is_err());
    let large = dir.0.join("large.png");
    std::fs::File::create(&large)
        .unwrap()
        .set_len(MAX_INPUT_BYTES + 1)
        .unwrap();
    assert!(prepare_file(&large)
        .unwrap_err()
        .to_string()
        .contains("32 MiB"));
}

#[test]
fn rgba_clipboard_dimensions_and_bytes_are_validated() {
    assert!(broqr_core::prepare_rgba(2, 2, vec![0; 3]).is_err());
    assert!(broqr_core::prepare_rgba(0, 0, vec![]).is_err());
}
