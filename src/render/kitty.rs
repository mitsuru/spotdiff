use anyhow::ensure;
use crossterm::{cursor::MoveTo, queue};
use image::RgbaImage;
use ratatui::layout::{Rect, Size};
use std::{borrow::Cow, fmt::Write as _, io::Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
/// A normal Kitty placement, without Unicode placeholders or virtual placements.
pub struct KittyImage {
    size: Size,
    pixels: (u32, u32),
    id: u32,
    region: ImageRegion,
    initial_region: ImageRegion,
    transmission: String,
    sent: bool,
    placement: Option<(Rect, ImageRegion)>,
}
impl KittyImage {
    pub fn new(
        image: RgbaImage,
        size: Size,
        id: u32,
        compress: bool,
        region: ImageRegion,
    ) -> anyhow::Result<Self> {
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
                    "i={id},a=T,f=32,{compression}t=d,s={},v={},x={},y={},w={},h={},C=1,p=1,",
                    image.width(),
                    image.height(),
                    region.x,
                    region.y,
                    region.width,
                    region.height,
                )?;
            }
            write!(transmission, "m={};", u8::from(index + 1 < count))?;
            base64_simd::STANDARD.encode_append(chunk, &mut transmission);
            transmission.push_str("\x1b\\");
        }
        Ok(Self {
            size,
            pixels: image.dimensions(),
            id,
            region,
            initial_region: region,
            transmission,
            sent: false,
            placement: None,
        })
    }

    pub fn can_reuse(&self, region: ImageRegion) -> bool {
        region.width > 0
            && region.height > 0
            && u64::from(region.x) + u64::from(region.width) <= u64::from(self.pixels.0)
            && u64::from(region.y) + u64::from(region.height) <= u64::from(self.pixels.1)
    }

    pub fn set_region(&mut self, region: ImageRegion) {
        self.region = region;
    }

    pub fn write(&mut self, out: &mut impl Write, area: Rect) -> anyhow::Result<()> {
        if self.placement == Some((area, self.region)) {
            return Ok(());
        }
        ensure!(
            area.width >= self.size.width && area.height >= self.size.height,
            "Image exceeds the display area"
        );
        queue!(out, MoveTo(area.x, area.y))?;
        if self.sent {
            write!(
                out,
                "\x1b_Gq=2,a=p,i={},p=1,x={},y={},w={},h={},C=1;\x1b\\",
                self.id, self.region.x, self.region.y, self.region.width, self.region.height,
            )?;
        } else {
            if self.region == self.initial_region {
                out.write_all(self.transmission.as_bytes())?;
            } else {
                // A completed background frame can cover newer pan input. Only
                // rewrite its initial placement header; the encoded pixels stay valid.
                let end = self.transmission.find(';').expect("Kitty header delimiter");
                let fields = |region: ImageRegion| {
                    format!(
                        "x={},y={},w={},h={}",
                        region.x, region.y, region.width, region.height
                    )
                };
                let header = self.transmission[..end]
                    .replace(&fields(self.initial_region), &fields(self.region));
                out.write_all(header.as_bytes())?;
                out.write_all(&self.transmission.as_bytes()[end..])?;
            }
            self.transmission = String::new();
            self.sent = true;
        }
        self.placement = Some((area, self.region));
        Ok(())
    }
}
