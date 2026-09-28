use crate::diff::Comparison;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Margin, Rect};
use std::sync::Arc;
// Bound the rasterized viewport independently of the terminal window size.
pub const MAX_IMAGE_CELLS: u16 = 297;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    SideBySide,
    Highlight,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub zoom: f64,
    pub x: f64,
    pub y: f64,
    pub width: u32,
    pub height: u32,
}
pub struct Layout {
    pub header: Rect,
    pub before: Rect,
    pub after: Rect,
    pub highlight: Rect,
    pub footer: Rect,
    pub too_small: bool,
}
pub struct App {
    comparison: Arc<Comparison>,
    mode: Mode,
    viewport: Viewport,
    fit: bool,
    area: Rect,
    cell: (u16, u16),
    generation: u64,
}
impl App {
    pub fn new(comparison: Arc<Comparison>) -> Self {
        Self {
            comparison,
            mode: Mode::SideBySide,
            viewport: Viewport {
                zoom: 1.0,
                x: 0.0,
                y: 0.0,
                width: 0,
                height: 0,
            },
            fit: true,
            area: Rect::default(),
            cell: (1, 1),
            generation: 0,
        }
    }
    pub fn resize(&mut self, area: Rect, cell_pixels: (u16, u16)) {
        let old = (self.area, self.cell, self.viewport);
        self.area = area;
        self.cell = cell_pixels;
        self.update_dimensions();
        if old != (self.area, self.cell, self.viewport) {
            self.generation += 1;
        }
    }
    fn update_dimensions(&mut self) {
        let l = layout(self.area, self.mode);
        let size = if l.too_small {
            Rect::default()
        } else {
            match self.mode {
                Mode::SideBySide => {
                    let a = l.before.inner(Margin::new(1, 1));
                    let b = l.after.inner(Margin::new(1, 1));
                    Rect::new(0, 0, a.width.min(b.width), a.height.min(b.height))
                }
                Mode::Highlight => l.highlight.inner(Margin::new(1, 1)),
            }
        };
        self.viewport.width = u32::from(size.width.min(MAX_IMAGE_CELLS)) * u32::from(self.cell.0);
        self.viewport.height = u32::from(size.height.min(MAX_IMAGE_CELLS)) * u32::from(self.cell.1);
        if self.fit && self.viewport.width > 0 && self.viewport.height > 0 {
            self.viewport.zoom = (f64::from(self.viewport.width)
                / f64::from(self.comparison.width))
            .min(f64::from(self.viewport.height) / f64::from(self.comparison.height))
            .min(1.0);
            self.viewport.x = 0.0;
            self.viewport.y = 0.0;
        }
        self.clamp();
    }
    fn clamp(&mut self) {
        let v = &mut self.viewport;
        v.x = v.x.clamp(
            0.0,
            (f64::from(self.comparison.width) - f64::from(v.width) / v.zoom).max(0.0),
        );
        v.y = v.y.clamp(
            0.0,
            (f64::from(self.comparison.height) - f64::from(v.height) / v.zoom).max(0.0),
        );
    }
    pub fn on_key(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }
        if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            return true;
        }
        let old = (self.mode, self.viewport);
        match key.code {
            KeyCode::Tab => {
                self.mode = match self.mode {
                    Mode::SideBySide => Mode::Highlight,
                    Mode::Highlight => Mode::SideBySide,
                };
                self.update_dimensions();
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.fit = false;
                self.viewport.zoom = (self.viewport.zoom * 2.0).clamp(1.0 / 16.0, 16.0);
            }
            KeyCode::Char('-') => {
                self.fit = false;
                self.viewport.zoom = (self.viewport.zoom / 2.0).clamp(1.0 / 16.0, 16.0);
            }
            KeyCode::Char('1') => {
                self.fit = false;
                self.viewport.zoom = 1.0;
            }
            KeyCode::Char('f') => {
                self.fit = true;
                self.update_dimensions();
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.viewport.x -= f64::from(self.cell.0) / self.viewport.zoom
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.viewport.x += f64::from(self.cell.0) / self.viewport.zoom
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.viewport.y -= f64::from(self.cell.1) / self.viewport.zoom
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.viewport.y += f64::from(self.cell.1) / self.viewport.zoom
            }
            _ => {}
        }
        self.clamp();
        if old != (self.mode, self.viewport) {
            self.generation += 1;
        }
        false
    }
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn comparison(&self) -> &Comparison {
        &self.comparison
    }
}
pub fn layout(area: Rect, _mode: Mode) -> Layout {
    let header_height = area.height.min(2);
    let footer_height = area.height.saturating_sub(header_height).min(2);
    let header = Rect::new(area.x, area.y, area.width, header_height);
    let footer = Rect::new(
        area.x,
        area.bottom().saturating_sub(footer_height),
        area.width,
        footer_height,
    );
    let main = Rect::new(
        area.x,
        area.y + header_height,
        area.width,
        area.height.saturating_sub(header_height + footer_height),
    );
    let half = main.width / 2;
    Layout {
        header,
        footer,
        before: Rect::new(main.x, main.y, half, main.height),
        after: Rect::new(main.x + half, main.y, main.width - half, main.height),
        highlight: main,
        too_small: area.width < 20 || area.height < 8,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::compare;
    use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
    use image::{Rgba, RgbaImage};
    fn app() -> App {
        App::new(Arc::new(
            compare(
                Some(RgbaImage::from_pixel(100, 100, Rgba([0, 0, 0, 255]))),
                None,
            )
            .unwrap(),
        ))
    }
    fn key(a: &mut App, k: KeyCode) -> bool {
        a.on_key(KeyEvent::new(k, KeyModifiers::NONE))
    }
    #[test]
    fn fit_uses_common_scale() {
        let mut a = app();
        a.resize(Rect::new(0, 0, 24, 16), (10, 10));
        assert_eq!(a.mode(), Mode::SideBySide);
        assert_eq!(a.viewport().zoom, 1.0);
        assert_eq!((a.viewport().width, a.viewport().height), (100, 100));
        a.resize(Rect::new(0, 0, 24, 16), (5, 5));
        assert_eq!(a.viewport().zoom, 0.5);
    }
    #[test]
    fn resize_updates_placements_even_when_viewport_pixels_do_not_change() {
        let mut a = app();
        a.resize(Rect::new(0, 0, 24, 16), (10, 10));
        let generation = a.generation();
        let viewport = a.viewport();
        // The narrower pane has the same size, but the right border moves.
        a.resize(Rect::new(0, 0, 25, 16), (10, 10));
        assert_eq!(a.viewport(), viewport);
        assert!(a.generation() > generation);
    }
    #[test]
    fn zoom_is_bounded() {
        let mut a = app();
        a.resize(Rect::new(0, 0, 24, 16), (10, 10));
        for _ in 0..20 {
            key(&mut a, KeyCode::Char('+'));
        }
        assert_eq!(a.viewport().zoom, 16.0);
        for _ in 0..30 {
            key(&mut a, KeyCode::Char('-'));
        }
        assert_eq!(a.viewport().zoom, 1.0 / 16.0);
        key(&mut a, KeyCode::Char('1'));
        assert_eq!(a.viewport().zoom, 1.0);
    }
    #[test]
    fn pan_clamps_to_canvas() {
        let mut a = app();
        a.resize(Rect::new(0, 0, 20, 8), (1, 1));
        key(&mut a, KeyCode::Char('1'));
        for _ in 0..200 {
            key(&mut a, KeyCode::Right);
            key(&mut a, KeyCode::Down);
        }
        assert_eq!((a.viewport().x, a.viewport().y), (92.0, 98.0));
        for _ in 0..200 {
            key(&mut a, KeyCode::Char('h'));
            key(&mut a, KeyCode::Char('k'));
        }
        assert_eq!((a.viewport().x, a.viewport().y), (0.0, 0.0));
    }
    #[test]
    fn resize_preserves_manual_zoom() {
        let mut a = app();
        a.resize(Rect::new(0, 0, 24, 16), (10, 10));
        key(&mut a, KeyCode::Char('+'));
        a.resize(Rect::new(0, 0, 20, 8), (10, 10));
        assert_eq!(a.viewport().zoom, 2.0);
        key(&mut a, KeyCode::Char('f'));
        assert_eq!(a.viewport().zoom, 0.2);
    }
    #[test]
    fn mode_and_quit_keys() {
        let mut a = app();
        a.resize(Rect::new(0, 0, 24, 16), (10, 10));
        key(&mut a, KeyCode::Tab);
        assert_eq!(a.mode(), Mode::Highlight);
        for k in [KeyCode::Char('q'), KeyCode::Esc] {
            assert!(key(&mut a, k));
        }
        assert!(a.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        assert!(!a.on_key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release
        )));
    }
    #[test]
    fn tiny_layout_has_no_images() {
        assert!(layout(Rect::new(0, 0, 1, 1), Mode::SideBySide).too_small);
        let mut a = app();
        a.resize(Rect::new(0, 0, 1, 1), (10, 20));
        assert_eq!((a.viewport().width, a.viewport().height), (0, 0));
    }
    #[test]
    fn wide_and_tall_terminal_fits_and_pans_within_kitty_limit() {
        let mut a = App::new(Arc::new(
            compare(
                None,
                Some(RgbaImage::from_pixel(1000, 1000, Rgba([0, 0, 0, 255]))),
            )
            .unwrap(),
        ));
        a.resize(Rect::new(0, 0, 800, 500), (1, 1));
        key(&mut a, KeyCode::Tab);
        assert_eq!((a.viewport().width, a.viewport().height), (297, 297));
        assert_eq!(a.viewport().zoom, 0.297);
        key(&mut a, KeyCode::Char('1'));
        for _ in 0..1000 {
            key(&mut a, KeyCode::Right);
            key(&mut a, KeyCode::Down);
        }
        assert_eq!((a.viewport().x, a.viewport().y), (703.0, 703.0));
    }
}
