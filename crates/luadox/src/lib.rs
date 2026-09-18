//! luadox: Lua API documentation, rewritten in Rust.
//!
//! The pipeline is the Python's, with the guesswork taken out of the front of it:
//!
//! ```text
//!   config  ->  lua (parse)  ->  parse (scan + register)  ->  prerender  ->  render
//! ```
//!
//! Phase 2 of the rewrite covers everything up to and including the json renderer, which
//! is the document made inspectable and therefore what the differential harness compares.
//! The LuaLS and html renderers are Phase 3 and Phase 4.

pub mod config;
pub mod content;
pub mod diag;
pub mod ir;
pub mod json;
pub mod lua;
pub mod markdown;
pub mod parse;
pub mod prerender;
pub mod render;
pub mod tags;
pub mod util;

use std::fmt;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::diag::Category;
use crate::parse::Parser;

#[derive(Debug)]
pub enum Error {
    Config(String),
    NoInput,
    UnknownRenderer(String),
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Config(msg) => write!(f, "{msg}"),
            Error::NoInput => write!(
                f,
                "no input files or directories specified on command line or config file"
            ),
            Error::UnknownRenderer(name) => {
                write!(f, "unknown renderer \"{name}\", valid types are: json")
            }
            Error::Io(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Default)]
pub struct Options {
    pub config: Option<PathBuf>,
    pub files: Vec<String>,
    pub renderer: Option<String>,
    pub out: Option<String>,
    pub name: Option<String>,
    pub snippet_path: Option<String>,
    pub allow_incomplete: Option<String>,
    pub manual: Vec<String>,
    pub diagnostics_json: Option<PathBuf>,
    pub diagnostics_root: Option<PathBuf>,
    pub nofollow: bool,
}

/// What a run reports on the way out: the process exit code plus the lines a human reads.
pub struct Outcome {
    pub exit_code: i32,
    pub summary: Vec<String>,
    pub output: PathBuf,
}

pub fn run(options: &Options) -> Result<Outcome, Error> {
    let config = build_config(options)?;
    let renderer = options
        .renderer
        .clone()
        .unwrap_or_else(|| config.get_or("project", "renderer", "html"));
    if renderer != "json" {
        // Phase 2 ships the json renderer only, and says so rather than rendering
        // something that is not what was asked for.
        return Err(Error::UnknownRenderer(renderer));
    }

    let files = input_files(&config);
    if files.is_empty() {
        return Err(Error::NoInput);
    }

    let encoding = config.get_or("project", "encoding", "utf8");
    if !matches!(encoding.to_ascii_lowercase().as_str(), "utf8" | "utf-8") {
        return Err(Error::Config(format!(
            "encoding \"{encoding}\" is not supported; luadox reads utf-8"
        )));
    }
    if config.get_bool("project", "follow", true) && !options.nofollow {
        return Err(Error::Config(
            "follow = true is not supported: set follow = false, or list every file".to_string(),
        ));
    }

    let mut parser = Parser::new(config);
    for name in parser.diagnostics.unknown_allowed.clone() {
        let known: Vec<&str> = Category::ALL.iter().map(|c| c.as_str()).collect();
        parser.diagnostics.add(
            Category::Structure,
            format!(
                "ignoring unknown allow_incomplete category {name} (known: {})",
                known.join(", ")
            ),
            None,
            None,
        );
    }

    for path in &files {
        let text = read(path)?;
        let name = path.to_string_lossy().to_string();
        parser.parse_source(&name, &text);
    }
    let pages = parser.config.items("manual").to_vec();
    for (name, path) in pages {
        let text = read(Path::new(&path))?;
        parser.parse_manual(&name, &path, &text);
    }

    parser.bind_aliases();
    parser.assign_ids();
    parser.validate_enums();

    let toprefs = prerender::process(&mut parser);
    let document = render::json::render(&mut parser, &toprefs);

    let out = out_path(options, &parser.config);
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
    }
    std::fs::write(&out, document.write())
        .map_err(|e| Error::Io(format!("{}: {e}", out.display())))?;

    if let Some(path) = &options.diagnostics_json {
        write_diagnostics(&parser, path, options.diagnostics_root.as_deref())?;
    }

    Ok(Outcome {
        exit_code: parser.diagnostics.exit_code(),
        summary: parser.diagnostics.summary(),
        output: out,
    })
}

fn read(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|e| Error::Io(format!("{}: {e}", path.display())))
}

fn build_config(options: &Options) -> Result<Config, Error> {
    let mut config = match &options.config {
        Some(path) => {
            let text = read(path)?;
            Config::parse(&text).map_err(|e| Error::Config(format!("{}: {e}", path.display())))?
        }
        None => Config::default(),
    };
    config.add_section("project");
    config.add_section("manual");
    if !options.files.is_empty() {
        config.set("project", "files", options.files.join("\n"));
    }
    if options.nofollow {
        config.set("project", "follow", "false");
    }
    for (key, value) in [
        ("name", &options.name),
        ("out", &options.out),
        ("snippet_path", &options.snippet_path),
        ("allow_incomplete", &options.allow_incomplete),
    ] {
        if let Some(value) = value {
            config.set("project", key, value.clone());
        }
    }
    for spec in &options.manual {
        let Some((id, path)) = spec.split_once('=') else {
            return Err(Error::Config(format!(
                "--manual takes id=filename, not \"{spec}\""
            )));
        };
        config.set("manual", id, path);
    }
    Ok(config)
}

/// Expands the `files` option: one or more globs per line, each optionally prefixed with
/// a module alias.
fn input_files(config: &Config) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in config.get("project", "files").unwrap_or("").trim().lines() {
        for spec in util::shlex_split(line) {
            // `alias=path`, where an alias may not contain a path separator.
            let pattern = match spec.split_once('=') {
                Some((alias, rest)) if !alias.contains('/') && !alias.contains('\\') => {
                    rest.to_string()
                }
                _ => spec,
            };
            let Ok(entries) = glob::glob(&pattern) else {
                continue;
            };
            let mut found: Vec<PathBuf> = entries.filter_map(Result::ok).collect();
            found.sort();
            for path in found {
                let path = std::fs::canonicalize(&path).unwrap_or(path);
                let path = strip_verbatim(&path);
                if seen.insert(path.clone()) {
                    out.push(path);
                }
            }
        }
    }
    out
}

/// Windows canonicalisation returns a `\\?\` path, which is not what a diagnostic should
/// quote back at a reader.
fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

fn out_path(options: &Options, config: &Config) -> PathBuf {
    let configured = options
        .out
        .clone()
        .or_else(|| config.get("project", "out").map(str::to_string))
        .or_else(|| config.get("project", "outdir").map(str::to_string));
    let Some(dst) = configured else {
        return PathBuf::from("./luadox.json");
    };
    let path = PathBuf::from(&dst);
    if path.is_file() || dst.ends_with(".json") {
        path
    } else {
        path.join("luadox.json")
    }
}

/// Writes every diagnostic as JSON, so a run can be compared against another
/// implementation's. Paths are relative to `root` with forward slashes, so the dump does
/// not name the machine it was produced on.
fn write_diagnostics(parser: &Parser, path: &Path, root: Option<&Path>) -> Result<(), Error> {
    let base = root
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    let base = std::fs::canonicalize(&base).unwrap_or(base);

    let mut records: Vec<(String, String, i64, String, Json)> = Vec::new();
    for entry in parser.diagnostics.entries() {
        let file = entry.file.as_ref().map(|f| relative(f, &base));
        let mut record = Json::obj();
        record.set("category", entry.category.as_str().into());
        record.set(
            "file",
            match &file {
                Some(f) => Json::Str(f.clone()),
                None => Json::Str(String::new()),
            },
        );
        record.set(
            "line",
            entry
                .line
                .map(|l| Json::Int(l as i64))
                .unwrap_or(Json::Int(0)),
        );
        record.set("message", entry.message.clone().into());
        records.push((
            entry.category.as_str().to_string(),
            file.unwrap_or_default(),
            entry.line.unwrap_or(0) as i64,
            entry.message.clone(),
            record,
        ));
    }
    records.sort_by(|a, b| (&a.0, &a.1, a.2, &a.3).cmp(&(&b.0, &b.1, b.2, &b.3)));

    let mut payload = Json::obj();
    payload.set(
        "allowed",
        Json::Arr(
            parser
                .diagnostics
                .allowed()
                .map(|c| Json::Str(c.as_str().to_string()))
                .collect(),
        ),
    );
    payload.set(
        "diagnostics",
        Json::Arr(records.into_iter().map(|r| r.4).collect()),
    );
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
    }
    std::fs::write(path, payload.write() + "\n")
        .map_err(|e| Error::Io(format!("{}: {e}", path.display())))
}

fn relative(file: &str, base: &Path) -> String {
    let path = PathBuf::from(file);
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let absolute = std::fs::canonicalize(&absolute).unwrap_or(absolute);
    let absolute = strip_verbatim(&absolute);
    let base = strip_verbatim(base);
    relative_to(&absolute, &base).replace('\\', "/")
}

/// `os.path.relpath`: how to get from `base` to `path`, with `..` where needed.
fn relative_to(path: &Path, base: &Path) -> String {
    let path: Vec<_> = path.components().collect();
    let base: Vec<_> = base.components().collect();
    let shared = path
        .iter()
        .zip(base.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<String> = base
        .get(shared..)
        .unwrap_or(&[])
        .iter()
        .map(|_| "..".to_string())
        .collect();
    parts.extend(
        path.get(shared..)
            .unwrap_or(&[])
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().to_string()),
    );
    if parts.is_empty() {
        return ".".to_string();
    }
    parts.join("/")
}

use crate::json::Json;
