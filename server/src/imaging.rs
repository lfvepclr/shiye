//! 派生图：display（原文预览，长边 1600）与 thumb（列表缩略，长边 320）。

use image::imageops::FilterType;
use image::{ExtendedColorType, ImageEncoder};

use crate::error::{AppError, AppResult};

/// 解码 JPEG，产出 (disp, thumb, w, h)。先校验像素上限。
pub fn derive(
    jpeg: &[u8],
    display_edge: u32,
    thumb_edge: u32,
    max_pixels: u64,
) -> AppResult<(Vec<u8>, Vec<u8>, u32, u32)> {
    let img = image::load_from_memory(jpeg)
        .map_err(|e| AppError::BadRequest(format!("图片解码失败: {e}")))?;

    let (w, h) = (img.width(), img.height());
    if w as u64 * h as u64 > max_pixels {
        return Err(AppError::BadRequest(format!(
            "图片像素 {}x{} 超上限 {}",
            w, h, max_pixels
        )));
    }

    let disp = encode_resized(&img, display_edge, 82)?;
    let thumb = encode_resized(&img, thumb_edge, 75)?;
    Ok((disp, thumb, w, h))
}

fn encode_resized(img: &image::DynamicImage, edge: u32, quality: u8) -> AppResult<Vec<u8>> {
    let resized = if img.width() > edge || img.height() > edge {
        img.resize(edge, edge, FilterType::Lanczos3)
    } else {
        img.clone()
    };
    let rgb = resized.to_rgb8();
    let mut cursor = std::io::Cursor::new(Vec::new());
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, quality);
    enc.write_image(rgb.as_raw(), rgb.width(), rgb.height(), ExtendedColorType::Rgb8)
        .map_err(|e| AppError::internal(format!("JPEG 编码失败: {e}")))?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_jpeg() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(400, 300, image::Rgb([200, 200, 200]));
        let mut cursor = std::io::Cursor::new(Vec::new());
        let enc = image::codecs::jpeg::JpegEncoder::new(&mut cursor);
        enc.write_image(img.as_raw(), 400, 300, ExtendedColorType::Rgb8).unwrap();
        cursor.into_inner()
    }

    #[test]
    fn derive_produces_two_jpegs() {
        let (disp, thumb, w, h) = derive(&tiny_jpeg(), 1600, 320, 25_000_000).unwrap();
        assert_eq!((w, h), (400, 300));
        assert!(!disp.is_empty());
        assert!(!thumb.is_empty());
        assert!(thumb.len() < disp.len());
    }

    #[test]
    fn derive_respects_pixel_limit() {
        let jpeg = tiny_jpeg();
        let err = derive(&jpeg, 1600, 320, 100).unwrap_err();
        assert!(err.to_string().contains("超上限"));
    }
}
