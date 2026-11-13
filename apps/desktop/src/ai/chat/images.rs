//! Normalize every managed image at the request boundary, including JPEG
//! screenshots and restored attachments. Keep the original local file intact.
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageFormat, ImageReader};
use lennette_protocol::{MAX_CHAT_IMAGE_BYTES, MAX_CHAT_IMAGE_EDGE};
use std::io::Cursor;

pub(super) fn mime(encoded: &str) -> &'static str {
    if encoded.starts_with("/9j/") {
        "image/jpeg"
    } else {
        "image/png"
    }
}

pub(super) fn prepare(images: Vec<String>) -> Result<Vec<String>, String> {
    if images.is_empty() {
        return Ok(images);
    }
    if images.len() > 8 {
        return Err("Attach at most eight images to one question.".into());
    }
    // Leave room for text and JSON under the API's 4 MiB body limit.
    let budget = MAX_CHAT_IMAGE_BYTES.min(2 * 1024 * 1024 / images.len());
    images
        .into_iter()
        .map(|encoded| normalize(&encoded, budget, MAX_CHAT_IMAGE_EDGE))
        .collect()
}

/// Small persisted previews, with a shared budget for the whole message.
pub(crate) fn previews(images: &[String]) -> Result<Vec<String>, String> {
    if images.len() > 8 {
        return Err("Attach at most eight images to one question.".into());
    }
    let budget = 64 * 1024 / images.len().max(1);
    images
        .iter()
        .map(|encoded| normalize(encoded, budget, 224))
        .collect()
}

fn normalize(encoded: &str, budget: usize, edge: u32) -> Result<String, String> {
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "Could not read the attached image. Capture or attach it again.".to_owned())?;
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut image = reader
        .decode()
        .map_err(|e| format!("Could not read the attached image: {e}"))?
        .thumbnail(edge, edge);
    loop {
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        if png.get_ref().len() <= budget {
            return Ok(STANDARD.encode(png.into_inner()));
        }
        let edge = image.width().max(image.height());
        if edge <= 16 {
            return Err("This image could not fit the managed request limit. Try a smaller screenshot region.".into());
        }
        let edge = (edge * 3 / 4).max(16);
        image = DynamicImage::ImageRgba8(image.thumbnail(edge, edge).to_rgba8());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn encode(image: &DynamicImage, format: ImageFormat) -> String {
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        STANDARD.encode(bytes.into_inner())
    }

    #[test]
    fn full_hd_jpeg_screenshot_becomes_a_bounded_real_png() {
        let source = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            1920,
            1080,
            image::Rgb([24, 48, 96]),
        ));
        let prepared = prepare(vec![encode(&source, ImageFormat::Jpeg)]).unwrap();
        let bytes = STANDARD.decode(&prepared[0]).unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Png);
        assert!(bytes.len() <= MAX_CHAT_IMAGE_BYTES);
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!(image.width(), MAX_CHAT_IMAGE_EDGE);
        assert!(image.height() < image.width());
    }

    #[test]
    fn saved_previews_bound_the_combined_message_size() {
        let source = DynamicImage::ImageRgb8(image::RgbImage::from_fn(480, 360, |x, y| {
            image::Rgb([(x * y) as u8, x as u8, y as u8])
        }));
        let images = vec![encode(&source, ImageFormat::Jpeg); 8];
        let previews = previews(&images).unwrap();
        let mut total = 0;
        for encoded in previews {
            let bytes = STANDARD.decode(encoded).unwrap();
            total += bytes.len();
            let image = image::load_from_memory(&bytes).unwrap();
            assert!(image.width() <= 224 && image.height() <= 224);
        }
        assert!(total <= 64 * 1024);
    }

    #[test]
    fn high_entropy_png_is_reduced_to_the_byte_limit() {
        let mut seed = 42u32;
        let source = DynamicImage::ImageRgb8(image::RgbImage::from_fn(1024, 1024, |_, _| {
            let mut rgb = [0; 3];
            for channel in &mut rgb {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *channel = (seed >> 24) as u8;
            }
            image::Rgb(rgb)
        }));
        let encoded = encode(&source, ImageFormat::Png);
        assert!(STANDARD.decode(&encoded).unwrap().len() > MAX_CHAT_IMAGE_BYTES);
        let prepared = prepare(vec![encoded]).unwrap();
        assert!(STANDARD.decode(&prepared[0]).unwrap().len() <= MAX_CHAT_IMAGE_BYTES);
    }
}
