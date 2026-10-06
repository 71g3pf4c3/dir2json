use std::env;
use std::error::Error;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dir2json::Options;

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
  -h, --help            Print help
  -V, --version         Print version";

fn main() -> ExitCode {
    let mut args = env::args_os();
    let program = args
        .next()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "dir2json".to_owned());

    let mut root = PathBuf::from(".");
    let mut compact = false;
    let mut skip_special = false;
    let mut positional_seen = false;
    let mut no_more_options = false;

    for arg in args {
        let Some(arg) = arg.to_str() else {
            usage_error(&program, "arguments must be valid UTF-8");
        };

        if !no_more_options {
            match arg {
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
                arg if arg.starts_with('-') && arg != "-" => {
                    usage_error(&program, &format!("unknown option: {arg}"));
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

    match run(&root, compact, skip_special) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dir2json: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(root: &Path, compact: bool, skip_special: bool) -> Result<(), Box<dyn Error>> {
    let value = dir2json::dir_to_json(root, &Options { skip_special })?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    if compact {
        serde_json::to_writer(&mut out, &value)?;
    } else {
        serde_json::to_writer_pretty(&mut out, &value)?;
    }
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

fn usage_error(program: &str, message: &str) -> ! {
    eprintln!("{program}: {message}");
    eprintln!("Try 'dir2json --help' for more information.");
    std::process::exit(2);
}
