//! End-to-end tests for the `dir2json` binary.

use std::fs;
use std::io::Write;
use std::os::unix::fs::symlink;
use std::process::{Command, Output, Stdio};

use serde_json::json;
use tempfile::TempDir;

fn dir2json(args: &[&str], cwd: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dir2json"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("failed to spawn dir2json")
}

fn make_tree() -> (TempDir, serde_json::Value) {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("greeting"), b"Hello, world!").unwrap();
    fs::create_dir(dir.path().join("dir")).unwrap();
    fs::write(dir.path().join("dir/subfile"), b"Content.\n").unwrap();
    symlink("target", dir.path().join("symlink")).unwrap();

    let tree = json!({
        "dir": { "subfile": "Content.\n" },
        "greeting": "Hello, world!",
        "symlink": ["link", "target"],
    });
    (dir, tree)
}

#[test]
fn help_and_version_succeed() {
    let (dir, _) = make_tree();

    let out = dir2json(&["--help"], dir.path());
    assert!(out.status.success());
    assert!(out.stdout.starts_with(b"Usage: dir2json"));

    let out = dir2json(&["--version"], dir.path());
    assert!(out.status.success());
    assert!(out.stdout.starts_with(b"dir2json 0."));
}

#[test]
fn usage_errors_exit_2() {
    let (dir, _) = make_tree();

    for args in [vec!["--bogus"], vec!["--check"], vec!["a", "b"]] {
        let out = dir2json(&args, dir.path());
        assert_eq!(out.status.code(), Some(2), "args: {args:?}");
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn compact_output_is_stable_json() {
    let (dir, tree) = make_tree();

    let out = dir2json(&["--compact", "."], dir.path());
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.trim(), serde_json::to_string(&tree).unwrap());
}

#[test]
fn non_directory_root_exits_1() {
    let (dir, _) = make_tree();
    let file = dir.path().join("greeting");

    let out = dir2json(&[file.to_str().unwrap()], dir.path());
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("not a directory"), "stderr: {stderr}");
}

#[test]
fn check_reports_no_drift_when_trees_match() {
    let (dir, tree) = make_tree();
    let aux = TempDir::new().unwrap();
    let desired = aux.path().join("desired.json");
    fs::write(&desired, serde_json::to_string(&tree).unwrap()).unwrap();

    let out = dir2json(&["--check", desired.to_str().unwrap(), "."], dir.path());
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn check_reports_drift_with_paths_and_exit_3() {
    let (dir, _) = make_tree();
    let aux = TempDir::new().unwrap();
    let desired = aux.path().join("desired.json");
    fs::write(
        &desired,
        r#"{
            "greeting": "Goodbye!",
            "dir": { "subfile": "Content.\n" },
            "symlink": ["link", "target"],
            "extra": "never materialized"
        }"#,
    )
    .unwrap();

    let out = dir2json(&["--check", desired.to_str().unwrap(), "."], dir.path());
    assert_eq!(out.status.code(), Some(3));

    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("2 mismatches"), "stdout: {stdout}");
    assert!(stdout.contains("greeting: expected \"Goodbye!\", got \"Hello, world!\""));
    assert!(stdout.contains("extra: missing on disk"));
}

#[test]
fn check_accepts_desired_tree_from_stdin() {
    let (dir, tree) = make_tree();

    let mut child = Command::new(env!("CARGO_BIN_EXE_dir2json"))
        .args(["--check", "-", "."])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn dir2json");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(serde_json::to_string(&tree).unwrap().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn check_rejects_unreadable_and_malformed_desired_trees() {
    let (dir, _) = make_tree();

    let out = dir2json(&["--check", "no-such-file.json", "."], dir.path());
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("cannot open desired tree"),
        "stderr: {stderr}"
    );

    let malformed = dir.path().join("malformed.json");
    fs::write(&malformed, "{not json").unwrap();
    let out = dir2json(&["--check", "malformed.json", "."], dir.path());
    assert_eq!(out.status.code(), Some(1));

    let scalar = dir.path().join("scalar.json");
    fs::write(&scalar, "42").unwrap();
    let out = dir2json(&["--check", "scalar.json", "."], dir.path());
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("root must be a JSON object"),
        "stderr: {stderr}"
    );
}
