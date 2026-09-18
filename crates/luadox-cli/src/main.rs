//! The `luadox` command.

use std::path::PathBuf;
use std::process::ExitCode;

use luadox::Options;

const USAGE: &str = "\
usage: luadox [options] [FILE ...]

  -c, --config FILE            luadox configuration file
  -r, --renderer TYPE          how to render the parsed content (json, luals)
  -o, --out PATH               where to write the rendered output
  -n, --name NAME              project name
      --snippet-path PATH      where @example <file> snippets are read from
      --allow-incomplete CATS  diagnostic categories that may leave the documentation
                               incomplete without failing the run
  -m, --manual ID=FILE         add a manual page
      --diagnostics-json FILE  write every diagnostic to FILE as JSON
      --diagnostics-root DIR   base directory those paths are relative to
      --nofollow               do not follow require()d files
  -h, --help                   this message
";

fn main() -> ExitCode {
    let mut options = Options {
        nofollow: true,
        ..Options::default()
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        let result = match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-c" | "--config" => value().map(|v| options.config = Some(PathBuf::from(v))),
            "-r" | "--renderer" => value().map(|v| options.renderer = Some(v)),
            "-o" | "--out" => value().map(|v| options.out = Some(v)),
            "-n" | "--name" => value().map(|v| options.name = Some(v)),
            "--snippet-path" => value().map(|v| options.snippet_path = Some(v)),
            "--allow-incomplete" => value().map(|v| options.allow_incomplete = Some(v)),
            "-m" | "--manual" => value().map(|v| options.manual.push(v)),
            "--diagnostics-json" => {
                value().map(|v| options.diagnostics_json = Some(PathBuf::from(v)))
            }
            "--diagnostics-root" => {
                value().map(|v| options.diagnostics_root = Some(PathBuf::from(v)))
            }
            "--nofollow" => Ok(()),
            other if other.starts_with('-') => Err(format!("unknown option {other}")),
            other => {
                options.files.push(other.to_string());
                Ok(())
            }
        };
        if let Err(message) = result {
            eprintln!("error: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    }

    match luadox::run(&options) {
        Ok(outcome) => {
            for line in &outcome.summary {
                eprintln!("{line}");
            }
            eprintln!("rendered to {}", outcome.output.display());
            ExitCode::from(outcome.exit_code as u8)
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
