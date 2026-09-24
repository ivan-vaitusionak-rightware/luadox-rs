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

use full_moon::ast::punctuated::Punctuated;
use full_moon::ast::{
    Assignment, Expression, Field, FunctionBody, FunctionDeclaration, Index, LocalAssignment,
    LocalFunction, Suffix, Var,
};
use full_moon::node::Node;
use full_moon::tokenizer::{Position, Token, TokenType};
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

/// One line of a source file, in the views the scanner reads it.
#[derive(Debug)]
pub struct Line {
    /// The line, stripped, as the Python's scanner sees it.
    pub text: String,
    /// The same line with comments and string literals blanked out, so counting `{` and
    /// `}` cannot be fooled by a brace inside a string -- the Python's own FIXME #2.
    pub code: String,
    /// Whether the line lies inside a `--[[ ]]` long comment. The Python has no notion of
    /// one, which is how it documents a function that is commented out.
    pub in_long_comment: bool,
}

#[derive(Debug)]
pub struct SourceFile {
    pub path: String,
    /// Line `n` is index `n - 1`.
    pub lines: Vec<Line>,
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
    pub fn line(&self, n: u32) -> Option<&Line> {
        self.lines.get(n.checked_sub(1)? as usize)
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
    let result = full_moon::parse_fallible(&source, full_moon::LuaVersion::new());
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

    let lines = source
        .lines()
        .zip(mask(&source, &spans.blank))
        .zip(1u32..)
        .map(|((text, code), n)| Line {
            text: text.trim().to_string(),
            code,
            in_long_comment: spans.long_comment.contains(&n),
        })
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
        self.text_between(start, end)
    }

    fn text_between(&self, start: Position, end: Position) -> Option<String> {
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

    fn add_assignment(&mut self, line: u32, symbol: String, value: Option<String>) {
        self.assignments
            .entry(line)
            .or_insert(FieldDecl { symbol, value });
    }

    /// The literal a field is assigned: the whole right-hand side, several expressions
    /// included, which is what the Python takes after the `=`.
    fn value_text(&self, expressions: &Punctuated<Expression>) -> Option<String> {
        let first = expressions.iter().next()?;
        // `X = function(a, b)` is a field with no value in the Python, because
        // `_parse_field` matches the assignment first and then refuses a value that
        // starts with `function`.
        if matches!(first, Expression::Function(_)) {
            return None;
        }
        let last = expressions.iter().last()?;
        let (start, _) = first.range()?;
        let (_, end) = last.range()?;
        self.text_between(start, end)
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

    // Of several targets, the one written last is the field: the Python's regex takes
    // the name directly before the `=`, so `local_var, self.field = f()` documents
    // `self.field`.
    fn visit_assignment(&mut self, node: &Assignment) {
        let Some(var) = node.variables().iter().last() else {
            return;
        };
        let Some(pos) = var.start_position() else {
            return;
        };
        let Some(name) = self.var_name(var) else {
            return;
        };
        let value = self.value_text(node.expressions());
        self.add_assignment(pos.line() as u32, name, value);
    }

    fn visit_local_assignment(&mut self, node: &LocalAssignment) {
        let Some(name) = node.names().iter().last() else {
            return;
        };
        let Some(pos) = node.local_token().start_position() else {
            return;
        };
        let line = pos.line() as u32;
        let value = self.value_text(node.expressions());
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
        let value = match value {
            Expression::Function(_) => None,
            value => self.text(value),
        };
        self.add_assignment(line, name, value);
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
        let in_long_comment = |n: u32| file.line(n).is_some_and(|l| l.in_long_comment);
        assert!(in_long_comment(2) && in_long_comment(3));
        assert!(!in_long_comment(5));
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
        assert_eq!(file.line(1).map(|l| l.code.as_str()), Some("local s ="));
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

    /// The Python's `_parse_field` regex takes the name directly before the `=` and
    /// everything after it.
    #[test]
    fn a_multiple_assignment_documents_the_last_target_with_the_whole_right_hand_side() {
        let Some(d) = field("notes, self.root = analyze(notes)\n", 1) else {
            panic!("an assignment is a declaration");
        };
        assert_eq!(d.symbol, "self.root");
        assert_eq!(d.value.as_deref(), Some("analyze(notes)"));
        let Some(d) = field("local first, second = 1, 2\n", 1) else {
            panic!("a local assignment is a declaration");
        };
        assert_eq!(d.symbol, "second");
        assert_eq!(d.value.as_deref(), Some("1, 2"));
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
