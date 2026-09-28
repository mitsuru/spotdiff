pub mod worker;
use crate::{
    app::{self, App, Mode, Viewport},
    diff::{self, Comparison, Side},
    source::SourcePair,
};
use anyhow::ensure;
use image::{DynamicImage, RgbaImage};
use ratatui::{
    Frame,
    layout::{Margin, Size},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};
use ratatui_image::{
    Image,
    protocol::{Protocol, kitty::Kitty},
};
#[derive(Clone)]
pub struct RenderRequest {
    pub generation: u64,
    pub mode: Mode,
    pub viewport: Viewport,
    pub cell_pixels: (u16, u16),
    pub image_ids: Vec<u32>,
}
pub struct PreparedFrame {
    pub generation: u64,
    pub before: Option<Protocol>,
    pub after: Option<Protocol>,
    pub highlight: Option<Protocol>,
    pub image_ids: Vec<u32>,
}

pub fn rasterize_viewport(c: &Comparison, side: Side, v: &Viewport) -> RgbaImage {
    RgbaImage::from_fn(v.width, v.height, |px, py| {
        let x = (v.x + f64::from(px) / v.zoom).floor() as u32;
        let y = (v.y + f64::from(py) / v.zoom).floor() as u32;
        diff::pixel(c, side, x, y)
    })
}
pub fn prepare(c: &Comparison, r: &RenderRequest) -> anyhow::Result<PreparedFrame> {
    ensure!(
        r.cell_pixels.0 > 0 && r.cell_pixels.1 > 0,
        "端末の文字セル寸法が不正です"
    );
    ensure!(
        r.viewport.zoom.is_finite() && r.viewport.zoom > 0.0,
        "画像の倍率が不正です"
    );
    diff::validate_rgba_size(r.viewport.width, r.viewport.height)?;
    let required = if r.mode == Mode::SideBySide { 2 } else { 1 };
    ensure!(r.image_ids.len() == required, "画像IDが不足しています");
    let size = Size::new(
        (r.viewport.width / u32::from(r.cell_pixels.0)).try_into()?,
        (r.viewport.height / u32::from(r.cell_pixels.1)).try_into()?,
    );
    ensure!(
        size.width <= app::MAX_IMAGE_CELLS && size.height <= app::MAX_IMAGE_CELLS,
        "Kittyの描画可能なセル数を超えています"
    );
    let make = |side: Side, id: u32| -> anyhow::Result<Protocol> {
        let image = DynamicImage::ImageRgba8(rasterize_viewport(c, side, &r.viewport));
        Ok(Protocol::Kitty(Kitty::new(image, size, id, false, false)?))
    };
    let mut p = PreparedFrame {
        generation: r.generation,
        before: None,
        after: None,
        highlight: None,
        image_ids: r.image_ids.clone(),
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
                .unwrap_or_else(|| "画像なし".into())
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
            "左右比較"
        } else {
            "差分強調"
        };
        let ratio = if c.compared == 0 {
            0.0
        } else {
            c.changed as f64 / c.compared as f64 * 100.0
        };
        let footer = format!(
            "{mode}  {:.1}%  変更: {} / {} px ({ratio:.2}%)\nTab:表示 +/−:ズーム hjkl/矢印:移動 f:fit 1:等倍 q:終了",
            a.viewport().zoom * 100.0,
            c.changed,
            c.compared
        );
        f.render_widget(Paragraph::new(footer), l.footer);
        if l.too_small {
            f.render_widget(
                Paragraph::new("ウィンドウを広げてください（qで終了）"),
                l.highlight,
            );
            return;
        }
        let p = p.filter(|p| is_current(a.generation(), p.generation));
        let mut image = |area: ratatui::layout::Rect,
                         title: &str,
                         protocol: Option<&Protocol>,
                         missing: Option<&str>| {
            f.render_widget(Block::default().borders(Borders::ALL).title(title), area);
            let inner = area.inner(Margin::new(1, 1));
            if let Some(text) = missing {
                f.render_widget(Paragraph::new(text), inner);
            } else if let Some(protocol) = protocol {
                f.render_widget(Image::new(protocol).allow_clipping(true), inner);
            } else {
                f.render_widget(Paragraph::new("描画を更新中…"), inner);
            }
        };
        match a.mode() {
            Mode::SideBySide => {
                image(
                    l.before,
                    "変更前",
                    p.and_then(|p| p.before.as_ref()),
                    c.before.is_none().then_some("追加前の画像なし"),
                );
                image(
                    l.after,
                    "変更後",
                    p.and_then(|p| p.after.as_ref()),
                    c.after.is_none().then_some("削除後の画像なし"),
                );
            }
            Mode::Highlight => image(
                l.highlight,
                "変更箇所（マゼンタ）",
                p.and_then(|p| p.highlight.as_ref()),
                None,
            ),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::compare;
    use crate::source::InputSide;
    use image::Rgba;
    use ratatui::{
        Terminal,
        backend::{CrosstermBackend, TestBackend},
        layout::Rect,
    };
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
    fn delayed_success_and_error_do_not_replace_latest_frame() {
        let frame = |generation| PreparedFrame {
            generation,
            before: None,
            after: None,
            highlight: None,
            image_ids: vec![42],
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
        assert!(text.contains("追加前の画像なし"));
        assert!(text.contains("日本語.png"));
    }
    #[test]
    fn kitty_image_ids_and_chunks() {
        let c = comparison();
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
            image_ids: vec![4242],
        };
        let mut p = prepare(&c, &r).unwrap();
        let protocol = p.highlight.take().unwrap();
        let mut out = Vec::new();
        {
            let mut backend = CrosstermBackend::new(&mut out);
            use ratatui::backend::Backend;
            let mut buf = ratatui::buffer::Buffer::empty(Rect::new(0, 0, 10, 5));
            use ratatui::widgets::Widget;
            ratatui_image::Image::new(&protocol).render(buf.area, &mut buf);
            backend
                .draw(
                    buf.content
                        .iter()
                        .enumerate()
                        .map(|(i, c)| ((i % 10) as u16, (i / 10) as u16, c)),
                )
                .unwrap();
        }
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("i=4242"));
        assert!(text.contains("m=1"));
        assert!(text.contains("m=0"));
    }
}
