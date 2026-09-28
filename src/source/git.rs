use super::{InputSide, SourcePair, read_regular_file};
use anyhow::{Context, bail, ensure};
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn command(root: &Path, args: &[&OsStr]) -> anyhow::Result<Output> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("--literal-pathspecs")
        .args(args)
        .output()
        .context("Failed to run Git")
}
fn checked(root: &Path, args: &[&OsStr]) -> anyhow::Result<Vec<u8>> {
    let out = command(root, args)?;
    ensure!(
        out.status.success(),
        "Git read failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(out.stdout)
}
fn strings<'a>(args: &'a [&'a str]) -> Vec<&'a OsStr> {
    args.iter().map(OsStr::new).collect()
}
fn resolve_missing(path: &Path) -> anyhow::Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(p) => Ok(p),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let parent = path.parent().context("Image path has no parent")?;
            let name = path.file_name().context("Image path has no file name")?;
            Ok(resolve_missing(parent)?.join(name))
        }
        Err(e) => Err(e).with_context(|| format!("Failed to resolve path: {}", path.display())),
    }
}
#[cfg(unix)]
fn bytes(s: &OsStr) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes()
}
#[cfg(not(unix))]
fn bytes(s: &OsStr) -> &[u8] {
    s.as_encoded_bytes()
}
fn object(root: &Path, path: &Path, head: bool) -> anyhow::Result<Option<Vec<u8>>> {
    let path = path.as_os_str();
    let args = if head {
        vec![
            OsStr::new("ls-tree"),
            OsStr::new("-rz"),
            OsStr::new("--full-tree"),
            OsStr::new("HEAD"),
            OsStr::new("--"),
            path,
        ]
    } else {
        vec![
            OsStr::new("ls-files"),
            OsStr::new("--stage"),
            OsStr::new("-z"),
            OsStr::new("--"),
            path,
        ]
    };
    let data = checked(root, &args)?;
    let mut id = None;
    for record in data.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .context("Failed to parse Git file metadata")?;
        if &record[tab + 1..] != bytes(path) {
            continue;
        }
        let fields: Vec<_> = record[..tab].split(|b| *b == b' ').collect();
        ensure!(fields.len() == 3, "Invalid Git file metadata");
        ensure!(
            fields[0] == b"100644" || fields[0] == b"100755",
            "Git image inputs must be regular files"
        );
        if !head {
            ensure!(
                fields[2] == b"0",
                "Image has a merge conflict. Resolve the conflict first"
            );
        }
        let oid = if head { fields[2] } else { fields[1] };
        id = Some(OsString::from(String::from_utf8(oid.to_vec())?));
    }
    let content = id
        .map(|id| checked(root, &[OsStr::new("cat-file"), OsStr::new("blob"), &id]))
        .transpose()?;
    if !head && content.as_ref().is_some_and(Vec::is_empty) && intent_to_add(root, path)? {
        return Ok(None);
    }
    Ok(content)
}
fn intent_to_add(root: &Path, path: &OsStr) -> anyhow::Result<bool> {
    // Compare Git's two documented ITA views; do not mistake a real empty blob
    // for intent-to-add or parse the unstable ls-files --debug output.
    let view = |option: &str| {
        checked(
            root,
            &[
                OsStr::new("diff"),
                OsStr::new("--cached"),
                OsStr::new("--raw"),
                OsStr::new("-z"),
                OsStr::new("--no-ext-diff"),
                OsStr::new("--no-textconv"),
                OsStr::new("--no-renames"),
                OsStr::new(option),
                OsStr::new("--"),
                path,
            ],
        )
    };
    Ok(view("--ita-visible-in-index")? != view("--ita-invisible-in-index")?)
}
fn has_head(root: &Path) -> anyhow::Result<bool> {
    if command(root, &strings(&["rev-parse", "--verify", "HEAD^{commit}"]))?
        .status
        .success()
    {
        return Ok(true);
    }
    let symbolic = command(root, &strings(&["symbolic-ref", "-q", "HEAD"]))?;
    ensure!(symbolic.status.success(), "Invalid Git HEAD");
    let mut name = symbolic.stdout;
    if name.last() == Some(&b'\n') {
        name.pop();
    }
    let name = OsString::from(String::from_utf8(name)?);
    let reference = command(
        root,
        &[
            OsStr::new("show-ref"),
            OsStr::new("--verify"),
            OsStr::new("--quiet"),
            &name,
        ],
    )?;
    ensure!(
        reference.status.code() == Some(1),
        "Git HEAD reference is corrupt"
    );
    Ok(false)
}
pub(super) fn load(path: &Path, staged: bool, cwd: &Path) -> anyhow::Result<SourcePair> {
    let mut root = checked(cwd, &strings(&["rev-parse", "--show-toplevel"]))?;
    if root.last() == Some(&b'\n') {
        root.pop();
    }
    #[cfg(unix)]
    let root = {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(OsString::from_vec(root))
    };
    #[cfg(not(unix))]
    let root = PathBuf::from(String::from_utf8(root)?);
    let root = fs::canonicalize(root)?;
    let absolute = cwd.join(path);
    if let Ok(meta) = fs::symlink_metadata(&absolute) {
        ensure!(
            !meta.file_type().is_symlink(),
            "Symbolic links are not supported for image comparison"
        );
    }
    let absolute = resolve_missing(&absolute)?;
    let relative = absolute
        .strip_prefix(&root)
        .context("Cannot compare images outside the Git repository")?;
    let index = object(&root, relative, false)?;
    let (before, after) = if staged {
        let head = if has_head(&root)? {
            object(&root, relative, true)?
        } else {
            None
        };
        (
            InputSide {
                label: format!("HEAD:{}", relative.display()),
                bytes: head,
            },
            InputSide {
                label: format!("index:{}", relative.display()),
                bytes: index,
            },
        )
    } else {
        let worktree = match read_regular_file(&absolute) {
            Ok(b) => Some(b),
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == ErrorKind::NotFound) =>
            {
                None
            }
            Err(e) => return Err(e).context("Failed to read image from the Git working tree"),
        };
        (
            InputSide {
                label: format!("index:{}", relative.display()),
                bytes: index,
            },
            InputSide {
                label: format!("worktree:{}", relative.display()),
                bytes: worktree,
            },
        )
    };
    if before.bytes.is_none() && after.bytes.is_none() {
        bail!(
            "No image on either side of the Git comparison: {}",
            path.display()
        );
    }
    Ok(SourcePair { before, after })
}
