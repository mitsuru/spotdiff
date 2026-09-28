pub mod app;
pub mod cli;
pub mod diff;
pub mod render;
pub mod source;
pub mod terminal;

pub fn run(request: cli::Request) -> anyhow::Result<()> {
    use crossterm::event::{self, Event};
    use ratatui::{Terminal, backend::CrosstermBackend};
    use render::{PreparedFrame, RenderRequest, Renderer, worker::Worker};
    use std::{
        io::{self, Write},
        sync::Arc,
        time::Duration,
    };
    let labels = source::load(&request, &std::env::current_dir()?)?;
    let before = labels
        .before
        .bytes
        .as_deref()
        .map(|b| diff::decode(b, &labels.before.label))
        .transpose()?;
    let after = labels
        .after
        .bytes
        .as_deref()
        .map(|b| diff::decode(b, &labels.after.label))
        .transpose()?;
    let comparison = Arc::new(diff::compare(before, after)?);
    let mut session = terminal::Session::enter()?;
    let result = (|| -> anyhow::Result<()> {
        let mut terminal = Terminal::new(CrosstermBackend::new(io::BufWriter::with_capacity(
            64 * 1024,
            io::stdout(),
        )))?;
        let font = session.picker().font_size();
        let cell = (font.width, font.height);
        let compress = session
            .picker()
            .capabilities()
            .contains(&ratatui_image::picker::Capability::KittyCompression);
        let worker = Worker::spawn(comparison.clone());
        let mut app = app::App::new(comparison);
        let mut prepared: Option<PreparedFrame> = None;
        let mut pending_ids = Vec::new();
        let mut submitted = None;
        loop {
            let size = terminal.size()?;
            let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
            app.resize(area, cell);
            let mut dirty = false;
            let mut retired_ids = Vec::new();
            if submitted != Some(app.generation()) {
                // Pending images have not been sent. Keep the visible frame until
                // its replacement is ready, so an operation doesn't flash blank.
                session.delete_ids(&pending_ids)?;
                pending_ids.clear();
                submitted = Some(app.generation());
                let viewport = app.viewport();
                if viewport.width > 0 && viewport.height > 0 {
                    pending_ids = session.allocate_ids(if app.mode() == app::Mode::SideBySide {
                        2
                    } else {
                        1
                    });
                    worker.submit(RenderRequest {
                        generation: app.generation(),
                        mode: app.mode(),
                        viewport,
                        cell_pixels: cell,
                        compress,
                        image_ids: pending_ids.clone(),
                    })?;
                    dirty = prepared.is_none();
                } else {
                    if let Some(old) = prepared.take() {
                        retired_ids.extend(old.image_ids);
                    }
                    dirty = true;
                }
            }
            if let Some((generation, result)) = worker.poll()
                && let Some(frame) = render::accept_result(app.generation(), generation, result)?
            {
                if let Some(old) = prepared.replace(frame) {
                    retired_ids.extend(old.image_ids);
                }
                pending_ids.clear();
                dirty = true;
            }
            if dirty {
                session.begin_update()?;
                session.delete_ids(&retired_ids)?;
                terminal.draw(|f| Renderer::draw(f, &app, prepared.as_ref(), &labels))?;
                if let Some(frame) = prepared.as_mut() {
                    Renderer::write_images(terminal.backend_mut(), area, &app, frame)?;
                    terminal.backend_mut().flush()?;
                }
                session.end_update()?;
            }
            // Poll input frequently only while an asynchronous frame is pending.
            // Completed frames otherwise waited for the full 50ms idle timeout.
            let timeout = if pending_ids.is_empty() { 50 } else { 4 };
            if event::poll(Duration::from_millis(timeout))? {
                let mut quit = false;
                loop {
                    if let Event::Key(key) = event::read()?
                        && app.on_key(key)
                    {
                        quit = true;
                        break;
                    }
                    if !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
                if quit {
                    break;
                }
            }
        }
        Ok(())
    })();
    let restored = session.finish();
    result.and(restored)
}
