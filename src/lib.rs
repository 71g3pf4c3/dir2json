//! `dir2json` converts a directory tree into a JSON object, using the
//! conversion scheme of [`json2dir`] (https://github.com/alurm/json2dir),
//! but in reverse:
//!
//! - directories become JSON objects (keys are entry names),
//! - regular files become strings (file contents),
//! - executable regular files become `["script", contents]`,
//! - symlinks become `["link", target]`.
//!
//! Symlinks are never followed (their targets are recorded verbatim), so
//! symlink cycles are impossible. Directory entries are sorted, making the
//! output deterministic.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// Options controlling tree traversal.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Skip special files (FIFOs, sockets, devices) instead of erroring out.
    /// Skipped entries are reported on stderr and omitted from the output.
    pub skip_special: bool,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The root path exists but is not a directory.
    #[error("{path}: not a directory")]
    NotADirectory { path: PathBuf },
    /// An I/O error occurred while inspecting or reading a path.
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// A file name or a symlink target is not valid UTF-8.
    #[error("{path}: name or symlink target is not valid UTF-8")]
    NonUtf8 { path: PathBuf },
    /// A regular file's contents are not valid UTF-8.
    #[error("{path}: file contents are not valid UTF-8")]
    NonUtf8Content { path: PathBuf },
    /// A special file was encountered and `skip_special` was not set.
    #[error("{path}: unsupported special file ({kind}); use --skip-special to skip")]
    Special { path: PathBuf, kind: &'static str },
}

/// Convert the directory tree rooted at `root` into a JSON value.
///
/// Returns [`Error::NotADirectory`] if `root` is not a directory. If `root`
/// itself is a symlink to a directory, it is followed (the root is the only
/// place where following makes sense, and it cannot cause a cycle).
pub fn dir_to_json(root: &Path, options: &Options) -> Result<Value, Error> {
    let metadata = fs::metadata(root).map_err(|source| Error::Io {
        path: root.to_owned(),
        source,
    })?;
    if !metadata.is_dir() {
        return Err(Error::NotADirectory {
            path: root.to_owned(),
        });
    }
    read_dir_object(root, options).map(Value::Object)
}

fn read_dir_object(dir: &Path, options: &Options) -> Result<Map<String, Value>, Error> {
    let mut names: Vec<String> = Vec::new();
    for entry in fs::read_dir(dir).map_err(|source| Error::Io {
        path: dir.to_owned(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::Io {
            path: dir.to_owned(),
            source,
        })?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(Error::NonUtf8 {
                path: dir.join(&name),
            });
        };
        names.push(name.to_owned());
    }
    names.sort();

    let mut map = Map::with_capacity(names.len());
    for name in names {
        let path = dir.join(&name);
        if let Some(value) = read_node(&path, options)? {
            map.insert(name, value);
        }
    }
    Ok(map)
}

/// Read a single filesystem node. Returns `None` if the entry is to be
/// skipped (see [`Options::skip_special`]).
fn read_node(path: &Path, options: &Options) -> Result<Option<Value>, Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        let target = fs::read_link(path).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })?;
        let Some(target) = target.to_str() else {
            return Err(Error::NonUtf8 {
                path: path.to_owned(),
            });
        };
        return Ok(Some(Value::Array(vec![
            Value::from("link"),
            Value::from(target),
        ])));
    }

    if file_type.is_dir() {
        return read_dir_object(path, options).map(Value::Object).map(Some);
    }

    if file_type.is_file() {
        let content = read_utf8_file(path)?;
        let value = if is_executable(&metadata) {
            Value::Array(vec![Value::from("script"), Value::String(content)])
        } else {
            Value::String(content)
        };
        return Ok(Some(value));
    }

    if options.skip_special {
        eprintln!(
            "dir2json: warning: skipping special file {} ({})",
            path.display(),
            special_kind(&file_type)
        );
        return Ok(None);
    }

    Err(Error::Special {
        path: path.to_owned(),
        kind: special_kind(&file_type),
    })
}

fn read_utf8_file(path: &Path) -> Result<String, Error> {
    let bytes = fs::read(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })?;
    String::from_utf8(bytes).map_err(|_| Error::NonUtf8Content {
        path: path.to_owned(),
    })
}

#[cfg(unix)]
fn special_kind(file_type: &fs::FileType) -> &'static str {
    use std::os::unix::fs::FileTypeExt;
    if file_type.is_fifo() {
        "fifo"
    } else if file_type.is_socket() {
        "socket"
    } else if file_type.is_block_device() {
        "block device"
    } else if file_type.is_char_device() {
        "character device"
    } else {
        "special file"
    }
}

#[cfg(not(unix))]
fn special_kind(_file_type: &fs::FileType) -> &'static str {
    "special file"
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    // The json2dir scheme does not support executable files on Windows.
    false
}

/// A single mismatch between a desired tree and an observed tree, as
/// reported by [`diff_trees`].
#[derive(Debug, Clone, PartialEq)]
pub struct Mismatch {
    /// Path of the mismatching entry, segments joined by `/`.
    pub path: String,
    /// The expected value. `None` if the entry is missing in the desired
    /// tree (i.e. it exists on disk but was not asked for).
    pub expected: Option<Value>,
    /// The observed value. `None` if the entry is missing on disk (i.e. it
    /// was asked for but does not exist).
    pub actual: Option<Value>,
}

/// Compare a desired tree against an observed tree (typically produced by
/// [`dir_to_json`]) and return every mismatch, ordered by path.
///
/// Both values must be JSON objects; any kind difference (missing entry,
/// string vs. object, different script contents, ...) is reported as a
/// single [`Mismatch`] on the longest common prefix.
pub fn diff_trees(desired: &Value, actual: &Value) -> Vec<Mismatch> {
    let mut mismatches = Vec::new();
    diff_at(Some(desired), Some(actual), String::new(), &mut mismatches);
    mismatches
}

fn diff_at(
    desired: Option<&Value>,
    actual: Option<&Value>,
    path: String,
    mismatches: &mut Vec<Mismatch>,
) {
    match (desired, actual) {
        (Some(Value::Object(desired)), Some(Value::Object(actual))) => {
            let mut names: std::collections::BTreeSet<&str> =
                desired.keys().map(String::as_str).collect();
            names.extend(actual.keys().map(String::as_str));
            for name in names {
                let child = if path.is_empty() {
                    name.to_owned()
                } else {
                    format!("{path}/{name}")
                };
                diff_at(desired.get(name), actual.get(name), child, mismatches);
            }
        }
        (Some(desired), Some(actual)) if desired == actual => {}
        (desired, actual) => mismatches.push(Mismatch {
            path,
            expected: desired.cloned(),
            actual: actual.cloned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    use serde_json::json;
    use tempfile::TempDir;

    fn strict(dir: &Path) -> Value {
        dir_to_json(dir, &Options::default()).expect("conversion failed")
    }

    #[test]
    fn regular_files_become_strings() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hello"), b"Hello, world!").unwrap();
        fs::write(dir.path().join("empty"), b"").unwrap();
        assert_eq!(
            strict(dir.path()),
            json!({ "hello": "Hello, world!", "empty": "" })
        );
    }

    #[test]
    fn executable_files_become_scripts() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("script");
        fs::write(&script, b"#!/bin/sh\necho Howdy!").unwrap();
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();

        let mut executable = serde_json::Map::new();
        executable.insert(
            "script".to_owned(),
            json!(["script", "#!/bin/sh\necho Howdy!"]),
        );
        assert_eq!(strict(dir.path()), Value::Object(executable));
    }

    #[test]
    fn symlinks_become_link_arrays_and_are_not_followed() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("target"), b"contents").unwrap();
        symlink("target", dir.path().join("alias")).unwrap();
        symlink("does-not-exist", dir.path().join("dangling")).unwrap();
        symlink("sub", dir.path().join("dirlink")).unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();

        assert_eq!(
            strict(dir.path()),
            json!({
                "alias": ["link", "target"],
                "dangling": ["link", "does-not-exist"],
                "dirlink": ["link", "sub"],
                "sub": {},
                "target": "contents",
            })
        );
    }

    #[test]
    fn nested_directories_become_nested_objects() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
        fs::write(dir.path().join("a/b/c/leaf"), b"42").unwrap();

        assert_eq!(
            strict(dir.path()),
            json!({ "a": { "b": { "c": { "leaf": "42" } } } })
        );
    }

    #[test]
    fn entries_are_sorted_deterministically() {
        let dir = TempDir::new().unwrap();
        for name in ["zebra", "apple", "mango"] {
            fs::write(dir.path().join(name), b"").unwrap();
        }
        let first = strict(dir.path());
        let second = strict(dir.path());
        assert_eq!(first, second);
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            r#"{"apple":"","mango":"","zebra":""}"#
        );
    }

    #[test]
    fn root_must_be_a_directory() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("file");
        fs::write(&file, b"").unwrap();
        assert!(matches!(
            dir_to_json(&file, &Options::default()),
            Err(Error::NotADirectory { .. })
        ));
    }

    #[test]
    fn diff_trees_empty_for_identical_trees() {
        let tree = json!({
            "greeting": "Hello, world!",
            "dir": { "subfile": "Content.\n", "subdir": {} },
            "symlink": ["link", "target path"],
            "script": ["script", "#!/bin/sh\necho Howdy!"],
        });
        assert!(diff_trees(&tree, &tree).is_empty());
    }

    #[test]
    fn diff_trees_reports_changed_missing_and_extra() {
        let desired = json!({
            "a": "old",
            "b": { "c": "x" },
            "gone": "keep me",
            "d": "untouched",
        });
        let actual = json!({
            "a": "new",
            "b": { "c": "x", "extra": "y" },
            "d": "untouched",
            "new": "surprise",
        });

        let mismatches = diff_trees(&desired, &actual);
        let paths: Vec<&str> = mismatches.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, ["a", "b/extra", "gone", "new"]);

        let a = &mismatches[0];
        assert_eq!(a.expected, Some(json!("old")));
        assert_eq!(a.actual, Some(json!("new")));

        let extra = &mismatches[1];
        assert_eq!(extra.expected, None);
        assert_eq!(extra.actual, Some(json!("y")));

        let gone = &mismatches[2];
        assert_eq!(gone.expected, Some(json!("keep me")));
        assert_eq!(gone.actual, None);
    }

    #[test]
    fn diff_trees_reports_kind_mismatches() {
        let desired = json!({ "x": { "y": "z" }, "s": ["script", "a"] });
        let actual = json!({ "x": "plain file", "s": "not executable" });

        let mismatches = diff_trees(&desired, &actual);
        let paths: Vec<&str> = mismatches.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, ["s", "x"]);
        assert_eq!(mismatches[0].expected, Some(json!(["script", "a"])));
        assert_eq!(mismatches[0].actual, Some(json!("not executable")));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_file_contents_are_rejected() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("binary"), [0xff, 0xfe, 0x00]).unwrap();
        assert!(matches!(
            dir_to_json(dir.path(), &Options::default()),
            Err(Error::NonUtf8Content { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn special_files_error_by_default_and_can_be_skipped() {
        let dir = TempDir::new().unwrap();
        let fifo = dir.path().join("pipe");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo must be available");
        assert!(status.success());

        assert!(matches!(
            dir_to_json(dir.path(), &Options::default()),
            Err(Error::Special { .. })
        ));

        let skipped = dir_to_json(dir.path(), &Options { skip_special: true }).unwrap();
        assert_eq!(skipped, json!({}));
    }

    /// End-to-end roundtrip: materialize a JSON tree the way `json2dir`
    /// would, then convert it back and compare.
    #[cfg(unix)]
    #[test]
    fn roundtrip_against_json2dir_scheme() {
        let original = json!({
            "greeting": "Hello, world!",
            "dir": {
                "subfile": "Content.\n",
                "subdir": {}
            },
            "symlink": ["link", "target path"],
            "script": ["script", "#!/bin/sh\necho Howdy!"],
        });

        let dir = TempDir::new().unwrap();
        materialize(&original, dir.path());
        assert_eq!(strict(dir.path()), original);
    }

    // Minimal json2dir-style materializer, used only for roundtrip testing.
    #[cfg(unix)]
    fn materialize(value: &Value, dir: &Path) {
        for (name, node) in value.as_object().expect("root must be an object") {
            let path = dir.join(name);
            match node {
                Value::String(contents) => fs::write(&path, contents).unwrap(),
                Value::Object(_) => {
                    fs::create_dir(&path).unwrap();
                    materialize(node, &path);
                }
                Value::Array(items) => match items.as_slice() {
                    [tag, Value::String(arg)] if tag == "link" => {
                        symlink(arg, &path).unwrap();
                    }
                    [tag, Value::String(arg)] if tag == "script" => {
                        fs::write(&path, arg).unwrap();
                        let mut perms = fs::metadata(&path).unwrap().permissions();
                        perms.set_mode(0o755);
                        fs::set_permissions(&path, perms).unwrap();
                    }
                    _ => panic!("unsupported array: {items:?}"),
                },
                _ => panic!("unsupported value: {node:?}"),
            }
        }
    }
}
