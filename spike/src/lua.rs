//! Finds documented declarations in a Lua source file using tree-sitter.
//!
//! This is the one question Phase 1 exists to answer: can a real parser attach a doc
//! comment to the declaration it documents, across the whole corpus? The Python does it
//! by scanning lines, which is why a `--` inside a string truncates a value and a
//! `--[[ ]]` block can declare a phantom function. Here a comment is a node and a
//! declaration is a node, so neither defect is expressible.
//!
//! What is deliberately *not* here: scopes, names, `@within`, inheritance, content. A
//! declaration's identity in this spike is (file, line, kind, local symbol), which is
//! everything a parser can decide on its own and nothing a resolver would.

use std::collections::BTreeMap;

use serde::Serialize;
use tree_sitter::{Node, Parser, Tree};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Class,
    Module,
    Section,
    Table,
    Function,
    Field,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
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

/// One declaration, keyed the way the Python oracle keys its own: for a function or a
/// field the line is the line of the *code*; for a collection it is the line of the tag.
#[derive(Debug, Clone, Serialize)]
pub struct Decl {
    pub file: String,
    pub line: usize,
    pub kind: Kind,
    /// As written, so `Class:method` keeps its colon.
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_enum: bool,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct FileStats {
    pub doc_blocks: usize,
    /// Doc blocks that named a collection (`@class`, `@table`, `@section`, ...).
    pub collections: usize,
    /// Doc blocks whose next line is a declaration this pass understood.
    pub attached: usize,
    /// Doc blocks followed immediately by a line that is not a declaration.
    pub unattached: usize,
    /// Doc blocks that would attach if a blank line between comment and code were
    /// tolerated: measures what the Python's strict next-line rule costs.
    pub attached_across_gap: usize,
    pub error_nodes: usize,
    pub missing_nodes: usize,
    /// (line, node kind, the source text tree-sitter could not fit) for each one.
    pub errors: Vec<(usize, String, String)>,
}

/// A run of comment lines opened by `---`, with whatever collection tag it carries.
struct DocBlock {
    start_line: usize,
    end_line: usize,
    collection: Option<(Kind, String, bool)>,
    /// `@class`, `@table` and `@enum` document the construct *below* them, so the
    /// following code line is not a separate declaration. `@module` and `@section` do
    /// not suppress it, which is the Python's behaviour and not a defect.
    suppress_code_line: bool,
}

pub struct LuaParser {
    parser: Parser,
}

impl LuaParser {
    pub fn new() -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_lua::LANGUAGE.into())
            .map_err(|e| format!("loading the Lua grammar: {e}"))?;
        Ok(Self { parser })
    }

    pub fn parse(&mut self, source: &str) -> Result<Tree, String> {
        self.parser
            .parse(source, None)
            .ok_or_else(|| "parser returned no tree".to_string())
    }

    pub fn declarations(
        &mut self,
        file: &str,
        source: &str,
    ) -> Result<(Vec<Decl>, FileStats), String> {
        let tree = self.parse(source)?;
        let mut stats = FileStats::default();
        count_errors(tree.root_node(), source, &mut stats);

        // One walk collects every construct that could be a declaration, keyed by the
        // line it starts on; attaching a doc block is then a lookup, not a second walk.
        let by_line = declarations_by_line(&tree, source);
        let blocks = doc_blocks(&tree, source);
        stats.doc_blocks = blocks.len();

        let mut decls = Vec::new();
        for block in &blocks {
            if let Some((kind, name, is_enum)) = &block.collection {
                stats.collections += 1;
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
            match by_line
                .range(block.end_line + 1..=block.end_line + 1)
                .next()
            {
                Some((line, found)) => {
                    stats.attached += 1;
                    decls.push(Decl {
                        file: file.to_string(),
                        line: *line,
                        kind: found.kind,
                        symbol: found.symbol.clone(),
                        value: found.value.clone(),
                        args: found.args.clone(),
                        is_enum: false,
                    });
                }
                None => {
                    stats.unattached += 1;
                    if by_line
                        .range(block.end_line + 1..block.end_line + 9)
                        .next()
                        .is_some()
                    {
                        stats.attached_across_gap += 1;
                    }
                }
            }
        }
        Ok((decls, stats))
    }
}

fn count_errors(node: Node, source: &str, stats: &mut FileStats) {
    let mut cursor = node.walk();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if n.is_error() || n.is_missing() {
            if n.is_error() {
                stats.error_nodes += 1;
            } else {
                stats.missing_nodes += 1;
            }
            let snippet = text(n, source).unwrap_or("").trim();
            stats.errors.push((
                n.start_position().row + 1,
                n.kind().to_string(),
                snippet.chars().take(90).collect(),
            ));
            continue;
        }
        if n.has_error() {
            stack.extend(n.children(&mut cursor));
        }
    }
}

/// True for a line comment that opens a luadox doc block: `---`, a run of dashes alone
/// on the line, or `---x` where x is not another dash. Mirrors `re_start_comment_block`.
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
    // `--[[` and `--[=[` open a long comment; anything else starting `--` is a line
    // comment. The Python has no such distinction, which is defect (b).
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
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('@')?;
    // `@{ref}` is a cross reference, not a tag.
    if rest.starts_with('{') {
        return None;
    }
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let (name, args) = rest.split_at(end);
    Some((name, args.trim()))
}

fn doc_blocks(tree: &Tree, source: &str) -> Vec<DocBlock> {
    let mut comments: Vec<(usize, &str)> = Vec::new();
    let mut cursor = tree.walk();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "comment" {
            let text = node.utf8_text(source.as_bytes()).unwrap_or("").trim_start();
            comments.push((node.start_position().row + 1, text));
        } else {
            stack.extend(node.children(&mut cursor));
        }
    }
    comments.sort_unstable_by_key(|&(line, _)| line);

    let mut blocks: Vec<DocBlock> = Vec::new();
    let mut open: Option<DocBlock> = None;
    for &(line, text) in &comments {
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

#[derive(Debug, Clone)]
struct Found {
    kind: Kind,
    symbol: String,
    value: Option<String>,
    args: Option<Vec<String>>,
}

fn declarations_by_line(tree: &Tree, source: &str) -> BTreeMap<usize, Found> {
    let mut out: BTreeMap<usize, Found> = BTreeMap::new();
    let mut cursor = tree.walk();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "comment" {
            continue;
        }
        if let Some((line, found)) = extract(node, source) {
            // The outermost construct on a line wins, which is the one whose text the
            // Python's line regex would have matched first.
            out.entry(line).or_insert(found);
        }
        stack.extend(node.children(&mut cursor));
    }
    out
}

fn text<'a>(node: Node, source: &'a str) -> Option<&'a str> {
    node.utf8_text(source.as_bytes()).ok()
}

fn extract(node: Node, source: &str) -> Option<(usize, Found)> {
    let line = node.start_position().row + 1;
    match node.kind() {
        "function_declaration" => {
            let name = node.child_by_field_name("name")?;
            let args = node
                .child_by_field_name("parameters")
                .map(|p| parameter_names(p, source))
                .unwrap_or_default();
            Some((
                line,
                Found {
                    kind: Kind::Function,
                    symbol: text(name, source)?.to_string(),
                    value: None,
                    args: Some(args),
                },
            ))
        }
        "assignment_statement" => {
            let name = node
                .named_child(0)
                .filter(|n| n.kind() == "variable_list")
                .and_then(|l| l.named_child(0))?;
            let value = node
                .named_child(1)
                .filter(|n| n.kind() == "expression_list")
                .and_then(|l| l.named_child(0));
            Some((line, finish(text(name, source)?, value, source)))
        }
        // `local x = ...` wraps an assignment_statement, which the walk reaches on its
        // own; nothing to do here.
        "field" => {
            let name = node.child_by_field_name("name")?;
            let value = node.child_by_field_name("value");
            Some((line, finish(text(name, source)?, value, source)))
        }
        _ => None,
    }
}

fn finish(name: &str, value: Option<Node>, source: &str) -> Found {
    let symbol = name.trim_matches(['[', ']', '"', '\'']).to_string();
    match value {
        Some(v) if v.kind() == "function_definition" => Found {
            kind: Kind::Function,
            symbol,
            value: None,
            args: Some(
                v.child_by_field_name("parameters")
                    .map(|p| parameter_names(p, source))
                    .unwrap_or_default(),
            ),
        },
        Some(v) => Found {
            kind: Kind::Field,
            symbol,
            // Multi-line values are reported whole: unlike the line scanner, the parser
            // knows where the expression ends.
            value: text(v, source).map(collapse),
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

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parameter_names(params: Node, source: &str) -> Vec<String> {
    let mut cursor = params.walk();
    params
        .named_children(&mut cursor)
        .filter_map(|c| text(c, source).map(str::to_string))
        .collect()
}
