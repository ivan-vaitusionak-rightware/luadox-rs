//! What a real parser knows about one Lua source file.
//!
//! The Python decides everything by scanning lines with regular expressions, which is
//! why a `--` inside a string truncates a value, a `--[[ ]]` block can declare a phantom
//! function, and `for ... do` is counted as two open blocks. None of the three is
//! expressible here: a comment is a token, a string is a token, and a declaration is a
//! node.
//!
//! This module answers only what a parser can answer -- at line N, is there a declaration,
//! and what does it say. Scopes, names, `@within` and content are the resolver's, and
//! live in `parse`.

use std::collections::{BTreeMap, HashSet};

use full_moon::ast::{
    Assignment, Expression, Field, FunctionBody, FunctionDeclaration, Index, LocalAssignment,
    LocalFunction, Suffix, Var,
};
use full_moon::node::Node;
use full_moon::tokenizer::{Token, TokenType};
use full_moon::visitors::Visitor;

/// An assignment as the source writes it: the name assigned to, and the literal
/// right-hand side when the Python's scanner would have kept one.
#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub symbol: String,
    pub value: Option<String>,
}

/// A function as the source declares it. `symbol` keeps the colon of `Class:method`,
/// because the colon is what tells the resolver the name is already scoped.
#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub symbol: String,
    pub args: Vec<String>,
}

#[derive(Debug)]
pub struct SourceFile {
    pub path: String,
    /// Every line, stripped, as the Python's scanner sees them. Line `n` is index `n - 1`.
    pub lines: Vec<String>,
    /// The same lines with comments and string literals blanked out, so counting `{` and
    /// `}` cannot be fooled by a brace inside a string -- the Python's own FIXME #2.
    pub code_lines: Vec<String>,
    /// Lines that lie inside a `--[[ ]]` long comment. The Python has no notion of one,
    /// which is how it documents a function that is commented out.
    pub long_comment: Vec<bool>,
    /// Assignments by the line they start on. Kept apart from `functions` because the
    /// Python tries `_parse_field` before `_parse_function` on every code line and then,
    /// for one special case, falls through from the first to the second.
    pub fields: BTreeMap<u32, FieldDecl>,
    pub functions: BTreeMap<u32, FunctionDecl>,
    /// Syntax the parser could not fit, as (line, what). Empty for the whole production corpus
    /// once the preprocessor directives are blanked.
    pub errors: Vec<(u32, String)>,
    pub blanked_directives: usize,
}

impl SourceFile {
    pub fn line(&self, n: u32) -> &str {
        self.lines
            .get(n.saturating_sub(1) as usize)
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn code_line(&self, n: u32) -> &str {
        self.code_lines
            .get(n.saturating_sub(1) as usize)
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn is_long_comment(&self, n: u32) -> bool {
        self.long_comment
            .get(n.saturating_sub(1) as usize)
            .copied()
            .unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Blanks C preprocessor directives, preserving the line count exactly.
///
/// The corpus's Lua sources are run through a C preprocessor before they reach the
/// interpreter, so 40 lines across 8 files are `#ifdef` / `#endif` directives. They are
/// not Lua, and no Lua parser should be asked to make sense of them; the Python's line
/// scanner only survives them because it never parses anything.
///
/// This is a named input stage, not a workaround for a parser. Each directive line is
/// replaced by an empty line so every following line keeps its number, which is the
/// identity a declaration and a diagnostic are reported under. On the pinned corpus it
/// takes the parse from 140 errors in 6 files to none, and changes no declaration.
pub fn blank_preprocessor_directives(source: &str) -> (String, usize) {
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
    // `#!` is a shebang and `#x` is Lua's length operator; a directive is `#<word>`.
    line.trim_start()
        .strip_prefix('#')
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_alphabetic()))
}

pub fn parse(path: &str, source: &str) -> SourceFile {
    let (source, blanked_directives) = blank_preprocessor_directives(source);
    let result = full_moon::parse_fallible(&source, full_moon::LuaVersion::lua51());
    let errors = result
        .errors()
        .iter()
        .map(|e| match e {
            full_moon::Error::AstError(err) => (
                err.range().0.line() as u32,
                format!("unexpected {}", err.token().to_string().trim()),
            ),
            full_moon::Error::TokenizerError(err) => {
                (err.position().line() as u32, err.to_string())
            }
        })
        .collect();
    let ast = result.into_ast();

    let mut spans = SpanCollector {
        blank: Vec::new(),
        long_comment: HashSet::new(),
    };
    spans.visit_ast(&ast);

    let lines: Vec<String> = source.lines().map(|l| l.trim().to_string()).collect();
    let code_lines = mask(&source, &spans.blank);
    let long_comment = (1..=lines.len() as u32)
        .map(|n| spans.long_comment.contains(&n))
        .collect();

    let mut collector = Collector {
        source: &source,
        assignments: BTreeMap::new(),
        functions: BTreeMap::new(),
    };
    collector.visit_ast(&ast);

    SourceFile {
        path: path.to_string(),
        lines,
        code_lines,
        long_comment,
        fields: collector.assignments,
        functions: collector.functions,
        errors,
        blanked_directives,
    }
}

/// Replaces each span with spaces, keeping newlines, then strips each line.
fn mask(source: &str, spans: &[(usize, usize)]) -> Vec<String> {
    let mut bytes = source.as_bytes().to_vec();
    for &(start, end) in spans {
        for i in start..end.min(bytes.len()) {
            if let Some(b) = bytes.get_mut(i) {
                if *b != b'\n' && *b != b'\r' {
                    *b = b' ';
                }
            }
        }
    }
    String::from_utf8_lossy(&bytes)
        .lines()
        .map(|l| l.trim().to_string())
        .collect()
}

/// Collects the byte spans of everything that is not code, plus the lines a long comment
/// covers.
struct SpanCollector {
    blank: Vec<(usize, usize)>,
    long_comment: HashSet<u32>,
}

impl Visitor for SpanCollector {
    fn visit_token(&mut self, token: &Token) {
        let blank = matches!(
            token.token_type(),
            TokenType::SingleLineComment { .. }
                | TokenType::MultiLineComment { .. }
                | TokenType::StringLiteral { .. }
        );
        if !blank {
            return;
        }
        let (start, end) = (token.start_position(), token.end_position());
        self.blank.push((start.bytes(), end.bytes()));
        if matches!(token.token_type(), TokenType::MultiLineComment { .. }) {
            for line in start.line()..=end.line() {
                self.long_comment.insert(line as u32);
            }
        }
    }
}

struct Collector<'a> {
    source: &'a str,
    assignments: BTreeMap<u32, FieldDecl>,
    functions: BTreeMap<u32, FunctionDecl>,
}

impl Collector<'_> {
    /// The source text of a node. Collapsed to one line only when it spans several, so a
    /// value that fits on its line keeps its spacing exactly as written.
    fn text(&self, node: &impl Node) -> Option<String> {
        let (start, end) = node.range()?;
        let raw = self.source.get(start.bytes()..end.bytes())?;
        if raw.contains('\n') {
            Some(raw.split_whitespace().collect::<Vec<_>>().join(" "))
        } else {
            Some(raw.trim().to_string())
        }
    }

    fn parameters(&self, body: &FunctionBody) -> Vec<String> {
        body.parameters()
            .iter()
            .map(|p| p.to_string().trim().to_string())
            .collect()
    }

    fn add_assignment(&mut self, line: u32, symbol: String, value: Option<&Expression>) {
        let value = match value {
            // `X = function(a, b)` is a field with no value in the Python, because
            // `_parse_field` matches the assignment first and then refuses a value that
            // starts with `function`.
            Some(Expression::Function(_)) | None => None,
            Some(expr) => self.text(expr),
        };
        self.assignments
            .entry(line)
            .or_insert(FieldDecl { symbol, value });
    }

    /// The name the Python's `_parse_field` regexes would produce.
    ///
    /// `[expr] = v` is matched first there, so an index at the end of the target names
    /// the field and its quotes are dropped; anything else is the target written out.
    fn var_name(&self, var: &Var) -> Option<String> {
        match var {
            Var::Name(token) => Some(token.token().to_string()),
            Var::Expression(expr) => {
                let last = expr.suffixes().last();
                if let Some(Suffix::Index(Index::Brackets { expression, .. })) = last {
                    return Some(unquote(&self.text(expression)?));
                }
                self.text(var)
            }
            other => self.text(other),
        }
    }
}

fn unquote(s: &str) -> String {
    s.chars().filter(|c| *c != '"' && *c != '\'').collect()
}

impl Visitor for Collector<'_> {
    fn visit_function_declaration(&mut self, node: &FunctionDeclaration) {
        let Some(pos) = node.function_token().start_position() else {
            return;
        };
        let line = pos.line() as u32;
        let decl = FunctionDecl {
            symbol: node.name().to_string().trim().to_string(),
            args: self.parameters(node.body()),
        };
        self.functions.entry(line).or_insert(decl);
    }

    fn visit_local_function(&mut self, node: &LocalFunction) {
        let Some(pos) = node.local_token().start_position() else {
            return;
        };
        let line = pos.line() as u32;
        let decl = FunctionDecl {
            symbol: node.name().token().to_string(),
            args: self.parameters(node.body()),
        };
        self.functions.entry(line).or_insert(decl);
    }

    fn visit_assignment(&mut self, node: &Assignment) {
        let Some(var) = node.variables().iter().next() else {
            return;
        };
        let Some(pos) = var.start_position() else {
            return;
        };
        let Some(name) = self.var_name(var) else {
            return;
        };
        let value = node.expressions().iter().next();
        self.add_assignment(pos.line() as u32, name, value);
    }

    fn visit_local_assignment(&mut self, node: &LocalAssignment) {
        let Some(name) = node.names().iter().next() else {
            return;
        };
        let Some(pos) = node.local_token().start_position() else {
            return;
        };
        let line = pos.line() as u32;
        let value = node.expressions().iter().next();
        self.add_assignment(line, name.token().to_string(), value);
    }

    fn visit_field(&mut self, node: &Field) {
        let (name, value, line) = match node {
            Field::ExpressionKey { key, value, .. } => {
                let Some(pos) = key.start_position() else {
                    return;
                };
                let Some(text) = self.text(key) else { return };
                (unquote(&text), value, pos.line() as u32)
            }
            Field::NameKey { key, value, .. } => (
                key.token().to_string(),
                value,
                key.token().start_position().line() as u32,
            ), // a token always has a position
            _ => return,
        };
        self.add_assignment(line, name, Some(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(source: &str, line: u32) -> Option<FieldDecl> {
        parse("<test>", source).fields.get(&line).cloned()
    }

    /// Defect (a) in the Python: `RE_BLOCK_OPEN` matches both `for` and `do` on one line
    /// against one `end`, so its block depth never returns to zero and the scan leaks
    /// into the next function. A parser has no block depth to leak.
    #[test]
    fn a_for_do_loop_does_not_leak_into_the_next_function() {
        let source = "\
function T:loop(items)
    for i = 1, #items do print(i) end
end
function T:count() return 42 end
";
        let file = parse("<test>", source);
        assert_eq!(
            file.functions.get(&1).map(|d| d.symbol.clone()),
            Some("T:loop".into())
        );
        assert_eq!(
            file.functions.get(&4).map(|d| d.symbol.clone()),
            Some("T:count".into())
        );
        assert!(file.errors.is_empty(), "{:?}", file.errors);
    }

    /// Defect (b): the line scanner has no notion of a long comment, so a declaration
    /// written inside one is documented. Here the lines are marked and the caller skips
    /// them.
    #[test]
    fn a_declaration_inside_a_long_comment_is_marked_as_comment() {
        let source = "\
--[[
--- Ghost.
function L:ghost() end
]]
function L:real() end
";
        let file = parse("<test>", source);
        assert!(file.is_long_comment(2) && file.is_long_comment(3));
        assert!(!file.is_long_comment(5));
        assert_eq!(
            file.functions.get(&5).map(|d| d.symbol.clone()),
            Some("L:real".into())
        );
    }

    /// Defect (c): `strip_trailing_comment` is `re.sub(r'--.*', '', line)`, so a `--`
    /// inside a string truncates the line and the value is lost.
    #[test]
    fn a_double_dash_inside_a_string_does_not_truncate_the_value() {
        assert_eq!(
            field("S.sep = \"a--b\"\n", 1).and_then(|d| d.value),
            Some("\"a--b\"".to_string())
        );
    }

    #[test]
    fn braces_inside_a_string_do_not_count_as_a_table() {
        let file = parse("<test>", "local s = \"{{{\"\n");
        assert_eq!(file.code_line(1).matches('{').count(), 0);
    }

    #[test]
    fn a_value_spanning_lines_survives_whole() {
        let source = "M.X = find(\n    \"a\",\n    \"b\" )\n";
        assert_eq!(
            field(source, 1).and_then(|d| d.value),
            Some("find( \"a\", \"b\" )".to_string())
        );
    }

    #[test]
    fn an_assignment_of_a_function_is_a_field_with_no_value() {
        let file = parse("<test>", "M.f = function(a, b) end\n");
        let Some(d) = file.fields.get(&1) else {
            panic!("an assignment is a declaration");
        };
        assert_eq!(d.symbol, "M.f");
        assert_eq!(d.value, None);
        assert!(!file.functions.contains_key(&1));
    }

    #[test]
    fn a_bracket_key_names_the_field_without_its_quotes() {
        let Some(d) = field("M[\"key\"] = 1\n", 1) else {
            panic!("a bracket key is a declaration");
        };
        assert_eq!(d.symbol, "key");
        assert_eq!(d.value.as_deref(), Some("1"));
    }

    #[test]
    fn preprocessor_directives_are_blanked_and_line_numbers_survive() {
        let source =
            "function f()\n#ifdef DEBUG_BUILD\n    check()\n#endif\nend\nfunction g() end\n";
        let file = parse("<test>", source);
        assert_eq!(file.blanked_directives, 2);
        assert!(file.errors.is_empty(), "{:?}", file.errors);
        assert_eq!(
            file.functions.get(&6).map(|d| d.symbol.clone()),
            Some("g".into())
        );
    }
}
