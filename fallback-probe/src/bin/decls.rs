//! The same declaration extraction the tree-sitter spike performs, on full_moon, so the
//! two parsers can be graded against the same oracle dump with `harness/compare_decls.py`.
//!
//! Phase 1 left one claim unproven: the tree-sitter spike keeps "every declaration around
//! and inside" the corpus's 40 `#ifdef ENGINE_DEBUG` lines, while full_moon was only
//! observed to keep every *top-level* declaration. A lost declaration is a member that
//! silently does not render, so the claim has to be measured, not assumed.
//!
//! Everything here mirrors `spike/src/lua.rs` deliberately, down to the doc-block
//! scanner, so that a difference in the output is a difference between the two parsers
//! and not between two programs.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use full_moon::ast::{Ast, Assignment, Field, FunctionBody, FunctionDeclaration, LocalAssignment,
                     LocalFunction, Var};
use full_moon::node::Node;
use full_moon::tokenizer::Token;
use full_moon::visitors::Visitor;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Class,
    Module,
    Section,
    Table,
    Function,
    Field,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Class => "class",
            Kind::Module => "module",
            Kind::Section => "section",
            Kind::Table => "table",
            Kind::Function => "function",
            Kind::Field => "field",
        }
    }

    fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "class" => Some(Kind::Class),
            "module" => Some(Kind::Module),
            "section" => Some(Kind::Section),
            "table" | "enum" => Some(Kind::Table),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct Decl {
    file: String,
    line: usize,
    kind: Kind,
    symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    is_enum: bool,
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
    collections: usize,
    error_nodes: usize,
    by_kind: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct Report {
    files: usize,
    lines: usize,
    parse_seconds: f64,
    preprocessed_lines: usize,
    files_with_errors: Vec<FileErrors>,
    totals: Totals,
    declarations: Vec<Decl>,
}

/// Blanks C preprocessor directives, preserving the line count exactly.
///
/// The production Lua sources are run through a C preprocessor before they reach the
/// interpreter, so 40 lines across 8 files are `#ifdef ENGINE_DEBUG` / `#endif`. They are
/// not Lua, and no Lua parser should be asked to make sense of them; the line scanner in
/// the Python oracle only survives them because it never parses anything.
///
/// This is a documented input stage, not a workaround for a parser: the directive lines
/// are replaced by empty lines so every subsequent line keeps its number, which is the
/// identity a declaration and a diagnostic are reported under.
fn blank_preprocessor_directives(source: &str) -> (String, usize) {
    if !source.lines().any(is_directive) {
        return (source.to_string(), 0);
    }
    let mut out = String::with_capacity(source.len());
    let mut blanked = 0;
    for (i, line) in source.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if is_directive(line) {
            blanked += 1;
        } else {
            out.push_str(line);
        }
    }
    if source.ends_with('\n') {
        out.push('\n');
    }
    (out, blanked)
}

fn is_directive(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix('#') else {
        return false;
    };
    // `#!` is a shebang and `#x` is Lua's length operator; a directive is `#<word>`.
    rest.starts_with(|c: char| c.is_ascii_alphabetic())
}

/// True for a line comment that opens a luadox doc block. Mirrors `re_start_comment_block`.
fn opens_block(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("---") else {
        return false;
    };
    match rest.chars().next() {
        None => true,
        Some('-') => rest.chars().all(|c| c == '-'),
        Some(_) => true,
    }
}

fn is_line_comment(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("--") else {
        return false;
    };
    match rest.strip_prefix('[') {
        Some(r) => !r.trim_start_matches('=').starts_with('['),
        None => true,
    }
}

/// `-- @tag args` -> ("tag", "args"), matching `RE_COMMENTED_TAG`.
fn parse_tag(text: &str) -> Option<(&str, &str)> {
    let rest = text.trim_start_matches('-');
    if rest.len() == text.len() {
        return None;
    }
    let rest = rest.trim_start().strip_prefix('@')?;
    if rest.starts_with('{') {
        return None;
    }
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let (name, args) = rest.split_at(end);
    Some((name, args.trim()))
}

struct DocBlock {
    start_line: usize,
    end_line: usize,
    collection: Option<(Kind, String, bool)>,
    suppress_code_line: bool,
}

#[derive(Debug, Clone)]
struct Found {
    kind: Kind,
    symbol: String,
    value: Option<String>,
    args: Option<Vec<String>>,
}

/// One walk of the AST collecting both halves of the question: every comment token, and
/// every construct that could be a declaration, keyed by the line it starts on.
struct Collector<'a> {
    source: &'a str,
    comments: Vec<(usize, String)>,
    by_line: BTreeMap<usize, Found>,
}

impl<'a> Collector<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            comments: Vec::new(),
            by_line: BTreeMap::new(),
        }
    }

    fn slice(&self, node: &impl Node) -> Option<String> {
        let (start, end) = node.range()?;
        self.source
            .get(start.bytes()..end.bytes())
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
    }

    fn record(&mut self, line: usize, found: Found) {
        // The outermost construct on a line wins, as in the tree-sitter spike.
        self.by_line.entry(line).or_insert(found);
    }

    fn comment(&mut self, token: &Token) {
        let pos = token.start_position();
        {
            self.comments
                .push((pos.line(), token.to_string().trim_start().to_string()));
        }
    }

    fn parameters(&self, body: &FunctionBody) -> Vec<String> {
        body.parameters()
            .iter()
            .map(|p| p.to_string().trim().to_string())
            .collect()
    }
}

impl Visitor for Collector<'_> {
    fn visit_single_line_comment(&mut self, token: &Token) {
        self.comment(token);
    }

    fn visit_multi_line_comment(&mut self, token: &Token) {
        self.comment(token);
    }

    fn visit_function_declaration(&mut self, node: &FunctionDeclaration) {
        let Some(pos) = node.function_token().start_position() else {
            return;
        };
        let symbol = node.name().to_string().trim().to_string();
        let args = self.parameters(node.body());
        self.record(
            pos.line(),
            Found {
                kind: Kind::Function,
                symbol,
                value: None,
                args: Some(args),
            },
        );
    }

    fn visit_local_function(&mut self, node: &LocalFunction) {
        let Some(pos) = node.local_token().start_position() else {
            return;
        };
        let symbol = node.name().token().to_string();
        let args = self.parameters(node.body());
        self.record(
            pos.line(),
            Found {
                kind: Kind::Function,
                symbol,
                value: None,
                args: Some(args),
            },
        );
    }

    fn visit_assignment(&mut self, node: &Assignment) {
        let Some(var) = node.variables().iter().next() else {
            return;
        };
        let Some(pos) = var.start_position() else {
            return;
        };
        let name = match var {
            Var::Name(token) => token.token().to_string(),
            other => other.to_string().trim().to_string(),
        };
        let value = node.expressions().iter().next();
        let found = self.finish(&name, value.map(|e| (is_function(e), e)));
        self.record(pos.line(), found);
    }

    fn visit_local_assignment(&mut self, node: &LocalAssignment) {
        let Some(name) = node.names().iter().next() else {
            return;
        };
        let Some(pos) = node.local_token().start_position() else {
            return;
        };
        let value = node.expressions().iter().next();
        let found = self.finish(
            &name.token().to_string(),
            value.map(|e| (is_function(e), e)),
        );
        self.record(pos.line(), found);
    }

    fn visit_field(&mut self, node: &Field) {
        let (name, value, start) = match node {
            Field::ExpressionKey { key, value, .. } => {
                (key.to_string().trim().to_string(), value, key.start_position())
            }
            Field::NameKey { key, value, .. } => (
                key.token().to_string(),
                value,
                Some(key.token().start_position()),
            ),
            _ => return,
        };
        let Some(pos) = start else { return };
        let found = self.finish(&name, Some((is_function(value), value)));
        self.record(pos.line(), found);
    }
}

fn is_function(expr: &full_moon::ast::Expression) -> bool {
    matches!(expr, full_moon::ast::Expression::Function(_))
}

impl Collector<'_> {
    fn finish(
        &self,
        name: &str,
        value: Option<(bool, &full_moon::ast::Expression)>,
    ) -> Found {
        let symbol = name.trim_matches(['[', ']', '"', '\'']).to_string();
        match value {
            Some((true, expr)) => {
                let args = match expr {
                    full_moon::ast::Expression::Function(f) => self.parameters(f.body()),
                    _ => Vec::new(),
                };
                Found {
                    kind: Kind::Function,
                    symbol,
                    value: None,
                    args: Some(args),
                }
            }
            Some((false, expr)) => Found {
                kind: Kind::Field,
                symbol,
                value: self.slice(expr),
                args: None,
            },
            None => Found {
                kind: Kind::Field,
                symbol,
                value: None,
                args: None,
            },
        }
    }
}

fn doc_blocks(comments: &[(usize, String)]) -> Vec<DocBlock> {
    let mut sorted: Vec<&(usize, String)> = comments.iter().collect();
    sorted.sort_by_key(|(line, _)| *line);

    let mut blocks: Vec<DocBlock> = Vec::new();
    let mut open: Option<DocBlock> = None;
    for (line, text) in sorted {
        let line = *line;
        let line_comment = is_line_comment(text);
        let continues = open
            .as_ref()
            .is_some_and(|b| line_comment && line == b.end_line + 1);
        if !continues {
            if let Some(block) = open.take() {
                blocks.push(block);
            }
            if !(line_comment && opens_block(text)) {
                continue;
            }
            open = Some(DocBlock {
                start_line: line,
                end_line: line,
                collection: None,
                suppress_code_line: false,
            });
        }
        let Some(block) = open.as_mut() else { continue };
        block.end_line = line;
        if let Some((tag, args)) = parse_tag(text) {
            if let Some(kind) = Kind::from_tag(tag) {
                if block.collection.is_none() {
                    let name = args.split_whitespace().next().unwrap_or("").to_string();
                    block.collection = Some((kind, name, tag == "enum"));
                    block.start_line = line;
                }
                block.suppress_code_line |= matches!(tag, "class" | "table" | "enum");
            }
        }
    }
    if let Some(block) = open.take() {
        blocks.push(block);
    }
    blocks
}

fn declarations(file: &str, source: &str, totals: &mut Totals) -> (Vec<Decl>, Vec<(usize, String, String)>) {
    let result = full_moon::parse_fallible(source, full_moon::LuaVersion::lua51());
    let errors: Vec<(usize, String, String)> = result
        .errors()
        .iter()
        .map(|e| {
            let (line, text) = match e {
                full_moon::Error::AstError(err) => (
                    err.range().0.line(),
                    err.token().to_string().trim().to_string(),
                ),
                full_moon::Error::TokenizerError(err) => {
                    (err.position().line(), err.to_string())
                }
            };
            (line, "error".to_string(), text.chars().take(90).collect())
        })
        .collect();
    let ast: Ast = result.into_ast();

    let mut collector = Collector::new(source);
    collector.visit_ast(&ast);

    let blocks = doc_blocks(&collector.comments);
    totals.doc_blocks += blocks.len();
    totals.error_nodes += errors.len();

    let mut decls = Vec::new();
    for block in &blocks {
        if let Some((kind, name, is_enum)) = &block.collection {
            totals.collections += 1;
            decls.push(Decl {
                file: file.to_string(),
                line: block.start_line,
                kind: *kind,
                symbol: name.clone(),
                value: None,
                args: None,
                is_enum: *is_enum,
            });
        }
        if block.suppress_code_line {
            continue;
        }
        match collector.by_line.get(&(block.end_line + 1)) {
            Some(found) => {
                totals.attached += 1;
                decls.push(Decl {
                    file: file.to_string(),
                    line: block.end_line + 1,
                    kind: found.kind,
                    symbol: found.symbol.clone(),
                    value: found.value.clone(),
                    args: found.args.clone(),
                    is_enum: false,
                });
            }
            None => totals.unattached += 1,
        }
    }
    (decls, errors)
}

fn collect(root: &Path) -> Vec<PathBuf> {
    let subdirs = ["", "autogen", "autogen/metadata", "autogen/enums", "math-docs"];
    let mut files = Vec::new();
    for sub in subdirs {
        let dir = if sub.is_empty() {
            root.to_path_buf()
        } else {
            root.join(sub)
        };
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut here: Vec<PathBuf> = rd
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "lua"))
                .collect();
            here.sort();
            files.extend(here);
        }
    }
    files
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut positional = Vec::new();
    let mut preprocess = false;
    for arg in std::env::args().skip(1) {
        if arg == "--preprocess" {
            preprocess = true;
        } else {
            positional.push(arg);
        }
    }
    let usage = "usage: decls [--preprocess] <lua-src-dir> <out.json>";
    let root = PathBuf::from(positional.first().ok_or(usage)?);
    let out = PathBuf::from(positional.get(1).ok_or(usage)?);
    let relative_to = root
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.clone());

    let files = collect(&root);
    let mut preprocessed_lines = 0;
    let sources: Vec<(String, String)> = files
        .iter()
        .map(|path| {
            let name = path
                .strip_prefix(&relative_to)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(path).unwrap_or_default();
            let text = if preprocess {
                let (text, blanked) = blank_preprocessor_directives(&text);
                preprocessed_lines += blanked;
                text
            } else {
                text
            };
            (name, text)
        })
        .collect();

    let mut report = Report {
        files: files.len(),
        lines: 0,
        parse_seconds: 0.0,
        preprocessed_lines,
        files_with_errors: Vec::new(),
        totals: Totals::default(),
        declarations: Vec::new(),
    };

    let started = Instant::now();
    for (name, source) in &sources {
        report.lines += source.lines().count();
        let (decls, errors) = declarations(name, source, &mut report.totals);
        if !errors.is_empty() {
            report.files_with_errors.push(FileErrors {
                file: name.clone(),
                errors,
            });
        }
        report.declarations.extend(decls);
    }
    report.parse_seconds = started.elapsed().as_secs_f64();

    for decl in &report.declarations {
        *report
            .totals
            .by_kind
            .entry(decl.kind.as_str().to_string())
            .or_default() += 1;
    }
    report
        .declarations
        .sort_by(|a, b| (&a.file, a.line, &a.symbol).cmp(&(&b.file, b.line, &b.symbol)));

    println!(
        "{} files, {} lines, {:.3} s ({} directive lines blanked)",
        report.files, report.lines, report.parse_seconds, report.preprocessed_lines
    );
    println!(
        "  {} doc blocks: {} attached, {} unattached",
        report.totals.doc_blocks, report.totals.attached, report.totals.unattached
    );
    println!(
        "  {} parse errors in {} files",
        report.totals.error_nodes,
        report.files_with_errors.len()
    );
    for (kind, n) in &report.totals.by_kind {
        println!("  {n} {kind}");
    }

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::File::create(&out)?;
    serde_json::to_writer_pretty(&mut f, &report)?;
    f.write_all(b"\n")?;
    println!("  -> {}", out.display());
    Ok(())
}
