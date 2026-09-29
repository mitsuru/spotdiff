//! Render README frames without a terminal or display server.
//! See docs/images/README.md for PNG/GIF conversion commands.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use image::ImageFormat;
use ratatui::{
    Terminal,
    backend::TestBackend,
    layout::{Margin, Rect},
    style::Color,
};
use spotdiff::{
    app::{self, App, Mode},
    cli::Request,
    diff::{self, Side},
    render::{self, RenderRequest, Renderer},
    source::{self, SourcePair},
};
use std::{
    fmt::Write,
    fs,
    io::Cursor,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const CELL: (u16, u16) = (10, 20);
const BACKGROUND: &str = "#181e2a";
const FOREGROUND: &str = "#d8dee9";

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn frame(app: &mut App, labels: &SourcePair, output: &Path) -> anyhow::Result<()> {
    let columns = if app.mode() == Mode::SideBySide {
        132
    } else {
        80
    };
    let area = Rect::new(0, 0, columns, 26);
    app.resize(area, CELL);
    let prepared = render::prepare(
        app.comparison(),
        &RenderRequest {
            generation: app.generation(),
            mode: app.mode(),
            viewport: app.viewport(),
            cell_pixels: CELL,
            compress: false,
            image_ids: (1..=app.mode().image_count() as u32).collect(),
        },
    )?;
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height))?;
    terminal.draw(|f| Renderer::draw(f, app, Some(&prepared), labels))?;

    let width = u32::from(area.width) * u32::from(CELL.0) + 24;
    let height = u32::from(area.height) * u32::from(CELL.1) + 24;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n<rect width=\"100%\" height=\"100%\" fill=\"{BACKGROUND}\"/>\n<g transform=\"translate(12 12)\">\n"
    );
    // Use the actual Ratatui buffer for all text, titles, borders and statistics.
    svg.push_str("<g font-family=\"DejaVu Sans Mono\" font-size=\"16\">\n");
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &terminal.backend().buffer()[(x, y)];
            if cell.symbol() == " " {
                continue;
            }
            let color = match cell.fg {
                Color::Reset => FOREGROUND,
                Color::Cyan => "#8be9fd",
                other => anyhow::bail!("Add an SVG mapping for UI color {other:?}"),
            };
            writeln!(
                svg,
                "<text x=\"{}\" y=\"{}\" fill=\"{color}\" textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\">{}</text>",
                x * CELL.0,
                y * CELL.1 + 15,
                CELL.0,
                xml(cell.symbol()),
            )?;
        }
    }
    svg.push_str("</g>\n");
    // Kitty images are separate from the buffer. Composite the real rasterizer
    // output at the same cell positions instead of emulating terminal escapes.
    let layout = app::layout(area, app.mode());
    let images = match app.mode() {
        Mode::SideBySide => vec![(layout.before, Side::Before), (layout.after, Side::After)],
        Mode::Highlight => vec![(layout.highlight, Side::Highlight)],
        Mode::Blink => vec![(layout.highlight, app.blink_side())],
    };
    for (pane, side) in images {
        let viewport = app.viewport();
        let mut image = render::rasterize_viewport(app.comparison(), side, &viewport);
        // As in the renderer, leave the area outside the comparison canvas empty.
        let width = viewport
            .width
            .min((f64::from(app.comparison().width) * viewport.zoom).ceil() as u32);
        let height = viewport
            .height
            .min((f64::from(app.comparison().height) * viewport.zoom).ceil() as u32);
        let image = image::imageops::crop(&mut image, 0, 0, width, height).to_image();
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, ImageFormat::Png)?;
        let mut encoded = String::new();
        base64_simd::STANDARD.encode_append(png.get_ref(), &mut encoded);
        let inner = pane.inner(Margin::new(1, 1));
        writeln!(
            svg,
            "<image x=\"{}\" y=\"{}\" width=\"{width}\" height=\"{height}\" xlink:href=\"data:image/png;base64,{encoded}\"/>",
            inner.x * CELL.0,
            inner.y * CELL.1,
        )?;
    }
    svg.push_str("</g>\n</svg>\n");
    fs::write(output, svg)?;
    println!("{}", output.display());
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let labels = source::load(
        &Request::Files {
            before: "docs/images/before.png".into(),
            after: "docs/images/after.png".into(),
        },
        &std::env::current_dir()?,
    )?;
    let decode = |side: &source::InputSide| {
        diff::decode(side.bytes.as_deref().expect("file input"), &side.label)
    };
    let mut app = App::new(Arc::new(diff::compare(
        Some(decode(&labels.before)?),
        Some(decode(&labels.after)?),
    )?));
    let output = Path::new("target/readme-frames");
    fs::create_dir_all(output)?;
    let start = Instant::now();
    frame(&mut app, &labels, &output.join("side-by-side.svg"))?;
    app.on_key_at(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), start);
    frame(&mut app, &labels, &output.join("highlight.svg"))?;
    app.on_key_at(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), start);
    app.on_key_at(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE), start);
    frame(&mut app, &labels, &output.join("blink-before.svg"))?;
    app.tick(start + Duration::from_millis(500));
    frame(&mut app, &labels, &output.join("blink-after.svg"))?;
    Ok(())
}
