fn main() -> std::process::ExitCode {
    let request = match spotdiff::cli::parse_args(std::env::args_os()) {
        Ok(request) => request,
        Err(error) => {
            if let Some(clap) = error.downcast_ref::<clap::Error>() {
                let text = clap
                    .render()
                    .to_string()
                    .lines()
                    .map(spotdiff::render::sanitize_label)
                    .collect::<Vec<_>>()
                    .join("\n");
                if clap.use_stderr() {
                    eprintln!("{text}");
                } else {
                    println!("{text}");
                }
                return std::process::ExitCode::from(clap.exit_code() as u8);
            }
            eprintln!(
                "spotdiff: {}",
                spotdiff::render::sanitize_label(&format!("{error:#}"))
            );
            return std::process::ExitCode::FAILURE;
        }
    };
    match spotdiff::run(request) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!(
                "spotdiff: {}",
                spotdiff::render::sanitize_label(&format!("{error:#}"))
            );
            std::process::ExitCode::FAILURE
        }
    }
}
