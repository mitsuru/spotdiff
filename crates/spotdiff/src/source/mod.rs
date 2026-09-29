mod git;
use crate::cli::Request;
use anyhow::{Context, ensure};
use std::{fs, path::Path};
#[derive(Debug)]
pub struct InputSide {
    pub label: String,
    pub bytes: Option<Vec<u8>>,
}
#[derive(Debug)]
pub struct SourcePair {
    pub before: InputSide,
    pub after: InputSide,
}
fn read_regular_file(path: &Path) -> anyhow::Result<Vec<u8>> {
    ensure!(
        fs::metadata(path)?.is_file(),
        "Image inputs must be regular files: {}",
        path.display()
    );
    Ok(fs::read(path)?)
}
pub fn load(request: &Request, cwd: &Path) -> anyhow::Result<SourcePair> {
    match request {
        Request::Files { before, after } => {
            let read = |path: &Path| -> anyhow::Result<InputSide> {
                Ok(InputSide {
                    label: path.display().to_string(),
                    bytes: Some(
                        read_regular_file(&cwd.join(path))
                            .with_context(|| format!("Failed to read image: {}", path.display()))?,
                    ),
                })
            };
            Ok(SourcePair {
                before: read(before)?,
                after: read(after)?,
            })
        }
        Request::Git { path, staged } => git::load(path, *staged, cwd),
    }
}
