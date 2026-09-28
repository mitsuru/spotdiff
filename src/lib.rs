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
    use std::{io, sync::Arc, time::Duration};
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
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        let font = session.picker().font_size();
        let cell = (font.width, font.height);
        let worker = Worker::spawn(comparison.clone());
        let mut app = app::App::new(comparison);
        let mut prepared: Option<PreparedFrame> = None;
        let mut pending_ids = Vec::new();
        let mut submitted = None;
        loop {
            let size = terminal.size()?;
            app.resize(
                ratatui::layout::Rect::new(0, 0, size.width, size.height),
                cell,
            );
            if submitted != Some(app.generation()) {
                if let Some(old) = prepared.take() {
                    session.delete_ids(&old.image_ids)?;
                }
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
                        image_ids: pending_ids.clone(),
                    })?;
                }
            }
            if let Some((generation, result)) = worker.poll()
                && let Some(frame) = render::accept_result(app.generation(), generation, result)?
            {
                prepared = Some(frame);
                pending_ids.clear();
            }
            terminal.draw(|f| Renderer::draw(f, &app, prepared.as_ref(), &labels))?;
            if event::poll(Duration::from_millis(50))? {
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
