mod kitty;
pub mod worker;
use crate::{
    app::{self, App, Mode, Viewport},
    diff::{self, Comparison, Side},
    source::SourcePair,
};
use anyhow::ensure;
use image::RgbaImage;
use kitty::{ImageRegion, KittyImage};
use ratatui::{
    Frame,
    layout::{Margin, Size},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};
#[derive(Clone)]
pub struct RenderRequest {
    pub generation: u64,
    pub mode: Mode,
    pub viewport: Viewport,
    pub cell_pixels: (u16, u16),
    pub compress: bool,
    pub image_ids: Vec<u32>,
}
pub struct PreparedFrame {
    pub generation: u64,
    pub before: Option<KittyImage>,
    pub after: Option<KittyImage>,
    pub highlight: Option<KittyImage>,
    pub image_ids: Vec<u32>,
    view: Option<CachedView>,
}
struct CachedView {
    mode: Mode,
    viewport: Viewport,
    raster: Viewport,
    cell_pixels: (u16, u16),
}

fn visible_viewport(c: &Comparison, viewport: Viewport) -> Viewport {
    Viewport {
        width: viewport
            .width
            .min(((f64::from(c.width) - viewport.x).max(0.0) * viewport.zoom).ceil() as u32),
        height: viewport
            .height
            .min(((f64::from(c.height) - viewport.y).max(0.0) * viewport.zoom).ceil() as u32),
        ..viewport
    }
}

fn buffered_viewport(c: &Comparison, r: &RenderRequest) -> (Viewport, ImageRegion) {
    let v = r.viewport;
    let visible = visible_viewport(c, v);
    // Keep up to eight cells around the viewport so ordinary pans only update
    // the placement's source rectangle. Preserve the raster's sampling phase.
    let pad_x = (u32::from(r.cell_pixels.0) * 8).min(v.width / 2);
    let pad_y = (u32::from(r.cell_pixels.1) * 8).min(v.height / 2);
    let mut left = ((v.x * v.zoom).floor().max(0.0) as u32).min(pad_x);
    let mut top = ((v.y * v.zoom).floor().max(0.0) as u32).min(pad_y);
    let mut raster = visible_viewport(
        c,
        Viewport {
            x: (v.x - f64::from(left) / v.zoom).max(0.0),
            y: (v.y - f64::from(top) / v.zoom).max(0.0),
            width: v.width.saturating_add(left).saturating_add(pad_x),
            height: v.height.saturating_add(top).saturating_add(pad_y),
            ..v
        },
    );
    // Overscan must not turn a valid viewport into an oversized allocation.
    // Limit buffered textures to 64 MiB each; larger viewports use no buffer.
    if u64::from(raster.width) * u64::from(raster.height) * 4 > 64 * 1024 * 1024 {
        raster = visible;
        left = 0;
        top = 0;
    }
    (
        raster,
        ImageRegion {
            x: left,
            y: top,
            width: visible.width,
            height: visible.height,
        },
    )
}

impl PreparedFrame {
    pub fn pan_to(
        &mut self,
        c: &Comparison,
        generation: u64,
        mode: Mode,
        viewport: Viewport,
        cell_pixels: (u16, u16),
    ) -> bool {
        let Some(view) = self.view.as_ref() else {
            return false;
        };
        if mode != view.mode
            || cell_pixels != view.cell_pixels
            || viewport.zoom != view.viewport.zoom
            || viewport.width != view.viewport.width
            || viewport.height != view.viewport.height
        {
            return false;
        }
        let offset = |position: f64, origin: f64| {
            let pixels = (position - origin) * viewport.zoom;
            (pixels.is_finite() && pixels >= 0.0 && (pixels - pixels.round()).abs() < 1e-6)
                .then_some(pixels.round() as u32)
        };
        let (Some(x), Some(y)) = (
            offset(viewport.x, view.raster.x),
            offset(viewport.y, view.raster.y),
        ) else {
            return false;
        };
        let visible = visible_viewport(c, viewport);
        let region = ImageRegion {
            x,
            y,
            width: visible.width,
            height: visible.height,
        };
        if !self
            .before
            .iter()
            .chain(self.after.iter())
            .chain(self.highlight.iter())
            .all(|image| image.can_reuse(region))
        {
            return false;
        }
        for image in self
            .before
            .iter_mut()
            .chain(self.after.iter_mut())
            .chain(self.highlight.iter_mut())
        {
            image.set_region(region);
        }
        self.generation = generation;
        true
    }
}

pub fn rasterize_viewport(c: &Comparison, side: Side, v: &Viewport) -> RgbaImage {
    let axis = |origin: f64, length: u32| {
        let pixels = origin * v.zoom;
        // Pan placements use integer texture offsets. Normalize the same
        // near-integer screen coordinates before sampling a fresh texture.
        let pixels = if (pixels - pixels.round()).abs() < 1e-6 {
            pixels.round()
        } else {
            pixels
        };
        (0..length)
            .map(|offset| {
                let source = (pixels + f64::from(offset)) / v.zoom;
                let nearest = source.round();
                let source =
                    if (source - nearest).abs() <= 32.0 * f64::EPSILON * source.abs().max(1.0) {
                        nearest
                    } else {
                        source
                    };
                source.floor() as u32
            })
            .collect::<Vec<_>>()
    };
    let xs = axis(v.x, v.width);
    let ys = axis(v.y, v.height);
    RgbaImage::from_fn(v.width, v.height, |px, py| {
        diff::pixel(c, side, xs[px as usize], ys[py as usize])
    })
}
pub fn prepare(c: &Comparison, r: &RenderRequest) -> anyhow::Result<PreparedFrame> {
    ensure!(
        r.cell_pixels.0 > 0 && r.cell_pixels.1 > 0,
        "Invalid terminal cell dimensions"
    );
    ensure!(
        r.viewport.zoom.is_finite() && r.viewport.zoom > 0.0,
        "Invalid image zoom"
    );
    diff::validate_rgba_size(r.viewport.width, r.viewport.height)?;
    let required = if r.mode == Mode::SideBySide { 2 } else { 1 };
    ensure!(r.image_ids.len() == required, "Not enough image IDs");
    let size = Size::new(
        (r.viewport.width / u32::from(r.cell_pixels.0)).try_into()?,
        (r.viewport.height / u32::from(r.cell_pixels.1)).try_into()?,
    );
    ensure!(
        size.width <= app::MAX_IMAGE_CELLS && size.height <= app::MAX_IMAGE_CELLS,
        "Display area exceeds the cell count limit"
    );
    let (raster, region) = buffered_viewport(c, r);
    diff::validate_rgba_size(raster.width, raster.height)?;
    let make = |side: Side, id: u32| -> anyhow::Result<KittyImage> {
        KittyImage::new(
            rasterize_viewport(c, side, &raster),
            size,
            id,
            r.compress,
            region,
        )
    };
    let mut p = PreparedFrame {
        generation: r.generation,
        before: None,
        after: None,
        highlight: None,
        image_ids: r.image_ids.clone(),
        view: Some(CachedView {
            mode: r.mode,
            viewport: r.viewport,
            raster,
            cell_pixels: r.cell_pixels,
        }),
    };
    match r.mode {
        Mode::SideBySide => {
            if c.before.is_some() {
                p.before = Some(make(Side::Before, r.image_ids[0])?);
            }
            if c.after.is_some() {
                p.after = Some(make(Side::After, r.image_ids[1])?);
            }
        }
        Mode::Highlight => p.highlight = Some(make(Side::Highlight, r.image_ids[0])?),
    }
    Ok(p)
}
pub fn sanitize_label(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}
pub fn is_current(generation: u64, result_generation: u64) -> bool {
    generation == result_generation
}
pub fn accept_result(
    generation: u64,
    result_generation: u64,
    result: anyhow::Result<PreparedFrame>,
) -> anyhow::Result<Option<PreparedFrame>> {
    if is_current(generation, result_generation) {
        result.map(Some)
    } else {
        Ok(None)
    }
}
pub struct Renderer;
impl Renderer {
    pub fn draw(f: &mut Frame, a: &App, p: Option<&PreparedFrame>, labels: &SourcePair) {
        let l = app::layout(f.area(), a.mode());
        let c = a.comparison();
        let dimensions = |side: &Option<std::sync::Arc<RgbaImage>>| {
            side.as_ref()
                .map(|i| format!("{}×{}", i.width(), i.height()))
                .unwrap_or_else(|| "No image".into())
        };
        let header = format!(
            "spotdiff  {} → {}\n{} ({}) → {} ({})",
            dimensions(&c.before),
            dimensions(&c.after),
            sanitize_label(&labels.before.label),
            dimensions(&c.before),
            sanitize_label(&labels.after.label),
            dimensions(&c.after)
        );
        f.render_widget(
            Paragraph::new(header).style(Style::default().fg(Color::Cyan)),
            l.header,
        );
        let mode = if a.mode() == Mode::SideBySide {
            "Side by side"
        } else {
            "Highlight"
        };
        let ratio = if c.compared == 0 {
            0.0
        } else {
            c.changed as f64 / c.compared as f64 * 100.0
        };
        let footer = format!(
            "{mode}  {:.1}%  Changed: {} / {} px ({ratio:.2}%)\nTab:mode +/-:zoom hjkl/arrows:pan f:fit 1:actual q:quit",
            a.viewport().zoom * 100.0,
            c.changed,
            c.compared
        );
        f.render_widget(Paragraph::new(footer), l.footer);
        if l.too_small {
            f.render_widget(
                Paragraph::new("Enlarge the window (q to quit)"),
                l.highlight,
            );
            return;
        }
        let p = p.filter(|p| is_current(a.generation(), p.generation));
        let mut image = |area: ratatui::layout::Rect,
                         title: &str,
                         protocol: Option<&KittyImage>,
                         missing: Option<&str>| {
            f.render_widget(Block::default().borders(Borders::ALL).title(title), area);
            let inner = area.inner(Margin::new(1, 1));
            if let Some(text) = missing {
                f.render_widget(Paragraph::new(text), inner);
            } else if protocol.is_none() {
                f.render_widget(Paragraph::new("Rendering..."), inner);
            }
        };
        match a.mode() {
            Mode::SideBySide => {
                image(
                    l.before,
                    "Before",
                    p.and_then(|p| p.before.as_ref()),
                    c.before.is_none().then_some("No image before addition"),
                );
                image(
                    l.after,
                    "After",
                    p.and_then(|p| p.after.as_ref()),
                    c.after.is_none().then_some("No image after deletion"),
                );
            }
            Mode::Highlight => image(
                l.highlight,
                "Changes (magenta)",
                p.and_then(|p| p.highlight.as_ref()),
                None,
            ),
        }
    }

    pub fn write_images(
        out: &mut impl std::io::Write,
        area: ratatui::layout::Rect,
        app: &App,
        prepared: &mut PreparedFrame,
    ) -> anyhow::Result<()> {
        if !is_current(app.generation(), prepared.generation) {
            return Ok(());
        }
        let l = app::layout(area, app.mode());
        if l.too_small {
            return Ok(());
        }
        let mut write = |image: &mut Option<KittyImage>, area: ratatui::layout::Rect| {
            if let Some(image) = image {
                image.write(out, area.inner(Margin::new(1, 1)))?;
            }
            Ok::<_, anyhow::Error>(())
        };
        match app.mode() {
            Mode::SideBySide => {
                write(&mut prepared.before, l.before)?;
                write(&mut prepared.after, l.after)?;
            }
            Mode::Highlight => write(&mut prepared.highlight, l.highlight)?,
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::compare;
    use crate::source::InputSide;
    use image::Rgba;
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};
    use std::sync::Arc;
    fn comparison() -> Comparison {
        let mut i = RgbaImage::from_pixel(3, 1, Rgba([255, 0, 0, 255]));
        i.put_pixel(2, 0, Rgba([0, 255, 0, 255]));
        compare(None, Some(i)).unwrap()
    }
    #[test]
    fn viewport_is_sampled_without_full_zoom_allocation() {
        let c = comparison();
        let v = Viewport {
            zoom: 1.0,
            x: 2.0,
            y: 0.0,
            width: 1,
            height: 1,
        };
        let out = rasterize_viewport(&c, Side::After, &v);
        assert_eq!(out.dimensions(), (1, 1));
        assert_eq!(*out.get_pixel(0, 0), Rgba([0, 255, 0, 255]));
        let out = rasterize_viewport(
            &c,
            Side::After,
            &Viewport {
                zoom: 16.0,
                x: 0.0,
                ..v
            },
        );
        assert_eq!(out.dimensions(), (1, 1));
    }
    #[test]
    fn fractional_zoom_pan_does_not_shift_source_pixel_boundaries() {
        let c = compare(
            None,
            Some(RgbaImage::from_fn(1428, 1, |x, _| {
                Rgba([x as u8, (x / 256) as u8, 0, 255])
            })),
        )
        .unwrap();
        let v = Viewport {
            zoom: 5.0 / 3.0,
            x: 0.0,
            y: 0.0,
            width: 1302,
            height: 1,
        };
        let cached = rasterize_viewport(&c, Side::After, &v);
        // Six 14px pan steps: pixel 431 is source column 309 exactly.
        // Adding source-space floats used to floor it to column 308.
        let moved = rasterize_viewport(
            &c,
            Side::After,
            &Viewport {
                x: 50.4,
                width: 1190,
                ..v
            },
        );
        assert_eq!(moved.get_pixel(431, 0), cached.get_pixel(515, 0));
        assert_eq!(*moved.get_pixel(431, 0), Rgba([53, 1, 0, 255]));
    }
    #[test]
    fn small_canvas_is_not_padded_to_the_whole_pane() {
        let c = comparison();
        let r = RenderRequest {
            generation: 1,
            mode: Mode::Highlight,
            viewport: Viewport {
                zoom: 2.0,
                x: 0.0,
                y: 0.0,
                width: 100,
                height: 100,
            },
            cell_pixels: (10, 20),
            compress: false,
            image_ids: vec![4242],
        };
        let mut p = prepare(&c, &r).unwrap();
        let mut out = Vec::new();
        p.highlight
            .as_mut()
            .unwrap()
            .write(&mut out, Rect::new(0, 0, 10, 5))
            .unwrap();
        let text = String::from_utf8(out).unwrap();
        // The comparison canvas is 3x1; 2x zoom needs only a 6x2 texture.
        assert!(
            text.contains("s=6,v=2,"),
            "pane-sized texture was transmitted"
        );
        assert!(
            !text.contains("c=10,r=5"),
            "cropped image was stretched to fill the pane"
        );
        let apc = text.split_once("\x1b_G").unwrap().1;
        let payload = apc
            .split_once(';')
            .unwrap()
            .1
            .strip_suffix("\x1b\\")
            .unwrap();
        let pixels = base64_simd::STANDARD
            .decode_to_vec(payload.as_bytes())
            .unwrap();
        let row = [
            [255, 0, 127, 255],
            [255, 0, 127, 255],
            [255, 0, 127, 255],
            [255, 0, 127, 255],
            [127, 127, 127, 255],
            [127, 127, 127, 255],
        ]
        .concat();
        assert_eq!(pixels, [row.clone(), row].concat());
    }
    #[test]
    fn cached_pan_matches_sampled_pixels_and_rejects_incompatible_views() {
        let c = compare(
            Some(RgbaImage::from_fn(256, 192, |x, y| {
                Rgba([x as u8, y as u8, (x ^ y) as u8, (x % 256) as u8])
            })),
            Some(RgbaImage::from_fn(256, 192, |x, y| {
                Rgba([y as u8, (x / 2) as u8, (x ^ y) as u8, 255])
            })),
        )
        .unwrap();
        for (mode, side) in [
            (Mode::SideBySide, Side::Before),
            (Mode::SideBySide, Side::After),
            (Mode::Highlight, Side::Highlight),
        ] {
            for zoom in [0.37, 0.5, 1.0, 2.0] {
                let r = RenderRequest {
                    generation: 1,
                    mode,
                    viewport: Viewport {
                        zoom,
                        x: 0.0,
                        y: 0.0,
                        width: 64,
                        height: 48,
                    },
                    cell_pixels: (8, 8),
                    compress: false,
                    image_ids: if mode == Mode::SideBySide {
                        vec![42, 43]
                    } else {
                        vec![42]
                    },
                };
                let mut frame = prepare(&c, &r).unwrap();
                let moved = Viewport {
                    x: 16.0 / zoom,
                    y: 8.0 / zoom,
                    ..r.viewport
                };
                // Rebase a prepared but not yet transmitted frame to newer input.
                assert!(frame.pan_to(&c, 2, mode, moved, r.cell_pixels));
                let image = match side {
                    Side::Before => frame.before.as_mut().unwrap(),
                    Side::After => frame.after.as_mut().unwrap(),
                    Side::Highlight => frame.highlight.as_mut().unwrap(),
                };
                let mut output = Vec::new();
                image.write(&mut output, Rect::new(1, 1, 8, 6)).unwrap();
                let text = String::from_utf8(output).unwrap();
                let mut payload = Vec::new();
                let mut dimensions = (0, 0);
                for apc in text.split("\x1b_G").skip(1) {
                    let (header, rest) = apc.split_once(';').unwrap();
                    if header.contains("a=T") {
                        let value = |key: &str| {
                            header
                                .split(',')
                                .find_map(|f| f.strip_prefix(key))
                                .unwrap()
                                .parse::<u32>()
                                .unwrap()
                        };
                        dimensions = (value("s="), value("v="));
                        assert!(header.contains("x=16,y=8,w=64,h=48,"));
                    }
                    let encoded = rest.split_once("\x1b\\").unwrap().0;
                    payload.extend(
                        base64_simd::STANDARD
                            .decode_to_vec(encoded.as_bytes())
                            .unwrap(),
                    );
                }
                let texture = RgbaImage::from_raw(dimensions.0, dimensions.1, payload).unwrap();
                let actual = image::imageops::crop_imm(&texture, 16, 8, 64, 48).to_image();
                assert_eq!(
                    actual,
                    rasterize_viewport(&c, side, &moved),
                    "sampling changed at zoom {zoom}"
                );
                // An uploaded image can move back without sending any pixel data.
                assert!(frame.pan_to(&c, 3, mode, r.viewport, r.cell_pixels));
                assert_eq!(frame.generation, 3);
                for incompatible in [
                    Viewport {
                        x: 200.0 / zoom,
                        ..r.viewport
                    },
                    Viewport {
                        zoom: zoom * 2.0,
                        ..r.viewport
                    },
                    Viewport {
                        width: 32,
                        ..r.viewport
                    },
                    Viewport {
                        x: 0.25 / zoom,
                        ..r.viewport
                    },
                ] {
                    assert!(!frame.pan_to(&c, 4, mode, incompatible, r.cell_pixels));
                    assert_eq!(frame.generation, 3);
                }
                assert!(!frame.pan_to(&c, 4, mode, r.viewport, (4, 8)));
            }
        }
    }
    #[test]
    fn delayed_success_and_error_do_not_replace_latest_frame() {
        let frame = |generation| PreparedFrame {
            generation,
            before: None,
            after: None,
            highlight: None,
            image_ids: vec![42],
            view: None,
        };
        assert!(accept_result(2, 1, Ok(frame(1))).unwrap().is_none());
        assert!(
            accept_result(2, 1, Err(anyhow::anyhow!("old failure")))
                .unwrap()
                .is_none()
        );
        let latest = accept_result(2, 2, Ok(frame(2))).unwrap().unwrap();
        assert_eq!(latest.generation, 2);
        assert_eq!(latest.image_ids, vec![42]);
        assert!(
            accept_result(2, 2, Err(anyhow::anyhow!("current failure")))
                .err()
                .unwrap()
                .to_string()
                .contains("current failure")
        );
    }
    #[test]
    fn unicode_and_control_labels() {
        assert_eq!(sanitize_label("a\x1b[31m\nb"), "a�[31m�b");
        assert_eq!(sanitize_label("日本語"), "日本語");
    }
    #[test]
    fn missing_side_labels() {
        let mut a = App::new(Arc::new(comparison()));
        let mut t = Terminal::new(TestBackend::new(60, 16)).unwrap();
        let labels = SourcePair {
            before: InputSide {
                label: "日本語.png".into(),
                bytes: None,
            },
            after: InputSide {
                label: "new.png".into(),
                bytes: Some(vec![]),
            },
        };
        a.resize(Rect::new(0, 0, 60, 16), (10, 20));
        t.draw(|f| Renderer::draw(f, &a, None, &labels)).unwrap();
        let text = t
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
            .replace(' ', "");
        assert!(text.contains("Noimagebeforeaddition"));
        assert!(text.contains("日本語.png"));
    }
    #[test]
    fn kitty_image_ids_and_chunks() {
        let c = compare(
            None,
            Some(RgbaImage::from_pixel(128, 128, Rgba([255, 0, 0, 255]))),
        )
        .unwrap();
        let r = RenderRequest {
            generation: 1,
            mode: Mode::Highlight,
            viewport: Viewport {
                zoom: 1.0,
                x: 0.0,
                y: 0.0,
                width: 100,
                height: 100,
            },
            cell_pixels: (10, 20),
            compress: false,
            image_ids: vec![4242],
        };
        let mut p = prepare(&c, &r).unwrap();
        let mut protocol = p.highlight.take().unwrap();
        let mut out = Vec::new();
        protocol.write(&mut out, Rect::new(0, 0, 10, 5)).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("i=4242"));
        assert!(text.contains("m=1"));
        assert!(text.contains("m=0"));
    }
}
