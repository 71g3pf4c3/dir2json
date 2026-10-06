use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dir2json::{Mismatch, Options, diff_trees};
use serde_json::Value;

const USAGE: &str = "\
Usage: dir2json [OPTIONS] [DIR]

Convert a directory tree to a JSON object written to stdout, using the
conversion scheme of json2dir in reverse:

  directories           -> objects (keys are entry names)
  regular files         -> strings (file contents)
  executable files      -> [\"script\", contents]
  symlinks              -> [\"link\", target] (never followed)

Arguments:
  [DIR]                 Root directory to read [default: .]

Options:
  -c, --compact         Compact output instead of pretty-printed
  -s, --skip-special    Skip special files (FIFOs, sockets, devices) with a
                        warning instead of failing
      --check FILE      Compare DIR against the desired tree in FILE (a JSON
                        object, \"-\" for stdin) instead of printing; exits 3
                        with a mismatch report on drift
  -h, --help            Print help
  -V, --version         Print version

Exit status:
  0  success (or, with --check, trees match)
  1  conversion or I/O error
  2  usage error
  3  drift detected (--check only)";

const EXIT_SUCCESS: u8 = 0;
const EXIT_USAGE: u8 = 2;
const EXIT_DRIFT: u8 = 3;

/// How many mismatches to list before collapsing the rest into a summary.
const MAX_REPORTED_MISMATCHES: usize = 20;

fn main() -> ExitCode {
    let mut args = env::args_os();
    let program = args
        .next()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "dir2json".to_owned());

    let mut root = PathBuf::from(".");
    let mut compact = false;
    let mut skip_special = false;
    let mut check: Option<PathBuf> = None;
    let mut positional_seen = false;
    let mut no_more_options = false;

    while let Some(arg) = args.next() {
        if !no_more_options && let Some(flag) = arg.to_str() {
            match flag {
                "--" => {
                    no_more_options = true;
                    continue;
                }
                "-c" | "--compact" => {
                    compact = true;
                    continue;
                }
                "-s" | "--skip-special" => {
                    skip_special = true;
                    continue;
                }
                "-h" | "--help" => {
                    print!("{USAGE}");
                    return ExitCode::SUCCESS;
                }
                "-V" | "--version" => {
                    println!("dir2json {}", env!("CARGO_PKG_VERSION"));
                    return ExitCode::SUCCESS;
                }
                "--check" => {
                    let value = args.next().unwrap_or_else(|| {
                        usage_error(
                            &program,
                            "--check requires a file argument (use \"-\" for stdin)",
                        )
                    });
                    check = Some(PathBuf::from(value));
                    continue;
                }
                flag if flag.starts_with("--check=") => {
                    check = Some(PathBuf::from(&flag["--check=".len()..]));
                    continue;
                }
                flag if flag.starts_with('-') && flag != "-" => {
                    usage_error(&program, &format!("unknown option: {flag}"));
                }
                _ => {}
            }
        }

        if positional_seen {
            usage_error(&program, "unexpected extra argument");
        }
        root = PathBuf::from(arg);
        positional_seen = true;
    }

    match run(&root, compact, skip_special, check.as_deref()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("dir2json: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(
    root: &Path,
    compact: bool,
    skip_special: bool,
    check: Option<&Path>,
) -> Result<u8, Box<dyn Error>> {
    let actual = dir2json::dir_to_json(root, &Options { skip_special })?;

    let Some(desired_path) = check else {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        if compact {
            serde_json::to_writer(&mut out, &actual)?;
        } else {
            serde_json::to_writer_pretty(&mut out, &actual)?;
        }
        out.write_all(b"\n")?;
        out.flush()?;
        return Ok(EXIT_SUCCESS);
    };

    let desired = read_desired(desired_path)?;
    let mismatches = diff_trees(&desired, &actual);
    if mismatches.is_empty() {
        return Ok(EXIT_SUCCESS);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    report_mismatches(&mut out, &mismatches)?;
    out.flush()?;
    Ok(EXIT_DRIFT)
}

fn read_desired(path: &Path) -> Result<Value, Box<dyn Error>> {
    let value: Value = if path == Path::new("-") {
        serde_json::from_reader(io::stdin().lock())
            .map_err(|e| format!("failed to parse desired tree from stdin: {e}"))?
    } else {
        let file = fs::File::open(path)
            .map_err(|e| format!("cannot open desired tree {}: {e}", path.display()))?;
        serde_json::from_reader(file)
            .map_err(|e| format!("failed to parse desired tree {}: {e}", path.display()))?
    };

    if !value.is_object() {
        return Err(format!(
            "desired tree {}: root must be a JSON object",
            path.display()
        )
        .into());
    }
    Ok(value)
}

fn report_mismatches(out: &mut impl Write, mismatches: &[Mismatch]) -> io::Result<()> {
    let count = mismatches.len();
    writeln!(
        out,
        "drift detected: {} mismatch{}",
        count,
        if count == 1 { "" } else { "es" }
    )?;
    for mismatch in mismatches.iter().take(MAX_REPORTED_MISMATCHES) {
        match (&mismatch.expected, &mismatch.actual) {
            (Some(expected), Some(actual)) => writeln!(
                out,
                "  {}: expected {}, got {}",
                mismatch.path,
                preview(expected),
                preview(actual)
            )?,
            (Some(_), None) => writeln!(out, "  {}: missing on disk", mismatch.path)?,
            (None, Some(_)) => writeln!(out, "  {}: not in the desired tree", mismatch.path)?,
            (None, None) => unreachable!("a mismatch always has a side"),
        }
    }
    if count > MAX_REPORTED_MISMATCHES {
        writeln!(out, "  ... and {} more", count - MAX_REPORTED_MISMATCHES)?;
    }
    Ok(())
}

/// Compact JSON rendering of a value, truncated for one-line reporting.
fn preview(value: &Value) -> String {
    const LIMIT: usize = 120;
    let serialized = serde_json::to_string(value).unwrap_or_default();
    if serialized.chars().count() > LIMIT {
        let mut truncated: String = serialized.chars().take(LIMIT - 3).collect();
        truncated.push_str("...");
        truncated
    } else {
        serialized
    }
}

fn usage_error(program: &str, message: &str) -> ! {
    eprintln!("{program}: {message}");
    eprintln!("Try 'dir2json --help' for more information.");
    std::process::exit(EXIT_USAGE as i32);
}
