//! Block and inline content, as two types rather than one string.
//!
//! This exists because of a real class of bug, found in the shipped production docs. A
//! `@compact` collection has no detail box, so the html renderer puts a member's *entire*
//! documentation into the one-line synopsis cell, while a non-compact row gets only the
//! first sentence and the rest goes to a detail box. When that documentation is a code
//! block, a heading or a list, it is emitted but unreadable: the stylesheet has no rule
//! for a block element inside `div.synopsis td`, so a `<pre>` falls through to
//! `div.body pre { margin: 0 2em }` inside an already-padded cell. On the pinned corpus
//! that is 240 elements over 50 files. Nothing reported it: it looks right in the source
//! and in the html, and wrong only in a browser.
//!
//! The Python can only find it by matching a regular expression against rendered html
//! after the fact. Rust can make it unrepresentable instead:
//!
//!   * [`Inline`] is content a one-line context can lay out. It has no public
//!     constructor.
//!   * [`Block`] is anything a documentation block can produce.
//!   * [`Block::into_inline`] is the only way from one to the other, it is fallible, and
//!     its error names the block kinds that did not fit.
//!   * [`RowContent`] -- what every row-shaped renderer takes -- can only be built by
//!     that conversion, so a code block cannot reach a table cell by any route.
//!
//! The payoff is generality: the next tag combination that puts block content in a
//! one-line context fails to compile, instead of shipping a broken page that somebody
//! notices a year later.

use crate::ir::{Content, Fragment};

/// What a run of markdown lines is, and the html element it becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Code,
    Heading(u8),
    List {
        ordered: bool,
    },
    Quote,
    Table,
    Rule,
    /// Raw html written straight into the documentation, named by its opening tag.
    Html(String),
}

impl BlockKind {
    /// The html element this renders as, for a message that names what went wrong.
    pub fn element(&self) -> String {
        match self {
            Self::Paragraph => "p".to_string(),
            Self::Code => "pre".to_string(),
            Self::Heading(level) => format!("h{level}"),
            Self::List { ordered: true } => "ol".to_string(),
            Self::List { ordered: false } => "ul".to_string(),
            Self::Quote => "blockquote".to_string(),
            Self::Table => "table".to_string(),
            Self::Rule => "hr".to_string(),
            Self::Html(tag) => tag.clone(),
        }
    }

    /// Only a paragraph can be laid out on one line. Everything else needs vertical
    /// space a table cell does not give it.
    pub fn fits_a_row(&self) -> bool {
        *self == Self::Paragraph
    }
}

/// One markdown block, keeping the exact lines it was written as so a renderer emits byte
/// for byte what was parsed.
#[derive(Debug, Clone)]
pub struct Block {
    kind: BlockKind,
    lines: Vec<String>,
}

impl Block {
    pub fn kind(&self) -> &BlockKind {
        &self.kind
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// The only way to get [`Inline`] content, and the single place the invariant is
    /// enforced.
    pub fn into_inline(self) -> Result<Inline, TooBigForARow> {
        if self.kind.fits_a_row() {
            return Ok(Inline(self.text()));
        }
        Err(TooBigForARow {
            kinds: vec![self.kind],
        })
    }
}

impl TryFrom<Block> for Inline {
    type Error = TooBigForARow;

    fn try_from(block: Block) -> Result<Self, TooBigForARow> {
        block.into_inline()
    }
}

/// Content a one-line context can present. No public constructor: the only way in is
/// [`Block::into_inline`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inline(String);

impl Inline {
    pub fn text(&self) -> &str {
        &self.0
    }
}

/// What a row-shaped renderer takes: a synopsis cell, a compact table row, a tooltip, a
/// summary line. Constructible only by converting [`Content`], which is fallible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowContent(Vec<Inline>);

impl RowContent {
    pub fn parts(&self) -> &[Inline] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The text a one-line context renders. Paragraphs are joined by a space, because a
    /// row has no paragraph breaks to give them.
    pub fn text(&self) -> String {
        self.0
            .iter()
            .map(Inline::text)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Documentation that a one-line context cannot present, naming what did not fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TooBigForARow {
    pub kinds: Vec<BlockKind>,
}

impl TooBigForARow {
    /// The offending html elements, sorted and de-duplicated, as a message names them.
    pub fn elements(&self) -> Vec<String> {
        let mut names: Vec<String> = self.kinds.iter().map(BlockKind::element).collect();
        names.sort();
        names.dedup();
        names
    }
}

impl std::fmt::Display for TooBigForARow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{}>", self.elements().join(">, <"))
    }
}

impl std::error::Error for TooBigForARow {}

impl TryFrom<&Content> for RowContent {
    type Error = TooBigForARow;

    fn try_from(content: &Content) -> Result<Self, TooBigForARow> {
        let mut parts = Vec::new();
        let mut rejected: Vec<BlockKind> = Vec::new();
        for block in blocks_of(content) {
            match block.into_inline() {
                Ok(inline) => parts.push(inline),
                Err(e) => rejected.extend(e.kinds),
            }
        }
        if rejected.is_empty() {
            Ok(Self(parts))
        } else {
            Err(TooBigForARow { kinds: rejected })
        }
    }
}

/// Every block in a content tree, descending into an admonition's body because that body
/// is rendered into the same cell.
pub fn blocks_of(content: &Content) -> Vec<Block> {
    let mut out = Vec::new();
    for fragment in &content.0 {
        match fragment {
            Fragment::Markdown(md) => out.extend(split(&md.get())),
            Fragment::Admonition { content, .. } => out.extend(blocks_of(content)),
            // A `@see` list renders as its own styled block, which the compact layout
            // does have a rule for.
            Fragment::SeeAlso(_) => {}
        }
    }
    out
}

/// Splits markdown into blocks.
///
/// Deliberately coarse: block *boundaries* only matter for a paragraph's text, and block
/// *kinds* are what the invariant is about. Indented code blocks do not exist here,
/// because luadox sets `CODE_INDENT` high enough to disable them.
pub fn split(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let Some(line) = lines.get(i) else { break };
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        if let Some(fence) = fence_of(line) {
            let start = i;
            i += 1;
            while let Some(line) = lines.get(i) {
                i += 1;
                if fence_of(line).is_some_and(|f| f == fence) {
                    break;
                }
            }
            out.push(block(BlockKind::Code, &lines, start, i));
            continue;
        }
        let kind = opens(line);
        if matches!(kind, BlockKind::Heading(_) | BlockKind::Rule) {
            out.push(block(kind, &lines, i, i + 1));
            i += 1;
            continue;
        }
        // A run of lines with nothing in it that starts a different block.
        let start = i;
        i += 1;
        while let Some(next) = lines.get(i) {
            if next.trim().is_empty() || fence_of(next).is_some() {
                break;
            }
            let next_kind = opens(next);
            if next_kind != kind && next_kind != BlockKind::Paragraph {
                break;
            }
            i += 1;
        }
        out.push(block(kind, &lines, start, i));
    }
    out
}

fn block(kind: BlockKind, lines: &[&str], start: usize, end: usize) -> Block {
    Block {
        kind,
        lines: lines
            .get(start..end.min(lines.len()))
            .unwrap_or(&[])
            .iter()
            .map(|l| l.to_string())
            .collect(),
    }
}

/// The fence a line opens or closes, if any: three or more backticks or tildes.
fn fence_of(line: &str) -> Option<char> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
    if trimmed.chars().take_while(|c| *c == marker).count() >= 3 {
        Some(marker)
    } else {
        None
    }
}

/// What kind of block this line opens.
fn opens(line: &str) -> BlockKind {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) {
        let rest = trimmed.get(hashes..).unwrap_or("");
        if rest.is_empty() || rest.starts_with(' ') {
            return BlockKind::Heading(hashes as u8);
        }
    }
    if is_rule(trimmed) {
        return BlockKind::Rule;
    }
    if trimmed.starts_with("> ") || trimmed == ">" {
        return BlockKind::Quote;
    }
    if trimmed.starts_with('|') {
        return BlockKind::Table;
    }
    if let Some(rest) = trimmed.strip_prefix(['-', '*', '+']) {
        if rest.starts_with(' ') {
            return BlockKind::List { ordered: false };
        }
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let rest = trimmed.get(digits..).unwrap_or("");
        if rest.starts_with(". ") || rest.starts_with(") ") {
            return BlockKind::List { ordered: true };
        }
    }
    if let Some(tag) = html_block_tag(trimmed) {
        return BlockKind::Html(tag);
    }
    BlockKind::Paragraph
}

/// `---`, `***` or `___` alone on a line.
fn is_rule(line: &str) -> bool {
    let stripped: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    if stripped.len() < 3 {
        return false;
    }
    ['-', '*', '_']
        .into_iter()
        .any(|marker| stripped.chars().all(|c| c == marker))
}

/// The html elements that open a block when written literally in a doc comment. Inline
/// html such as `<b>` is not one of them, and the corpus uses it.
const HTML_BLOCKS: [&str; 9] = [
    "pre",
    "ul",
    "ol",
    "table",
    "blockquote",
    "dl",
    "div",
    "h5",
    "h6",
];

fn html_block_tag(line: &str) -> Option<String> {
    let rest = line.strip_prefix('<')?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    HTML_BLOCKS.contains(&name.as_str()).then_some(name)
}

// ---------------------------------------------------------------------------------
// Rendering
//
// comrak is pinned exactly, like the parser: a markdown library that moves changes every
// page without anybody changing the tool. 0.50 and newer are edition 2024, which needs
// Cargo 1.85 against this workspace's 1.83 pin, so 0.49.0 is the newest that builds.
//
// harness/markdown_parity.py renders every markdown fragment of the corpus through both
// this and the Python's commonmark, and they agree on all 4014 distinct fragments. The
// divergences spec/html.md section 10.2 names are real -- the same harness shows them on
// constructed input -- and the corpus contains none of them. It is the guard that keeps
// that true.

/// The options the oracle's `commonmark` run is equivalent to: raw HTML through, no GFM
/// extensions, no smart punctuation.
fn render_options() -> comrak::Options<'static> {
    let mut options = comrak::Options::default();
    // `autogen/enums/FocusScopeTypeEnums.lua` writes `<b>` in a doc comment, and the
    // oracle passes it through.
    options.render.r#unsafe = true;
    // The Python parses with `commonmark_extensions.tables.ParserWithTables`.
    options.extension.table = true;
    options
}

/// Renders markdown to HTML, rewriting each `luadox:<id>` destination into a real href.
///
/// The Python does the rewrite inside its renderer subclass, at the moment the link is
/// written; here it is a pass over the AST before formatting, which is the same thing said
/// out loud. A destination whose id does not resolve is left as it was: the Python raises
/// `KeyError` there and calls it a parser bug, and a documentation tool should not die
/// rendering a page over one.
pub fn to_html(md: &str, href_for: &dyn Fn(&str) -> Option<String>) -> String {
    let arena = comrak::Arena::new();
    let options = render_options();
    let root = comrak::parse_document(&arena, md, &options);
    for node in root.descendants() {
        let mut ast = node.data.borrow_mut();
        if let comrak::nodes::NodeValue::Link(link) = &mut ast.value {
            if let Some(id) = link.url.strip_prefix("luadox:") {
                if let Some(href) = href_for(id) {
                    link.url = href;
                }
            }
        }
    }
    let mut out = String::new();
    if comrak::format_html(root, &options, &mut out).is_err() {
        return String::new();
    }
    // The Python's table renderer opens every table it parses as `<table class="user">`,
    // and because it writes `</table>` with its own newline, the `cr()` of the block that
    // follows adds a blank line that comrak does not.
    let mut out = out
        .replace("<table>", "<table class=\"user\">")
        .replace("</table>\n", "</table>\n\n");
    if out.ends_with("</table>\n\n") {
        out.pop();
    }
    out
}

/// Strips markdown down to plain text, for the search index.
///
/// The substitutions and their order are the Python's `_markdown_to_text`, which is a
/// sequence of regular expressions over the markdown source rather than anything to do
/// with the parser.
pub fn to_text(md: &str) -> String {
    let text = remove_fenced_blocks(md);
    let text = unwrap_delimited(&text, '`', '`');
    let text = text.replace('#', "");
    let text = unwrap_delimited(&text, '*', '*');
    let text = unwrap_links(&text);
    let text = unwrap_braced_refs(&text);
    collapse_whitespace(&text)
}

/// `re.sub(r'\s+', ' ', text)`: every run of whitespace becomes one space, and nothing
/// is trimmed.
///
/// The trimming is the difference that matters. Every content block ends with the
/// sentinel's empty line, so a fragment flattens to text with a trailing space, and
/// `_content_to_text` then joins the fragments with a newline which the search index turns
/// into another space. That is where the *two* spaces before an admonition's title in
/// `index.js` come from, and trimming here loses them.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// ```` ```.*?``` ```` with DOTALL: the shortest run between two triple-backtick marks.
fn remove_fenced_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("```") {
        let after = rest.get(open + 3..).unwrap_or("");
        match after.find("```") {
            Some(close) => {
                out.push_str(rest.get(..open).unwrap_or(""));
                rest = after.get(close + 3..).unwrap_or("");
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// `` `([^`]+)` `` and `\*([^*]+)\*`: the delimiters removed, the text kept.
fn unwrap_delimited(text: &str, open: char, close: char) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while let Some(&c) = chars.get(i) {
        if c != open {
            out.push(c);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut inner = String::new();
        while let Some(&c) = chars.get(j) {
            if c == close {
                break;
            }
            inner.push(c);
            j += 1;
        }
        if inner.is_empty() || chars.get(j) != Some(&close) {
            out.push(c);
            i += 1;
            continue;
        }
        out.push_str(&inner);
        i = j + 1;
    }
    out
}

/// `!?\[([^]]*)\]\([^)]+\)`: a link or an inline image reduced to its text.
fn unwrap_links(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while let Some(&c) = chars.get(i) {
        let bang = c == '!' && chars.get(i + 1) == Some(&'[');
        let open = if bang { i + 1 } else { i };
        if chars.get(open) != Some(&'[') {
            out.push(c);
            i += 1;
            continue;
        }
        let mut j = open + 1;
        let mut label = String::new();
        while let Some(&c) = chars.get(j) {
            if c == ']' {
                break;
            }
            label.push(c);
            j += 1;
        }
        if chars.get(j) != Some(&']') || chars.get(j + 1) != Some(&'(') {
            out.push(c);
            i += 1;
            continue;
        }
        let mut k = j + 2;
        let mut url = String::new();
        while let Some(&c) = chars.get(k) {
            if c == ')' {
                break;
            }
            url.push(c);
            k += 1;
        }
        if url.is_empty() || chars.get(k) != Some(&')') {
            out.push(c);
            i += 1;
            continue;
        }
        out.push_str(&label);
        i = k + 1;
    }
    out
}

/// `@{a|b}` -> `b`, then `@{a}` -> `a`.
fn unwrap_braced_refs(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while let Some(&c) = chars.get(i) {
        if !(c == '@' && chars.get(i + 1) == Some(&'{')) {
            out.push(c);
            i += 1;
            continue;
        }
        let mut j = i + 2;
        let mut name = String::new();
        let mut label: Option<String> = None;
        while let Some(&c) = chars.get(j) {
            if c == '}' {
                break;
            }
            if c == '|' && label.is_none() {
                label = Some(String::new());
                j += 1;
                continue;
            }
            match label.as_mut() {
                Some(text) => text.push(c),
                None => name.push(c),
            }
            j += 1;
        }
        if name.is_empty() || chars.get(j) != Some(&'}') {
            out.push(c);
            i += 1;
            continue;
        }
        out.push_str(label.as_deref().unwrap_or(&name));
        i = j + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Markdown;

    fn content(text: &str) -> Content {
        Content(vec![Fragment::Markdown(Markdown::Resolved(
            text.to_string(),
        ))])
    }

    #[test]
    fn prose_fits_a_row() {
        let row = RowContent::try_from(&content("Retrieves the child.\n\nA second paragraph."));
        let Ok(row) = row else {
            panic!("prose must fit a row");
        };
        // Two paragraphs become one line: a row has no paragraph breaks to give them.
        assert_eq!(row.text(), "Retrieves the child. A second paragraph.");
    }

    /// The 166-case half of the shipped defect: an `@example` on a member of a compact
    /// `@table` emits a heading and a code block into a one-line cell.
    #[test]
    fn a_heading_and_a_code_block_do_not_fit_a_row() {
        let text = "Creates one.\n\n##### Example\n\n```lua\nlocal x = 1\n```\n";
        let Err(e) = RowContent::try_from(&content(text)) else {
            panic!("a code block must not fit a row");
        };
        assert_eq!(e.elements(), vec!["h5".to_string(), "pre".to_string()]);
        assert_eq!(e.to_string(), "<h5>, <pre>");
    }

    /// The 85-case half: a bullet list in a property tooltip.
    #[test]
    fn a_list_does_not_fit_a_row() {
        let Err(e) = RowContent::try_from(&content("Modes:\n\n- one\n- two\n")) else {
            panic!("a list must not fit a row");
        };
        assert_eq!(e.elements(), vec!["ul".to_string()]);
    }

    #[test]
    fn an_admonition_body_counts_because_it_lands_in_the_same_cell() {
        let body = Content(vec![Fragment::Markdown(Markdown::Resolved(
            "```lua\nx()\n```".to_string(),
        ))]);
        let c = Content(vec![Fragment::Admonition {
            level: crate::ir::AdmonitionLevel::Note,
            title: "Note".to_string(),
            content: body,
        }]);
        let Err(e) = RowContent::try_from(&c) else {
            panic!("a code block inside a note must not fit a row");
        };
        assert_eq!(e.elements(), vec!["pre".to_string()]);
    }

    #[test]
    fn a_fence_hides_what_looks_like_a_heading() {
        let blocks = split("```lua\n# not a heading\n- not a list\n```\n");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks.first().map(Block::kind), Some(&BlockKind::Code));
    }

    #[test]
    fn inline_html_is_not_a_block() {
        let Ok(row) = RowContent::try_from(&content("A <b>bold</b> word.")) else {
            panic!("inline html must fit a row");
        };
        assert_eq!(row.text(), "A <b>bold</b> word.");
    }
}
