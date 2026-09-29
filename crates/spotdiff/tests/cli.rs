use spotdiff::cli::{Request, parse_args};
use std::ffi::OsString;
fn args(xs: &[&str]) -> Vec<OsString> {
    xs.iter().map(OsString::from).collect()
}
#[test]
fn parses_three_forms() {
    assert_eq!(
        parse_args(args(&["spotdiff", "a.png", "b.png"])).unwrap(),
        Request::Files {
            before: "a.png".into(),
            after: "b.png".into()
        }
    );
    assert_eq!(
        parse_args(args(&["spotdiff", "git", "--", "a.png"])).unwrap(),
        Request::Git {
            path: "a.png".into(),
            staged: false
        }
    );
    assert_eq!(
        parse_args(args(&["spotdiff", "git", "--staged", "--", "a.png"])).unwrap(),
        Request::Git {
            path: "a.png".into(),
            staged: true
        }
    );
}
#[test]
fn unusual_paths_and_missing_arguments() {
    assert_eq!(
        parse_args(args(&["spotdiff", "--", "-a '日本語.png", "git"])).unwrap(),
        Request::Files {
            before: "-a '日本語.png".into(),
            after: "git".into()
        }
    );
    assert!(parse_args(args(&["spotdiff", "a.png"])).is_err());
    assert!(parse_args(args(&["spotdiff", "git", "--staged"])).is_err());
}
use std::{fs, io::Cursor, process::Command};
#[test]
fn help_and_version() {
    for arg in ["--help", "--version"] {
        let o = Command::new(env!("CARGO_BIN_EXE_spotdiff"))
            .arg(arg)
            .output()
            .unwrap();
        assert!(o.status.success());
        assert!(String::from_utf8_lossy(&o.stdout).contains("spotdiff"));
    }
}
#[test]
fn non_tty_explains_error() {
    let d = tempfile::TempDir::new().unwrap();
    let p = d.path().join("a.png");
    let mut b = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255]))
        .write_to(&mut b, image::ImageFormat::Png)
        .unwrap();
    fs::write(&p, b.into_inner()).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_spotdiff"))
        .arg(&p)
        .arg(&p)
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("TTY"));
}
#[test]
fn unsupported_image_explains_error() {
    let d = tempfile::TempDir::new().unwrap();
    let p = d.path().join("bad.png");
    fs::write(&p, b"<svg/>").unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_spotdiff"))
        .arg(&p)
        .arg(&p)
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("bad.png"));
    assert!(!o.stdout.contains(&27));
}
#[test]
fn missing_file_explains_error() {
    let d = tempfile::TempDir::new().unwrap();
    let p = d.path().join("missing.png");
    let o = Command::new(env!("CARGO_BIN_EXE_spotdiff"))
        .arg(&p)
        .arg(&p)
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("missing.png"));
    assert!(!o.stdout.contains(&27));
}
#[test]
fn non_repository_explains_error() {
    let d = tempfile::TempDir::new().unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_spotdiff"))
        .current_dir(d.path())
        .args(["git", "--", "a.png"])
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("Git"));
    assert!(!o.stdout.contains(&27));
}
#[test]
fn error_messages_never_emit_filename_control_sequences() {
    let d = tempfile::TempDir::new().unwrap();
    let p = d.path().join("bad\x1b[2J.png");
    for exists in [false, true] {
        if exists {
            fs::write(&p, b"invalid image").unwrap();
        }
        let o = Command::new(env!("CARGO_BIN_EXE_spotdiff"))
            .arg(&p)
            .arg(&p)
            .output()
            .unwrap();
        assert!(!o.status.success());
        assert!(
            !o.stderr.contains(&27),
            "stderr contains an escape sequence"
        );
        assert!(String::from_utf8_lossy(&o.stderr).contains("bad�[2J.png"));
    }
    let o = Command::new(env!("CARGO_BIN_EXE_spotdiff"))
        .arg("--bad\x1b[2J")
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(!o.stderr.contains(&27));
}
