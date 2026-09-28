use anyhow::ensure;
use crossterm::{cursor::MoveTo, queue};
use image::RgbaImage;
use ratatui::layout::{Rect, Size};
use std::{borrow::Cow, fmt::Write as _, io::Write};

/// A normal Kitty placement, without Unicode placeholders or virtual placements.
pub struct KittyImage {
    size: Size,
    transmission: String,
    sent: bool,
}
impl KittyImage {
    pub fn new(image: RgbaImage, size: Size, id: u32, compress: bool) -> anyhow::Result<Self> {
        ensure!(
            size.width > 0 && size.height > 0,
            "Image display area is empty"
        );
        ensure!(id != 0, "Invalid image ID");
        let bytes = if compress {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            encoder.write_all(image.as_raw())?;
            Cow::Owned(encoder.finish()?)
        } else {
            Cow::Borrowed(image.as_raw().as_slice())
        };
        // Each APC payload is at most 4096 base64 characters.
        let chunks = bytes.chunks(3072);
        let count = chunks.len();
        let mut transmission = String::with_capacity(count * (4096 + 32) + 128);
        for (index, chunk) in chunks.enumerate() {
            transmission.push_str("\x1b_Gq=2,");
            if index == 0 {
                let compression = if compress { "o=z," } else { "" };
                write!(
                    transmission,
                    "i={id},a=T,f=32,{compression}t=d,s={},v={},C=1,p=1,",
                    image.width(),
                    image.height(),
                )?;
            }
            write!(transmission, "m={};", u8::from(index + 1 < count))?;
            base64_simd::STANDARD.encode_append(chunk, &mut transmission);
            transmission.push_str("\x1b\\");
        }
        Ok(Self {
            size,
            transmission,
            sent: false,
        })
    }

    pub fn write(&mut self, out: &mut impl Write, area: Rect) -> anyhow::Result<()> {
        if self.sent {
            return Ok(());
        }
        ensure!(
            area.width >= self.size.width && area.height >= self.size.height,
            "Image exceeds the display area"
        );
        queue!(out, MoveTo(area.x, area.y))?;
        out.write_all(self.transmission.as_bytes())?;
        self.sent = true;
        Ok(())
    }
}
