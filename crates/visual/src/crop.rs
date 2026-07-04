use crate::VisualError;
use image::{DynamicImage, GenericImageView};
use std::sync::Arc;

pub fn crop_image(img: &DynamicImage, x: u32, y: u32, w: u32, h: u32) -> Result<Arc<[u8]>, VisualError> {
    if x >= img.width() || y >= img.height() {
        return Err(VisualError::Image("Crop coordinates out of bounds".to_string()));
    }

    let cw = std::cmp::min(w, img.width() - x);
    let ch = std::cmp::min(h, img.height() - y);

    if cw == 0 || ch == 0 {
        return Err(VisualError::Image("Crop width or height is zero".to_string()));
    }

    let cropped = img.view(x, y, cw, ch).to_image();
    let dynamic_cropped = DynamicImage::ImageRgba8(cropped);

    // Maximum 512px long edge per masterplan
    let resized = if cw > 512 || ch > 512 {
        dynamic_cropped.resize(512, 512, image::imageops::FilterType::Lanczos3)
    } else {
        dynamic_cropped
    };

    let mut cursor = std::io::Cursor::new(Vec::new());
    resized.write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| VisualError::Image(e.to_string()))?;

    Ok(Arc::from(cursor.into_inner().into_boxed_slice()))
}
