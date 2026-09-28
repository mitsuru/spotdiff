use spotdiff::{cli::Request, source::load};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
fn git_output(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .unwrap()
}
fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let out = git_output(root, args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}
fn repo() -> TempDir {
    let r = TempDir::new().unwrap();
    git(r.path(), &["init", "-q"]);
    r
}
fn req(name: &str, staged: bool) -> Request {
    Request::Git {
        path: name.into(),
        staged,
    }
}
fn committed(r: &TempDir, name: &str) {
    fs::write(r.path().join(name), b"head").unwrap();
    git(r.path(), &["add", "--", name]);
    git(r.path(), &["commit", "-qm", "initial"]);
}
#[test]
fn distinguishes_index_worktree_and_head() {
    let r = repo();
    committed(&r, "a.png");
    fs::write(r.path().join("a.png"), b"index").unwrap();
    git(r.path(), &["add", "a.png"]);
    fs::write(r.path().join("a.png"), b"worktree").unwrap();
    let a = load(&req("a.png", false), r.path()).unwrap();
    let b = load(&req("a.png", true), r.path()).unwrap();
    assert_eq!(a.before.bytes.as_deref(), Some(&b"index"[..]));
    assert_eq!(a.after.bytes.as_deref(), Some(&b"worktree"[..]));
    assert_eq!(b.before.bytes.as_deref(), Some(&b"head"[..]));
    assert_eq!(b.after.bytes.as_deref(), Some(&b"index"[..]));
}
#[test]
fn untracked_and_added_have_missing_before() {
    let r = repo();
    fs::write(r.path().join("a.png"), b"new").unwrap();
    let a = load(&req("a.png", false), r.path()).unwrap();
    assert!(a.before.bytes.is_none());
    assert_eq!(a.after.bytes.as_deref(), Some(&b"new"[..]));
    git(r.path(), &["add", "a.png"]);
    let a = load(&req("a.png", true), r.path()).unwrap();
    assert!(a.before.bytes.is_none());
}
#[test]
fn deleted_has_missing_after() {
    let r = repo();
    committed(&r, "a.png");
    fs::remove_file(r.path().join("a.png")).unwrap();
    let a = load(&req("a.png", false), r.path()).unwrap();
    assert_eq!(a.before.bytes.as_deref(), Some(&b"head"[..]));
    assert!(a.after.bytes.is_none());
    git(r.path(), &["add", "-u"]);
    let a = load(&req("a.png", true), r.path()).unwrap();
    assert!(a.after.bytes.is_none());
}
#[test]
fn missing_both_is_error() {
    let r = repo();
    assert!(load(&req("missing.png", false), r.path()).is_err());
}
#[test]
fn subdirectory_and_unusual_names() {
    let r = repo();
    fs::create_dir(r.path().join("sub")).unwrap();
    let name = "sub/-a '日本語\n.png";
    committed(&r, name);
    let a = load(&req("-a '日本語\n.png", false), &r.path().join("sub")).unwrap();
    assert_eq!(a.before.bytes.as_deref(), Some(&b"head"[..]));
}
#[test]
fn outside_symlink_is_error() {
    let r = repo();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("a.png"), b"other").unwrap();
    std::os::unix::fs::symlink(outside.path().join("a.png"), r.path().join("a.png")).unwrap();
    assert!(load(&req("a.png", false), r.path()).is_err());
}
#[test]
fn conflicted_index_is_error() {
    let r = repo();
    committed(&r, "a.png");
    git(r.path(), &["checkout", "-qb", "other"]);
    fs::write(r.path().join("a.png"), b"other").unwrap();
    git(r.path(), &["commit", "-qam", "other"]);
    git(r.path(), &["checkout", "-q", "-"]);
    fs::write(r.path().join("a.png"), b"main").unwrap();
    git(r.path(), &["commit", "-qam", "main"]);
    let merge = git_output(r.path(), &["merge", "other"]);
    assert_eq!(
        merge.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&merge.stderr)
    );
    assert!(!git(r.path(), &["ls-files", "--unmerged"]).is_empty());
    for staged in [false, true] {
        let error = load(&req("a.png", staged), r.path()).unwrap_err();
        assert!(error.to_string().contains("merge conflict"), "{error}");
    }
}
#[test]
fn corrupt_head_is_error() {
    let r = repo();
    committed(&r, "a.png");
    fs::write(r.path().join(".git/HEAD"), "ref: refs/heads/broken\n").unwrap();
    fs::write(
        r.path().join(".git/refs/heads/broken"),
        "0123456789012345678901234567890123456789\n",
    )
    .unwrap();
    assert!(load(&req("a.png", true), r.path()).is_err());
}
#[test]
fn intent_to_add_is_missing_but_real_empty_blob_is_present() {
    let r = repo();
    let name = "new '日本語\n.png";
    fs::write(r.path().join(name), b"new").unwrap();
    git(r.path(), &["add", "-N", "--", name]);
    let a = load(&req(name, false), r.path()).unwrap();
    assert!(a.before.bytes.is_none());
    assert_eq!(a.after.bytes.as_deref(), Some(&b"new"[..]));
    assert!(load(&req(name, true), r.path()).is_err());
    fs::remove_file(r.path().join(name)).unwrap();
    assert!(load(&req(name, false), r.path()).is_err());
    fs::write(r.path().join("empty.png"), b"").unwrap();
    git(r.path(), &["add", "empty.png"]);
    let a = load(&req("empty.png", false), r.path()).unwrap();
    assert_eq!(a.before.bytes.as_deref(), Some(&b""[..]));
    // Even when HEAD contains the same empty blob, intent-to-add is missing.
    git(r.path(), &["commit", "-qm", "empty"]);
    git(r.path(), &["rm", "--cached", "empty.png"]);
    git(r.path(), &["add", "-N", "empty.png"]);
    assert!(
        load(&req("empty.png", false), r.path())
            .unwrap()
            .before
            .bytes
            .is_none()
    );
}
