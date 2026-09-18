//! Phase 1 parser spike: parse the whole production Lua corpus with tree-sitter-lua and
//! report what it found, so the result can be compared against the Python oracle's
//! declaration dump.
//!
//! Not a luadox. It answers three questions and nothing else:
//!   * does the grammar parse the corpus without errors,
//!   * can a doc comment be attached to the declaration it documents,
//!   * what does that cost in build time and run time.

use luadox_spike::lua;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;

#[derive(Serialize)]
struct Report {
    files: usize,
    bytes: usize,
    lines: usize,
    parse_seconds: f64,
    files_with_errors: Vec<FileErrors>,
    totals: Totals,
    declarations: Vec<lua::Decl>,
}

#[derive(Serialize)]
struct FileErrors {
    file: String,
    errors: Vec<(usize, String, String)>,
}

#[derive(Serialize, Default)]
struct Totals {
    doc_blocks: usize,
    attached: usize,
    unattached: usize,
    attached_across_gap: usize,
    collections: usize,
    error_nodes: usize,
    missing_nodes: usize,
    by_kind: BTreeMap<String, usize>,
}

fn collect(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    // The corpus config's five globs, in its order.
    let subdirs = [
        "",
        "autogen",
        "autogen/metadata",
        "autogen/enums",
        "math-docs",
    ];
    let mut files = Vec::new();
    for sub in subdirs {
        let dir = if sub.is_empty() {
            root.to_path_buf()
        } else {
            root.join(sub)
        };
        let mut here: Vec<PathBuf> = std::fs::read_dir(&dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "lua"))
            .collect();
        here.sort();
        files.extend(here);
    }
    Ok(files)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: luadox-spike <lua-src-dir> <out.json>";
    let root = PathBuf::from(args.next().ok_or(usage)?);
    let out = PathBuf::from(args.next().ok_or(usage)?);
    // Report paths the way the oracle does: relative to lua/src's
    // great-grandparent, so the two dumps share one identity for a file.
    let relative_to = root
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.clone());

    let files = collect(&root)?;
    let mut parser = lua::LuaParser::new()?;
    let mut report = Report {
        files: files.len(),
        bytes: 0,
        lines: 0,
        parse_seconds: 0.0,
        files_with_errors: Vec::new(),
        totals: Totals::default(),
        declarations: Vec::new(),
    };

    let sources: Vec<(String, String)> = files
        .iter()
        .map(|path| {
            let name = path
                .strip_prefix(&relative_to)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(path).unwrap_or_default();
            (name, text)
        })
        .collect();

    let started = Instant::now();
    for (name, source) in &sources {
        report.bytes += source.len();
        report.lines += source.lines().count();
        let (decls, stats) = parser.declarations(name, source)?;
        if !stats.errors.is_empty() {
            report.files_with_errors.push(FileErrors {
                file: name.clone(),
                errors: stats.errors.clone(),
            });
        }
        report.totals.doc_blocks += stats.doc_blocks;
        report.totals.attached += stats.attached;
        report.totals.unattached += stats.unattached;
        report.totals.attached_across_gap += stats.attached_across_gap;
        report.totals.collections += stats.collections;
        report.totals.error_nodes += stats.error_nodes;
        report.totals.missing_nodes += stats.missing_nodes;
        report.declarations.extend(decls);
    }
    report.parse_seconds = started.elapsed().as_secs_f64();

    for decl in &report.declarations {
        let key = decl.kind.as_str().to_string();
        *report.totals.by_kind.entry(key).or_default() += 1;
    }
    report
        .declarations
        .sort_by(|a, b| (&a.file, a.line, &a.symbol).cmp(&(&b.file, b.line, &b.symbol)));

    println!(
        "{} files, {} lines, {:.3} s ({:.0} files/s)",
        report.files,
        report.lines,
        report.parse_seconds,
        report.files as f64 / report.parse_seconds.max(f64::MIN_POSITIVE)
    );
    println!(
        "  {} doc blocks: {} attached, {} unattached ({} would attach across a blank line)",
        report.totals.doc_blocks,
        report.totals.attached,
        report.totals.unattached,
        report.totals.attached_across_gap
    );
    println!(
        "  {} ERROR nodes, {} MISSING nodes in {} files",
        report.totals.error_nodes,
        report.totals.missing_nodes,
        report.files_with_errors.len()
    );
    for (kind, n) in &report.totals.by_kind {
        println!("  {n} {kind}");
    }

    let mut f = std::fs::File::create(&out)?;
    serde_json::to_writer_pretty(&mut f, &report)?;
    f.write_all(b"\n")?;
    println!("  -> {}", out.display());
    Ok(())
}
