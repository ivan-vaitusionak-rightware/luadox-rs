//! The `luadox` command.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use luadox::{Options, Renderer};

#[derive(Parser)]
#[command(name = "luadox", bin_name = "luadox")]
struct Args {
    /// luadox configuration file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// how to render the parsed content (json, luals)
    #[arg(short, long, value_name = "TYPE")]
    renderer: Option<RendererName>,

    /// where to write the rendered output
    #[arg(short, long, value_name = "PATH")]
    out: Option<String>,

    /// project name
    #[arg(short, long, value_name = "NAME")]
    name: Option<String>,

    /// where @example <file> snippets are read from
    #[arg(long, value_name = "PATH")]
    snippet_path: Option<String>,

    /// diagnostic categories that may leave the documentation incomplete without failing
    /// the run
    #[arg(long, value_name = "CATS")]
    allow_incomplete: Option<String>,

    /// add a manual page
    #[arg(short, long, value_name = "ID=FILE")]
    manual: Vec<String>,

    /// write every diagnostic to FILE as JSON
    #[arg(long, value_name = "FILE")]
    diagnostics_json: Option<PathBuf>,

    /// base directory those paths are relative to
    #[arg(long, value_name = "DIR")]
    diagnostics_root: Option<PathBuf>,

    /// do not follow require()d files
    #[arg(long)]
    nofollow: bool,

    #[arg(value_name = "FILE")]
    files: Vec<String>,
}

/// The library's `Renderer`, spelled the way clap wants a value enum declared.
#[derive(Clone, Copy, clap::ValueEnum)]
enum RendererName {
    Html,
    Json,
    Luals,
}

impl From<RendererName> for Renderer {
    fn from(name: RendererName) -> Self {
        match name {
            RendererName::Html => Self::Html,
            RendererName::Json => Self::Json,
            RendererName::Luals => Self::Luals,
        }
    }
}

impl From<Args> for Options {
    fn from(args: Args) -> Self {
        Self {
            config: args.config,
            files: args.files,
            renderer: args.renderer.map(Renderer::from),
            out: args.out,
            name: args.name,
            snippet_path: args.snippet_path,
            allow_incomplete: args.allow_incomplete,
            manual: args.manual,
            diagnostics_json: args.diagnostics_json,
            diagnostics_root: args.diagnostics_root,
            // Following require()d files is not implemented, so `--nofollow` is always in
            // effect whether or not it was given.
            nofollow: true,
        }
    }
}

fn main() -> ExitCode {
    let options = Options::from(Args::parse());
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
