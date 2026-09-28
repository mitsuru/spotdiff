use anyhow::{Context, ensure};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, Rgba, RgbaImage};
use std::{io::Cursor, sync::Arc};

const MAX_RGBA_BYTES: u64 = 256 * 1024 * 1024;
#[derive(Debug)]
pub struct Comparison {
    pub before: Option<Arc<RgbaImage>>,
    pub after: Option<Arc<RgbaImage>>,
    pub width: u32,
    pub height: u32,
    pub changed_mask: Vec<bool>,
    pub changed: u64,
    pub compared: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
    Highlight,
}

pub fn validate_rgba_size(width: u32, height: u32) -> anyhow::Result<()> {
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|n| n.checked_mul(4));
    ensure!(
        width > 0 && height > 0 && bytes.is_some_and(|n| n <= MAX_RGBA_BYTES),
        "画像のRGBAサイズが256 MiBの制限を超えています（または寸法が不正です）"
    );
    Ok(())
}
pub fn decode(bytes: &[u8], label: &str) -> anyhow::Result<RgbaImage> {
    let decode_inner = || -> anyhow::Result<RgbaImage> {
        ensure!(
            !bytes.starts_with(b"version https://git-lfs.github.com/spec/v1"),
            "Git LFSポインタの画像展開には対応していません"
        );
        let format = image::guess_format(bytes)
            .context("画像形式を判定できません。PNG・JPEG・WebPに対応しています")?;
        ensure!(
            matches!(
                format,
                ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
            ),
            "PNG・JPEG・WebP以外の形式は未対応です"
        );
        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = Limits::default();
        limits.max_alloc = Some(MAX_RGBA_BYTES);
        reader.limits(limits);
        let decoder = reader.into_decoder()?;
        let (w, h) = decoder.dimensions();
        validate_rgba_size(w, h)?;
        Ok(DynamicImage::from_decoder(decoder)?.into_rgba8())
    };
    decode_inner().with_context(|| format!("画像をデコードできません: {label}"))
}
pub fn compare(before: Option<RgbaImage>, after: Option<RgbaImage>) -> anyhow::Result<Comparison> {
    ensure!(
        before.is_some() || after.is_some(),
        "比較する画像がありません"
    );
    let width = before
        .iter()
        .chain(after.iter())
        .map(RgbaImage::width)
        .max()
        .unwrap_or(0);
    let height = before
        .iter()
        .chain(after.iter())
        .map(RgbaImage::height)
        .max()
        .unwrap_or(0);
    validate_rgba_size(width, height)?;
    let mut changed_mask = vec![false; width as usize * height as usize];
    let mut changed = 0;
    let mut compared = 0;
    for y in 0..height {
        for x in 0..width {
            let a = before.as_ref().and_then(|i| i.get_pixel_checked(x, y));
            let b = after.as_ref().and_then(|i| i.get_pixel_checked(x, y));
            if a.is_none() && b.is_none() {
                continue;
            }
            compared += 1;
            let differs = match (a, b) {
                (Some(a), Some(b)) => a != b && !(a[3] == 0 && b[3] == 0),
                _ => true,
            };
            if differs {
                changed += 1;
                changed_mask[(y * width + x) as usize] = true;
            }
        }
    }
    Ok(Comparison {
        before: before.map(Arc::new),
        after: after.map(Arc::new),
        width,
        height,
        changed_mask,
        changed,
        compared,
    })
}
pub fn pixel(c: &Comparison, side: Side, x: u32, y: u32) -> Rgba<u8> {
    let gray = if ((x / 8) + (y / 8)).is_multiple_of(2) {
        48
    } else {
        80
    };
    let mut base = [gray; 3];
    let before = c.before.as_ref().and_then(|i| i.get_pixel_checked(x, y));
    let after = c.after.as_ref().and_then(|i| i.get_pixel_checked(x, y));
    let source = match side {
        Side::Before => before,
        Side::After => after,
        Side::Highlight => after.or(before),
    };
    if let Some(p) = source {
        let a = u32::from(p[3]);
        for k in 0..3 {
            base[k] = ((u32::from(p[k]) * a + u32::from(gray) * (255 - a)) / 255) as u8;
        }
    }
    if side == Side::Highlight {
        let changed = x < c.width && y < c.height && c.changed_mask[(y * c.width + x) as usize];
        for (k, channel) in base.iter_mut().enumerate() {
            *channel = if changed {
                ((u16::from(*channel) + if k == 1 { 0 } else { 255 }) / 2) as u8
            } else {
                *channel / 2
            };
        }
    }
    Rgba([base[0], base[1], base[2], 255])
}
#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat};
    use std::io::Cursor;
    fn img(p: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(1, 1, Rgba(p))
    }
    #[test]
    fn rgba_diff_rules() {
        for (a, b, want) in [
            ([1, 2, 3, 255], [1, 2, 3, 255], 0),
            ([1, 2, 3, 255], [2, 2, 3, 255], 1),
            ([1, 2, 3, 255], [1, 2, 3, 254], 1),
            ([1, 2, 3, 0], [9, 8, 7, 0], 0),
        ] {
            assert_eq!(compare(Some(img(a)), Some(img(b))).unwrap().changed, want);
        }
    }
    #[test]
    fn unequal_dimensions_use_union() {
        let c = compare(
            Some(RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 255]))),
            Some(RgbaImage::from_pixel(1, 2, Rgba([0, 0, 0, 255]))),
        )
        .unwrap();
        assert_eq!((c.width, c.height, c.changed, c.compared), (2, 2, 2, 3));
        assert!(!c.changed_mask[3]);
    }
    #[test]
    fn missing_side_is_change() {
        for (a, b) in [
            (Some(img([0, 0, 0, 0])), None),
            (None, Some(img([0, 0, 0, 0]))),
        ] {
            let c = compare(a, b).unwrap();
            assert_eq!((c.changed, c.compared), (1, 1));
        }
        assert!(compare(None, None).is_err());
    }
    #[test]
    fn decode_supported_formats() {
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            let mut b = Cursor::new(Vec::new());
            DynamicImage::ImageRgba8(img([1, 2, 3, 255]))
                .to_rgb8()
                .write_to(&mut b, format)
                .unwrap();
            assert_eq!(decode(b.get_ref(), "fixture").unwrap().dimensions(), (1, 1));
        }
    }
    #[test]
    fn unsupported_and_corrupt_explain_error() {
        for b in [
            b"<svg/>".as_slice(),
            b"text",
            b"version https://git-lfs.github.com/spec/v1\n",
            b"\x89PNG\r\n\x1a\n",
        ] {
            let e = decode(b, "bad.png").unwrap_err();
            assert!(format!("{e:#}").contains("bad.png"));
        }
    }
    #[test]
    fn transparent_highlight_is_visible() {
        let c = compare(None, Some(img([0, 0, 0, 0]))).unwrap();
        assert_eq!(pixel(&c, Side::Highlight, 0, 0), Rgba([151, 24, 151, 255]));
        let c = compare(Some(img([255, 0, 0, 255])), None).unwrap();
        assert_eq!(pixel(&c, Side::Highlight, 0, 0), Rgba([255, 0, 127, 255]));
    }
    #[test]
    fn size_limit_precedes_allocation() {
        assert!(validate_rgba_size(8192, 8192).is_ok());
        assert!(validate_rgba_size(8193, 8192).is_err());
        assert!(validate_rgba_size(u32::MAX, u32::MAX).is_err());
        assert!(validate_rgba_size(0, 1).is_err());
    }
}
