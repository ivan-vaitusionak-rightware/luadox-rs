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

pub mod assets;
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
pub mod settings;
pub mod tags;
pub mod util;

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::config::Config;
use crate::diag::Category;
use crate::parse::Parser;
use crate::settings::Settings;

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
            Self::Config(msg) => write!(f, "{msg}"),
            Self::NoInput => write!(
                f,
                "no input files or directories specified on command line or config file"
            ),
            Self::UnknownRenderer(name) => {
                let valid: Vec<&str> = Renderer::ALL.iter().map(|r| r.as_str()).collect();
                write!(
                    f,
                    "unknown renderer \"{name}\", valid types are: {}",
                    valid.join(", ")
                )
            }
            Self::Io(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error {}

/// The output formats. A name that is not one of these is an error at the edge -- the
/// command line or the config file -- so no string reaches the pipeline that could fall
/// through to the wrong renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    Html,
    Json,
    Luals,
}

impl Renderer {
    pub const ALL: [Self; 3] = [Self::Html, Self::Json, Self::Luals];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Json => "json",
            Self::Luals => "luals",
        }
    }

    /// The suffix of the file a run writes; the html renderer writes a directory, not a
    /// file.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Html => "",
            Self::Json => ".json",
            Self::Luals => ".lua",
        }
    }
}

impl FromStr for Renderer {
    type Err = Error;

    fn from_str(name: &str) -> Result<Self, Error> {
        Self::ALL
            .into_iter()
            .find(|r| r.as_str() == name)
            .ok_or_else(|| Error::UnknownRenderer(name.to_string()))
    }
}

#[derive(Debug, Default)]
pub struct Options {
    pub config: Option<PathBuf>,
    pub files: Vec<String>,
    pub renderer: Option<Renderer>,
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
    let settings = Settings::from_config(&config)?;
    let renderer = settings.renderer;

    let files = input_files(&settings.files);
    if files.is_empty() {
        return Err(Error::NoInput);
    }

    let encoding = &settings.encoding;
    if !matches!(encoding.to_ascii_lowercase().as_str(), "utf8" | "utf-8") {
        return Err(Error::Config(format!(
            "encoding \"{encoding}\" is not supported; luadox reads utf-8"
        )));
    }
    if settings.follow && !options.nofollow {
        return Err(Error::Config(
            "follow = true is not supported: set follow = false, or list every file".to_string(),
        ));
    }

    let mut parser = Parser::new(settings);
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
    let pages = parser.settings.manual.clone();
    for (name, path) in pages {
        let text = read(Path::new(&path))?;
        parser.parse_manual(&name, &path, &text);
    }

    parser.bind_aliases();
    parser.assign_ids();
    parser.validate_enums();

    let toprefs = prerender::process(&mut parser);
    let document = match renderer {
        Renderer::Html => {
            let out = parser.settings.html_out_dir();
            let written = write_html(&mut parser, &toprefs, &out)?;
            if let Some(path) = &options.diagnostics_json {
                write_diagnostics(&parser, path, options.diagnostics_root.as_deref())?;
            }
            return Ok(Outcome {
                exit_code: parser.diagnostics.exit_code(),
                summary: parser.diagnostics.summary(),
                output: out.join(format!("{written} files")),
            });
        }
        Renderer::Luals => render::luals::render(&mut parser, &toprefs).map_err(Error::Config)?,
        Renderer::Json => render::json::render(&mut parser, &toprefs).write(),
    };

    let out = parser.settings.out_path(renderer.extension());
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
    }
    // LF unconditionally. The Python opens its output in text mode, so on Windows the
    // same run writes CRLF and the recorded bytes become a function of the host; the
    // fixtures store LF and the comparison normalises. See spec/html.md section 11.
    std::fs::write(&out, document).map_err(|e| Error::Io(format!("{}: {e}", out.display())))?;

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
    if let Some(renderer) = options.renderer {
        config.set("project", "renderer", renderer.as_str());
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

/// Expands the `files` globs.
fn input_files(patterns: &[String]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pattern in patterns {
        let Ok(entries) = glob::glob(pattern) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries.filter_map(Result::ok).collect();
        // Case-insensitive, then exact: deterministic, and the order the Python happens
        // to produce on Windows.
        //
        // `glob.glob` does not sort at all -- it returns `os.scandir` order, which on
        // NTFS is case-insensitive alphabetical and on ext4 is hash order. So the order
        // files are read in, which decides the module list in a sidebar, the order of
        // the search index and the previous/next chain, is a property of the machine
        // that built the docs. Sorting here makes it a property of the input.
        found.sort_by(|a, b| {
            let key = |p: &PathBuf| p.to_string_lossy().to_lowercase();
            key(a).cmp(&key(b)).then_with(|| a.cmp(b))
        });
        for path in found {
            let path = std::fs::canonicalize(&path).unwrap_or(path);
            let path = strip_verbatim(&path);
            if seen.insert(path.clone()) {
                out.push(path);
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

/// Renders and writes the whole site, returning how many files it wrote.
fn write_html(
    parser: &mut Parser,
    toprefs: &[crate::ir::ItemId],
    out: &Path,
) -> Result<usize, Error> {
    std::fs::create_dir_all(out).map_err(|e| Error::Io(format!("{}: {e}", out.display())))?;

    // The files project.css, project.js and project.favicon name, copied flat. One that
    // does not exist is reported and skipped, and the run continues -- but its tag is
    // still emitted on every page, which is why the shipped production docs carry a dangling
    // custom-styles-lua.css.
    let (found, missing) = render::html::config_files(parser);
    for name in missing {
        parser.diagnostics.add(
            Category::Structure,
            format!("file \"{name}\" does not exist, skipping"),
            None,
            None,
        );
    }
    for path in found {
        if let Some(name) = path.file_name() {
            std::fs::copy(&path, out.join(name))
                .map_err(|e| Error::Io(format!("{}: {e}", path.display())))?;
        }
    }

    let pages = render::html::render(parser, toprefs).map_err(Error::Config)?;
    write_pages(&pages, out)?;
    Ok(pages.len() + 1)
}

/// Writes the rendered pages from up to four threads. Closing a freshly written file is
/// where antivirus filter drivers and remote filesystems make the caller wait, and the
/// pages are independent, so overlapping the writes hides that wait. It is the same wait
/// whatever the core count, so four writers cover it on any machine. Every chunk runs to
/// completion before the first error is returned.
fn write_pages(pages: &[render::html::Output], out: &Path) -> Result<(), Error> {
    let write_page = |page: &render::html::Output| -> Result<(), Error> {
        let target = out.join(&page.path);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        }
        // LF unconditionally, as spec/html.md section 11 recommends: the Python writes
        // through text mode, so its bytes are a property of the host.
        std::fs::write(&target, &page.bytes)
            .map_err(|e| Error::Io(format!("{}: {e}", target.display())))
    };
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(4)
        .min(pages.len())
        .max(1);
    let chunk = pages.len().div_ceil(threads).max(1);
    let results: Vec<Result<(), Error>> = std::thread::scope(|scope| {
        let handles: Vec<_> = pages
            .chunks(chunk)
            .map(|part| scope.spawn(move || part.iter().try_for_each(&write_page)))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err(Error::Io("writer thread panicked".into())))
            })
            .collect()
    });
    results.into_iter().collect()
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
