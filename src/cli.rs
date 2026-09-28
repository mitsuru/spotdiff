use clap::Parser;
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
};

#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Files { before: PathBuf, after: PathBuf },
    Git { path: PathBuf, staged: bool },
}
#[derive(Parser)]
#[command(
    name = "spotdiff",
    version,
    about = "Kitty Graphics Protocolで画像を比較",
    override_usage = "spotdiff <BEFORE> <AFTER>\n       spotdiff git [--staged] -- <PATH>"
)]
struct Files {
    before: PathBuf,
    after: PathBuf,
}
#[derive(Parser)]
#[command(name = "spotdiff git", about = "Gitの画像変更を比較")]
struct Git {
    #[arg(long)]
    staged: bool,
    path: PathBuf,
}

pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> anyhow::Result<Request> {
    let mut args: Vec<_> = args.into_iter().collect();
    if args.get(1).is_some_and(|a| a == OsStr::new("git")) {
        args.remove(1);
        let git = Git::try_parse_from(args)?;
        Ok(Request::Git {
            path: git.path,
            staged: git.staged,
        })
    } else {
        let files = Files::try_parse_from(args)?;
        Ok(Request::Files {
            before: files.before,
            after: files.after,
        })
    }
}
