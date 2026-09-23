//! Assembling a documentation block into content, and resolving its cross references.
//!
//! The Python defers reference resolution into a callback hung off every `Markdown`
//! object, which runs the first time a renderer asks for the text -- so *when* a renderer
//! asks decides *what* a name resolves to. That is real behaviour, not an accident: a
//! field's `@{Foo}` resolves relative to the collection it is rendered under, not to the
//! field itself. Here the same rule is an explicit pass, with the context named at the
//! call site instead of hidden in a closure.

use std::borrow::Cow;
use std::path::Path;

use crate::diag::Category;
use crate::ir::{AdmonitionLevel, Content, Fragment, ItemId, Markdown, Param, RawLine, Returned};
use crate::parse::Parser;
use crate::tags::{self, Tag};
use crate::util;

/// What one documentation block turned into.
#[derive(Debug, Default)]
pub struct Parsed {
    /// `@tparam` by name, in the order the tags appear.
    pub params: Vec<(String, Vec<String>, Content)>,
    pub returns: Vec<Returned>,
    pub content: Content,
}

/// One open tag on the nesting stack: the indent that opened it, the tag, and which
/// buffer its body accumulates into.
struct Open {
    indent: usize,
    tag: Option<Tag>,
    body: usize,
}

impl Parser {
    /// Parses a documentation block into content, pulling out `@tparam` and `@treturn`.
    ///
    /// `ctx.item` must already name the element the block documents: `@see` and an
    /// admonition's title resolve here, relative to it, while everything else resolves
    /// later against whatever the renderer is rendering under.
    pub fn parse_raw_content(&mut self, lines: &[RawLine]) -> Parsed {
        // Bodies are addressed by index while the block is open, so a nested tag can fill
        // its own without anything holding two mutable borrows.
        let mut bodies: Vec<Content> = vec![Content::default()];
        let mut stack: Vec<Open> = vec![Open {
            indent: 0,
            tag: None,
            body: 0,
        }];
        // (parent body, fragment index in it, body to move in) -- collected because a
        // body keeps filling after the fragment that owns it has been placed.
        let mut attach: Vec<(usize, usize, usize)> = Vec::new();
        let mut params: Vec<(String, Vec<String>, usize)> = Vec::new();
        let mut returns: Vec<(Vec<String>, usize)> = Vec::new();
        // How far to dedent a raw line: taken from the first line after the nesting
        // changed, and reset whenever it changes again.
        let mut dedent: Option<usize> = None;

        let sentinel = RawLine {
            line: u32::MAX,
            text: String::new(),
            tags: None,
        };
        for raw in lines.iter().chain(std::iter::once(&sentinel)) {
            let is_sentinel = raw.line == u32::MAX;
            self.ctx.line = if is_sentinel { None } else { Some(raw.line) };

            let (text, tag) = match &raw.tags {
                // A manual page's lines carry tags without a comment prefix.
                None => {
                    let parsed = tags::parse(&raw.text, false).unwrap_or_default();
                    (raw.text.clone(), parsed.into_iter().next())
                }
                Some(found) => (
                    raw.text.trim_start_matches('-').trim_end().to_string(),
                    found.first().cloned(),
                ),
            };
            let indent = util::indent_level(&text);

            while stack.len() > 1 && (!text.is_empty() || is_sentinel) {
                if stack.last().is_some_and(|top| top.indent < indent) {
                    break;
                }
                let Some(done) = stack.pop() else { break };
                if done.tag.as_ref().is_some_and(|t| t.as_code().is_some()) {
                    if let Some(body) = bodies.get_mut(done.body) {
                        let md = body.md(true);
                        md.rstrip();
                        md.append("```\n");
                    }
                }
                dedent = None;
            }

            let parent = stack.last().map(|o| o.body).unwrap_or(0);

            let Some(tag) = tag else {
                // The sentinel appends its empty line too, so every content block ends
                // with one. That line is invisible wherever content is trimmed -- a doc
                // comment, a json value, an inlined description -- and visible in exactly
                // one place: it is what separates a field's description from the `*meta*`
                // line the LuaLS renderer appends after it.
                let at = *dedent.get_or_insert(indent);
                let line: String = text.chars().skip(at).collect();
                if let Some(body) = bodies.get_mut(parent) {
                    body.md(true).append(line);
                }
                continue;
            };

            let body = if tag.takes_content() {
                bodies.push(Content::default());
                bodies.len() - 1
            } else {
                parent
            };
            stack.push(Open {
                indent,
                tag: Some(tag.clone()),
                body,
            });

            if let Some((lang, snippet)) = tag.as_code() {
                if let Some(heading) = tag.code_heading() {
                    if let Some(content) = bodies.get_mut(parent) {
                        content.md(true).append(format!("##### {heading}\n"));
                    }
                }
                let lang = lang.unwrap_or("lua").to_string();
                let snippet = snippet.map(str::to_string);
                if let Some(content) = bodies.get_mut(parent) {
                    content.md(true).append(format!("```{lang}"));
                }
                dedent = None;
                if let Some(snippet) = snippet {
                    self.read_snippet(&mut bodies, parent, &snippet);
                }
                continue;
            }

            if let Some((level, title)) = tag.as_admonition() {
                let title = title
                    .map(str::to_string)
                    .unwrap_or_else(|| title_case(level.as_str()));
                let title = self.resolve_text(&title);
                if let Some(content) = bodies.get_mut(parent) {
                    content.push(Fragment::Admonition {
                        level,
                        title,
                        content: Content::default(),
                    });
                    attach.push((parent, content.0.len() - 1, body));
                }
                dedent = None;
                continue;
            }

            match &tag {
                Tag::Deprecated(desc) => {
                    if let Some(desc) = desc {
                        if let Some(content) = bodies.get_mut(body) {
                            content.md(true).append(desc.clone());
                        }
                    }
                    if let Some(content) = bodies.get_mut(parent) {
                        content.push(Fragment::Admonition {
                            level: AdmonitionLevel::Deprecated,
                            title: "Deprecated".to_string(),
                            content: Content::default(),
                        });
                        attach.push((parent, content.0.len() - 1, body));
                    }
                    dedent = None;
                }
                Tag::Param { types, name, desc } => {
                    if let Some(desc) = desc {
                        if let Some(content) = bodies.get_mut(body) {
                            content.md(true).append(desc.clone());
                        }
                    }
                    // A repeated `@tparam` for the same name replaces the first in place,
                    // the way assigning to a dict key does.
                    match params.iter_mut().find(|(n, _, _)| n == name) {
                        Some(slot) => *slot = (name.clone(), types.clone(), body),
                        None => params.push((name.clone(), types.clone(), body)),
                    }
                }
                Tag::Return { types, desc } => {
                    if let Some(desc) = desc {
                        if let Some(content) = bodies.get_mut(body) {
                            content.md(true).append(desc.clone());
                        }
                    }
                    returns.push((types.clone(), body));
                }
                Tag::See(names) => {
                    let mut ids = Vec::new();
                    for name in names.clone() {
                        if let Some(found) = self.resolve_ref(&name) {
                            ids.push(self.item(found).id.clone());
                        }
                    }
                    if let Some(content) = bodies.get_mut(parent) {
                        content.push(Fragment::SeeAlso(ids));
                    }
                }
                other => {
                    let name = other.reported_name().to_string();
                    let file = self.ctx.file.clone();
                    self.diagnostics.add(
                        Category::Structure,
                        format!("unknown tag @{name} or missing arguments"),
                        file.as_deref(),
                        Some(raw.line),
                    );
                }
            }
        }

        // A body is always pushed after the body that owns it, so moving them in
        // descending order fills children before their parents.
        attach.sort_by_key(|a| std::cmp::Reverse(a.2));
        for (parent, index, child) in attach {
            let taken = bodies
                .get_mut(child)
                .map(std::mem::take)
                .unwrap_or_default();
            if let Some(Fragment::Admonition { content, .. }) =
                bodies.get_mut(parent).and_then(|c| c.0.get_mut(index))
            {
                *content = taken;
            }
        }

        Parsed {
            params: params
                .into_iter()
                .map(|(name, types, body)| {
                    (name, types, bodies.get(body).cloned().unwrap_or_default())
                })
                .collect(),
            returns: returns
                .into_iter()
                .map(|(types, body)| Returned {
                    types,
                    content: bodies.get(body).cloned().unwrap_or_default(),
                })
                .collect(),
            content: bodies.first().cloned().unwrap_or_default(),
        }
    }

    fn read_snippet(&mut self, bodies: &mut [Content], parent: usize, snippet: &str) {
        let dir = self
            .config
            .get("project", "snippet_path")
            .map(str::to_string);
        let problem = match dir {
            None => Some("no snippet_path configured".to_string()),
            Some(dir) => {
                let path = Path::new(&dir).join(snippet);
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        if let Some(content) = bodies.get_mut(parent) {
                            let md = content.md(true);
                            for line in text.lines() {
                                md.append(line);
                            }
                        }
                        None
                    }
                    Err(e) => Some(os_error(&e, &path)),
                }
            }
        };
        let Some(problem) = problem else { return };
        let (file, line) = (self.ctx.file.clone(), self.ctx.line);
        self.diagnostics.add(
            Category::Snippets,
            format!("cannot read snippet \"{snippet}\": {problem}"),
            file.as_deref(),
            line,
        );
        // Keep the omission visible, so an allowed `snippets` category cannot publish an
        // empty code block.
        if let Some(content) = bodies.get_mut(parent) {
            content
                .md(true)
                .append(format!("MISSING SNIPPET: {snippet}"));
        }
    }

    // -- cross references in text ----------------------------------------------

    /// Replaces `` `name` `` and `@{name}` with markdown links whose target is
    /// `luadox:<id>`, which the renderer turns into a real href.
    pub fn resolve_text(&mut self, text: &str) -> String {
        let text = self.resolve_backtick_refs(text);
        self.resolve_brace_refs(&text)
    }

    /// `` `name` `` -- a backtick, a name with neither space nor backtick in it, a
    /// backtick. An opening backtick preceded by another is part of a code fence.
    fn resolve_backtick_refs(&mut self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        let mut i = 0usize;
        while let Some(&c) = chars.get(i) {
            let opens =
                c == '`' && !matches!(i.checked_sub(1).and_then(|p| chars.get(p)), Some('`'));
            if !opens {
                out.push(c);
                i += 1;
                continue;
            }
            let mut j = i + 1;
            let mut name = String::new();
            while let Some(&c) = chars.get(j) {
                if c == '`' || c == ' ' {
                    break;
                }
                name.push(c);
                j += 1;
            }
            if name.is_empty() || !matches!(chars.get(j), Some('`')) {
                out.push(c);
                i += 1;
                continue;
            }
            match self.resolve_ref(&name) {
                Some(found) => out.push_str(&self.ref_markdown(found, Some(&name), true)),
                None => {
                    out.push('`');
                    out.push_str(&name);
                    out.push('`');
                }
            }
            i = j + 1;
        }
        out
    }

    /// `@{name}` and `@{name|text}`, either optionally wrapped in backticks. The
    /// backticks are consumed: the link carries the code styling itself.
    fn resolve_brace_refs(&mut self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        let mut i = 0usize;
        while let Some(&here) = chars.get(i) {
            let code = here == '`' && matches!(chars.get(i + 1), Some('@'));
            let at = if code { i + 1 } else { i };
            if !(matches!(chars.get(at), Some('@')) && matches!(chars.get(at + 1), Some('{'))) {
                out.push(here);
                i += 1;
                continue;
            }
            let mut j = at + 2;
            let mut name = String::new();
            while let Some(&c) = chars.get(j) {
                if c == '}' || c == '|' {
                    break;
                }
                name.push(c);
                j += 1;
            }
            let mut label: Option<String> = None;
            if matches!(chars.get(j), Some('|')) {
                j += 1;
                let mut text = String::new();
                while let Some(&c) = chars.get(j) {
                    if c == '}' {
                        break;
                    }
                    text.push(c);
                    j += 1;
                }
                label = Some(text);
            }
            if name.is_empty() || !matches!(chars.get(j), Some('}')) {
                out.push(here);
                i += 1;
                continue;
            }
            j += 1;
            if code && matches!(chars.get(j), Some('`')) {
                j += 1;
            }
            match self.resolve_ref(&name) {
                Some(found) => out.push_str(&self.ref_markdown(found, label.as_deref(), code)),
                None => {
                    let (file, line) = (self.ctx.file.clone(), self.ctx.line);
                    self.diagnostics.add(
                        Category::References,
                        format!("reference \"{name}\" could not be resolved"),
                        file.as_deref(),
                        line,
                    );
                    out.push_str(label.as_deref().unwrap_or(&name));
                }
            }
            i = j;
        }
        out
    }

    /// Resolves every reference still pending in a content tree, against `ctx.item`.
    /// This is the Python's deferred `Markdown.get()` made into a pass.
    pub fn resolve_content(&mut self, content: &mut Content) {
        for index in 0..content.0.len() {
            match content.0.get_mut(index) {
                Some(Fragment::Markdown(md)) => {
                    if md.is_resolved() || !md.resolve {
                        continue;
                    }
                    let raw = md.raw();
                    let resolved = self.resolve_text(&raw);
                    if let Some(Fragment::Markdown(md)) = content.0.get_mut(index) {
                        md.set_resolved(resolved);
                    }
                }
                Some(Fragment::Admonition { .. }) => {
                    let Some(Fragment::Admonition { content: body, .. }) = content.0.get_mut(index)
                    else {
                        continue;
                    };
                    let mut taken = std::mem::take(body);
                    self.resolve_content(&mut taken);
                    if let Some(Fragment::Admonition { content: body, .. }) =
                        content.0.get_mut(index)
                    {
                        *body = taken;
                    }
                }
                _ => {}
            }
        }
    }

    /// The first sentence of already-resolved content, left where it is.
    ///
    /// `skip_leading` steps over leading non-markdown fragments so a `@deprecated` box
    /// cannot become an element's summary; without it, a leading fragment that is not
    /// markdown yields nothing at all.
    pub fn peek_first_sentence(&self, content: &Content, skip_leading: bool) -> String {
        let at = if skip_leading {
            content
                .0
                .iter()
                .position(|f| matches!(f, Fragment::Markdown(_)))
        } else {
            match content.0.first() {
                Some(Fragment::Markdown(_)) => Some(0),
                _ => None,
            }
        };
        let Some(Fragment::Markdown(md)) = at.and_then(|n| content.0.get(n)) else {
            return String::new();
        };
        util::first_sentence(&md.get()).0.to_string()
    }

    /// The first sentence of a content tree, resolved and removed from it. A leading
    /// fragment that is not markdown yields nothing, which is how a section that opens
    /// with an admonition keeps its own name as its heading.
    pub fn take_first_sentence(&mut self, content: &mut Content) -> String {
        let Some(Fragment::Markdown(md)) = content.0.first() else {
            return String::new();
        };
        let raw = md.raw();
        let already = md.is_resolved() || !md.resolve;
        let resolved = if already {
            raw
        } else {
            Cow::Owned(self.resolve_text(&raw))
        };
        let (first, rest) = util::first_sentence(&resolved);
        let (first, rest) = (first.to_string(), rest.to_string());
        if rest.is_empty() {
            content.0.remove(0);
        } else if let Some(slot) = content.0.first_mut() {
            *slot = Fragment::Markdown(Markdown::resolved(rest));
        }
        first
    }
}

/// A failure to read a file, worded and pathed the way Python's `OSError` prints one.
///
/// The wording is not parity for its own sake: a run of this tool is compared against a
/// run of the Python, and a message that says the same thing differently turns 71 real
/// reports into 142 lines of delta that hide the reports that actually changed. The
/// errno is POSIX, as Python's is on every platform, rather than the host's own code.
fn os_error(e: &std::io::Error, path: &Path) -> String {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let path = path.display().to_string();
    let path = path.strip_prefix(r"\\?\").unwrap_or(&path);
    match e.kind() {
        std::io::ErrorKind::NotFound => {
            format!("[Errno 2] No such file or directory: '{path}'")
        }
        std::io::ErrorKind::PermissionDenied => {
            format!("[Errno 13] Permission denied: '{path}'")
        }
        std::io::ErrorKind::InvalidData => {
            format!("'utf-8' codec can't decode the bytes of '{path}'")
        }
        _ => format!("{e}: '{path}'"),
    }
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// One entry per parameter the source declares, documented or not.
pub fn params_for(parser: &mut Parser, id: ItemId, parsed: &Parsed, args: &[String]) -> Vec<Param> {
    let mut out = Vec::new();
    for arg in args {
        match parsed.params.iter().find(|(n, _, _)| n == arg) {
            Some((name, types, content)) => out.push(Param {
                name: name.clone(),
                types: types.clone(),
                content: content.clone(),
            }),
            None => {
                out.push(Param {
                    name: arg.clone(),
                    types: Vec::new(),
                    content: Content::default(),
                });
                // Reported whether or not any other parameter is documented: gating only
                // partly documented functions would reward deleting the rest.
                let item = parser.item(id);
                let (name, file, line) = (item.name.clone(), item.file.clone(), item.line);
                parser.diagnostics.add(
                    Category::Untyped,
                    format!("{name}() missing @tparam for \"{arg}\" parameter"),
                    Some(&file),
                    line,
                );
            }
        }
    }
    out
}
