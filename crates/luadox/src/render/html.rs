//! The html renderer: one page per class, module and manual page, plus a search page, a
//! search index and the asset bundle.
//!
//! Implemented against `spec/html.md`, not against `render/html.py`.
//!
//! Two properties of the Python shape almost every rule here, and both are easy to lose:
//!
//!   * **Every `out(...)` is one line of the page, joined with `\n` at the end.** So an
//!     empty string is a blank line, and `out(self._since(colref))` costs a line whether
//!     or not the element has a `@since`. Several of the blank lines in the output exist
//!     for no other reason.
//!   * **The root path depends on what the renderer is currently looking at**, not on
//!     what is being linked. `_get_root_path` reads the context ref, which the walk
//!     mutates, so the same reference renders a different href from two different pages.
//!
//! Type names are printed as written here. The LuaLS renderer maps them -- `@treturn
//! void` is `void` on a page and `nil` in `luadox.lua` -- so a shared "format a type"
//! helper between the two renderers is a bug.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};

use crate::assets;
use crate::ir::{Content, Fragment, ItemId, Kind, Member, Page, RefId, SeeRef};
use crate::markdown;
use crate::parse::{Parser, RefStyle};
use crate::util;
use crate::Error;

/// A rendered file and where it goes, relative to the output directory.
pub struct Output {
    pub path: String,
    pub bytes: Vec<u8>,
}

struct Templates {
    head: String,
    foot: String,
    search: String,
    sidebar: String,
}

/// A file under construction. The Python collects lines and joins them with `\n`; the
/// same bytes are written here as each line arrives, into the one buffer the file is.
struct Lines(String);

impl Lines {
    fn new() -> Self {
        Self(String::new())
    }

    fn push(&mut self, line: &str) {
        self.0.push_str(line);
        self.0.push('\n');
    }

    fn line(&mut self, args: fmt::Arguments<'_>) {
        // Writing into a String cannot fail.
        self.0.write_fmt(args).unwrap_or_default();
        self.0.push('\n');
    }

    /// The lines joined, without the newline the last one was pushed with.
    fn finish(mut self) -> String {
        self.0.pop();
        self.0
    }
}

struct Renderer<'a> {
    parser: &'a mut Parser,
    version: String,
    templates: Templates,
    search: ItemId,
    /// Class pages sorted by name, and module pages in registration order: the two
    /// orders the sidebar and the previous/next buttons walk.
    classes: Vec<ItemId>,
    modules: Vec<ItemId>,
    manuals: Vec<ItemId>,
    /// Whether a `[manual] index` is configured, which decides the topbar's home button.
    has_manual_index: bool,
    hometext: String,
    project_title: String,
}

pub fn render(parser: &mut Parser, toprefs: &[ItemId]) -> Result<Vec<Output>, Error> {
    let templates = load_templates(parser)?;
    let search = parser.add_search_ref();

    let mut classes: Vec<ItemId> = parser
        .topsyms
        .iter()
        .copied()
        .filter(|id| parser.item(*id).kind == Kind::Class)
        .collect();
    classes.sort_by_key(|id| parser.item(*id).name.clone());
    let modules: Vec<ItemId> = parser
        .topsyms
        .iter()
        .copied()
        .filter(|id| parser.item(*id).kind == Kind::Module)
        .collect();
    let manuals: Vec<ItemId> = parser
        .topsyms
        .iter()
        .copied()
        .filter(|id| parser.item(*id).kind == Kind::Manual)
        .collect();

    let project_title = parser.settings.project_title().to_string();
    let hometext = parser.settings.hometext().to_string();
    let has_manual_index = parser.settings.has_manual_index();

    let mut r = Renderer {
        version: assets::version(),
        templates,
        search,
        classes,
        modules,
        manuals,
        has_manual_index,
        hometext,
        project_title,
        parser,
    };

    let mut out = Vec::new();
    for topref in toprefs {
        let item = r.parser.item(*topref);
        if item.empty && item.implicit {
            continue;
        }
        let path = match item.page() {
            Some(Page::Landing) => format!("{}.html", item.name),
            _ => format!("{}/{}.html", item.kind.as_str(), item.name),
        };
        let html = r.render_page(*topref);
        out.push(Output {
            path,
            bytes: html.into_bytes(),
        });
    }

    out.push(Output {
        path: "index.js".to_string(),
        bytes: r.search_index().into_bytes(),
    });
    out.push(Output {
        path: "search.html".to_string(),
        bytes: r.search_page().into_bytes(),
    });
    // Only when the project has not supplied its own landing page.
    let has_index = r
        .parser
        .of_kind(Kind::Manual)
        .iter()
        .any(|id| r.parser.item(*id).page() == Some(Page::Landing));
    if !has_index {
        out.push(Output {
            path: "index.html".to_string(),
            bytes: r.landing_page().into_bytes(),
        });
    }

    for name in assets::COPIED {
        if let Some(bytes) = assets::get(name) {
            out.push(Output {
                path: name.to_string(),
                bytes: bytes.to_vec(),
            });
        }
    }
    Ok(out)
}

/// The files `project.css`, `project.js` and `project.favicon` name, copied flat into the
/// output. One that does not exist is skipped and reported, and the run continues -- but
/// its `<link>` or `<script>` tag is still emitted on every page, which is why the shipped
/// production docs carry a dangling `custom-styles-lua.css`.
pub fn config_files(parser: &Parser) -> (Vec<PathBuf>, Vec<String>) {
    let mut found = Vec::new();
    let mut missing = Vec::new();
    let settings = &parser.settings;
    for name in [&settings.css, &settings.js, &settings.favicon]
        .into_iter()
        .flatten()
    {
        let path = PathBuf::from(name);
        if path.is_file() {
            found.push(path);
        } else {
            missing.push(name.clone());
        }
    }
    (found, missing)
}

fn load_templates(parser: &Parser) -> Result<Templates, Error> {
    let read = |path: Option<&str>, asset: &str| -> Result<String, Error> {
        match path {
            // A configured template is read in text mode by the Python, so its line
            // endings are normalised; reading it as a string does the same.
            Some(path) => std::fs::read_to_string(path)
                .map(|s| s.replace("\r\n", "\n"))
                .map_err(Error::io(path)),
            None => Ok(assets::text(asset)),
        }
    };
    let paths = &parser.settings.templates;
    Ok(Templates {
        head: read(paths.head.as_deref(), "head.tmpl.html")?,
        foot: read(paths.foot.as_deref(), "foot.tmpl.html")?,
        search: read(paths.search.as_deref(), "search.tmpl.html")?,
        sidebar: read(paths.sidebar.as_deref(), "sidebar.tmpl.html")?,
    })
}

/// `str.format` with the named fields luadox's templates use.
///
/// The Python uses `str.format`, so a literal `{` or `}` in a custom template raises.
/// Here an unknown field is left as written, which is the same decision a documentation
/// tool should make about somebody else's template: report nothing, change nothing.
fn format_template(template: &str, fields: &BTreeMap<&str, &str>) -> String {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::with_capacity(template.len());
    let mut i = 0usize;
    while let Some(&c) = chars.get(i) {
        if c != '{' {
            out.push(c);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut name = String::new();
        while let Some(&c) = chars.get(j) {
            if c == '}' {
                break;
            }
            name.push(c);
            j += 1;
        }
        match fields
            .get(name.as_str())
            .filter(|_| chars.get(j) == Some(&'}'))
        {
            Some(value) => {
                out.push_str(value);
                i = j + 1;
            }
            None => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

impl Renderer<'_> {
    // -- paths -----------------------------------------------------------------

    /// The prefix that reaches the document root from the page being rendered.
    fn root(&self) -> String {
        let Some(ctx) = self.parser.ctx.item else {
            return String::new();
        };
        let via = self.parser.topref(ctx);
        let item = self.parser.item(via);
        if matches!(item.page(), Some(Page::Landing | Page::Search)) {
            String::new()
        } else {
            "../".to_string()
        }
    }

    /// The href of a link to an element.
    ///
    /// The asymmetry is easy to lose: the *directory* comes from the `@within`-redirected
    /// page's type, the *manual/index test* from the element's own page, and the *file
    /// name* from the redirected symbol.
    fn href(&self, id: ItemId) -> String {
        let item = self.parser.item(id);
        let topref = match self.parser.within_page.get(&id) {
            Some(page) => *page,
            None => match self.parser.refs.get(&item.topsym) {
                Some(topref) => *topref,
                // The Python re-raises a KeyError here and the run dies. A documentation
                // tool should emit a dead link and keep going.
                None => return format!("{}.html", item.topsym),
            },
        };

        let own = self.parser.topref(id);
        let own_is_landing = self.parser.item(own).page() == Some(Page::Landing);

        let mut prefix = self.root();
        if !own_is_landing {
            prefix += &format!("{}/", self.parser.item(topref).kind.as_str());
        }
        let fragment = if own_is_landing && !item.symbol.is_empty() {
            // A manual does not use fully qualified fragments.
            if item.scopes.is_empty() {
                String::new()
            } else {
                format!("#{}", item.symbol)
            }
        } else if item.name != item.topsym {
            format!("#{}", item.name)
        } else {
            String::new()
        };
        format!("{prefix}{}.html{fragment}", self.parser.item(topref).name)
    }

    fn permalink(&self, id: &str) -> String {
        format!(
            "<a class=\"permalink\" href=\"#{id}\" title=\"Permalink to this definition\">\u{b6}</a>"
        )
    }

    // -- markdown --------------------------------------------------------------

    fn markdown(&self, md: &str) -> String {
        let parser = &self.parser;
        markdown::to_html(md, &|id: &str| {
            parser
                .item_by_id(&RefId::from(id))
                .map(|found| self.href(found))
        })
    }

    fn content_html(&self, content: &Content) -> String {
        self.fragments_html(&content.0)
    }

    fn fragments_html(&self, fragments: &[Fragment]) -> String {
        let mut out: Vec<String> = Vec::new();
        for fragment in fragments {
            match fragment {
                Fragment::Markdown(md) => out.push(self.markdown(&md.get())),
                Fragment::Admonition {
                    level,
                    title,
                    content,
                } => {
                    let inner = self.content_html(content);
                    let inner = inner.trim();
                    // An admonition with no body -- a bare `@deprecated` -- is its title.
                    let body = if inner.is_empty() {
                        String::new()
                    } else {
                        format!("<div class=\"body\">{inner}\n</div>")
                    };
                    let level = level.as_str();
                    out.push(format!(
                        "<div class=\"admonition {level}\"><div class=\"title\">{title}</div>{body}</div>"
                    ));
                }
                Fragment::SeeAlso(refs) => {
                    let md = refs
                        .iter()
                        .filter_map(SeeRef::item)
                        .map(|found| self.parser.ref_markdown(found, None, RefStyle::Plain))
                        .collect::<Vec<_>>()
                        .join(", ");
                    // The slice is unconditional in the Python, so an empty `@see` still
                    // prints "See also " -- where the LuaLS renderer drops it.
                    let html = self.markdown(&md);
                    let html = strip_paragraph(html.trim());
                    out.push(format!("<div class=\"see\">See also {html}</div>"));
                }
            }
        }
        out.join("\n")
    }

    fn content_text(&self, content: &Content) -> String {
        self.fragments_text(&content.0)
    }

    #[allow(clippy::only_used_in_recursion)]
    fn fragments_text(&self, fragments: &[Fragment]) -> String {
        let mut out: Vec<String> = Vec::new();
        for fragment in fragments {
            match fragment {
                Fragment::Admonition { title, content, .. } => {
                    out.push(markdown::to_text(title));
                    out.push(self.content_text(content));
                }
                Fragment::Markdown(md) => out.push(markdown::to_text(&md.get())),
                Fragment::SeeAlso(_) => {}
            }
        }
        out.join("\n").trim().to_string()
    }

    /// Type names, resolved to links where they resolve, joined for a human.
    fn types_html(&mut self, types: &[String]) -> String {
        let mut resolved: Vec<String> = Vec::new();
        for name in types {
            let found = self.parser.resolve_ref(name);
            let body = match found {
                Some(found) => format!("<a href=\"{}\">{name}</a>", self.href(found)),
                None => name.clone(),
            };
            resolved.push(format!("<em>{body}</em>"));
        }
        match resolved.len() {
            0 => String::new(),
            1 => resolved.join(""),
            n => {
                let last = resolved.get(n - 1).cloned().unwrap_or_default();
                let head = resolved.get(..n - 1).unwrap_or(&[]).join(", ");
                format!("{head} or {last}")
            }
        }
    }

    // -- the page frame --------------------------------------------------------

    fn render_page(&mut self, topref: ItemId) -> String {
        let mut lines = Lines::new();
        self.frame_open(topref, &mut lines);
        match self.parser.item(topref).kind {
            Kind::Class | Kind::Module => self.classmod(topref, &mut lines),
            Kind::Manual => self.manual(topref, &mut lines),
            _ => {}
        }
        self.frame_close(&mut lines);
        lines.finish()
    }

    fn frame_open(&mut self, topref: ItemId, out: &mut Lines) {
        self.parser.focus(topref);
        let root = self.root();

        let page_title = if self.parser.item(topref).kind == Kind::Manual {
            self.parser
                .item(topref)
                .collections
                .first()
                .map(|c| self.parser.item(*c).heading.clone())
                .unwrap_or_default()
        } else {
            self.parser.item(topref).display.clone()
        };
        let html_title = format!("{page_title} - {}", self.project_title);

        let mut head: Vec<String> = Vec::new();
        for css in &self.parser.settings.css {
            let name = basename(css);
            head.push(format!(
                "<link href=\"{root}{name}?{}\" rel=\"stylesheet\" />",
                self.version
            ));
        }
        for js in &self.parser.settings.js {
            let name = basename(js);
            head.push(format!(
                "<script src=\"{root}{name}?{}\"></script>",
                self.version
            ));
        }
        for favicon in &self.parser.settings.favicon {
            let mimetype = match mimetype_of(favicon) {
                Some(t) => format!(" type=\"{t}\""),
                None => String::new(),
            };
            let name = basename(favicon);
            // The format string already has a space before the type, so a known type
            // produces two.
            head.push(format!(
                "<link rel=\"shortcut icon\" {mimetype} href=\"{root}{name}?{}\"/>",
                self.version
            ));
        }

        let item = self.parser.item(topref);
        let bodyclass = format!(
            "{}-{}",
            if item.kind.as_str().is_empty() {
                "other"
            } else {
                item.kind.as_str()
            },
            item.name
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_')
                .collect::<String>()
                .to_lowercase()
        );

        let head_joined = head.join("\n");
        let fields = BTreeMap::from([
            ("version", self.version.as_str()),
            ("title", html_title.as_str()),
            ("head", head_joined.as_str()),
            ("root", root.as_str()),
            ("bodyclass", bodyclass.as_str()),
        ]);
        out.push(&format_template(&self.templates.head, &fields));

        self.topbar(topref, &root, out);
        self.sidebar(topref, &root, out);
        out.push("<div class=\"body\">");
    }

    fn frame_close(&mut self, out: &mut Lines) {
        out.push("</div>");
        let root = self.root();
        let fields = BTreeMap::from([("root", root.as_str()), ("version", self.version.as_str())]);
        out.push(&format_template(&self.templates.foot, &fields));
    }

    fn topbar(&mut self, topref: ItemId, root: &str, out: &mut Lines) {
        // The walk stops one past the current page; on the search and landing pages the
        // very first entry counts as the match, so they get a Next and no Previous.
        let order: Vec<ItemId> = self
            .manuals
            .iter()
            .chain(self.classes.iter())
            .chain(self.modules.iter())
            .copied()
            .collect();
        let page = self.parser.item(topref).page();
        let name = self.parser.item(topref).name.clone();
        let (mut prev, mut next, mut found) = (None, None, false);
        for id in order {
            if found {
                next = Some(id);
                break;
            }
            if self.parser.item(id).topsym == name || page == Some(Page::Search) {
                found = true;
            } else {
                prev = Some(id);
            }
        }

        out.push("<div class=\"topbar\">");
        out.push("<div class=\"group one\">");
        if self.has_manual_index {
            let path = if page == Some(Page::Landing) {
                ""
            } else {
                "../"
            };
            out.line(format_args!(
                "<div class=\"button description\"><a href=\"{path}index.html\"><span>{}</span></a></div>",
                self.hometext
            ));
        } else {
            out.line(format_args!(
                "<div class=\"description\"><span>{}</span></div>",
                self.hometext
            ));
        }
        out.push("</div>");
        out.push("<div class=\"group two\">");
        self.user_links(root, out);
        out.push("</div>");
        out.push("<div class=\"group three\">");
        if let Some(prev) = prev {
            out.line(format_args!(
                "<div class=\"button iconleft\"><a href=\"{}\" title=\"{}\"><img src=\"{root}img/i-left.svg?{}\" alt=\"\"/><span>Previous</span></a></div>",
                self.href(prev),
                self.parser.item(prev).name,
                self.version
            ));
        }
        if let Some(next) = next {
            out.line(format_args!(
                "<div class=\"button iconright\"><a href=\"{}\" title=\"{}\"><span>Next</span><img src=\"{root}img/i-right.svg?{}\" alt=\"\"/></a></div>",
                self.href(next),
                self.parser.item(next).name,
                self.version
            ));
        }
        out.push("</div>");
        out.push("</div>");
    }

    /// Every config section whose name starts with `link`, in section-name order.
    fn user_links(&self, root: &str, out: &mut Lines) {
        for link in &self.parser.settings.links {
            let mut class = String::new();
            let img = match &link.icon {
                Some(icon) => {
                    let icon = match icon.as_str() {
                        "download" | "github" | "gitlab" | "bitbucket" => {
                            format!("{{root}}img/i-{icon}.svg?{}", self.version)
                        }
                        other => other.to_string(),
                    };
                    class = " iconleft".to_string();
                    format!("<img src=\"{}\" alt=\"\"/>", icon.replace("{root}", root))
                }
                None => String::new(),
            };
            out.line(format_args!(
                "<div class=\"button{class}\"><a href=\"{}\" title=\"{}\">{img}<span>{}</span></a></div>",
                link.url.replace("{root}", root),
                link.tooltip,
                link.text
            ));
        }
    }

    fn sidebar(&mut self, topref: ItemId, root: &str, out: &mut Lines) {
        out.push("<div class=\"sidebar\">");
        let fields = BTreeMap::from([("root", root), ("version", self.version.as_str())]);
        out.push(&format_template(&self.templates.sidebar, &fields));
        out.line(format_args!("<form action=\"{root}search.html\">"));
        out.push("<input class=\"search\" name=\"q\" type=\"search\" placeholder=\"Search\" />");
        out.push("</form>");

        let name = self.parser.item(topref).name.clone();
        let collections = self.parser.item(topref).collections.clone();
        if !collections.is_empty() {
            out.push("<div class=\"sections\">");
            out.push("<div class=\"heading\">Contents</div>");
            out.push("<ul>");
            for col in collections {
                let item = self.parser.item(col);
                if item.kind == Kind::Manual {
                    continue;
                }
                // Raw, not markdown-rendered.
                let heading = if matches!(item.kind, Kind::Class | Kind::Module) {
                    format!(
                        "{} <code>{}</code>",
                        title_case(item.kind.as_str()),
                        item.heading
                    )
                } else {
                    item.heading.clone()
                };
                out.line(format_args!(
                    "<li><a href=\"#{}\">{heading}</a></li>",
                    item.symbol
                ));
            }
            out.push("</ul>");
            out.push("</div>");
        }

        let manuals = self.parser.of_kind(Kind::Manual).to_vec();
        if !manuals.is_empty() {
            out.push("<div class=\"manual\">");
            out.push("<div class=\"heading\">Manual</div>");
            out.push("<ul>");
            for id in manuals {
                if self.parser.item(id).scope().is_some() {
                    continue;
                }
                let selected = self.selected(id, &name);
                out.line(format_args!(
                    "<li{selected}><a href=\"{}\">{}</a></li>",
                    self.href(id),
                    self.parser.item(id).heading
                ));
            }
            out.push("</ul>");
            out.push("</div>");
        }

        if !self.classes.is_empty() {
            out.push("<div class=\"classes\">");
            out.push("<div class=\"heading\">Classes</div>");
            out.push("<ul>");
            for id in self.classes.clone() {
                let selected = self.selected(id, &name);
                out.line(format_args!(
                    "<li{selected}><a href=\"{}\">{}</a></li>",
                    self.href(id),
                    self.parser.item(id).display
                ));
            }
            out.push("</ul>");
            out.push("</div>");
        }

        if !self.modules.is_empty() {
            out.push("<div class=\"modules\">");
            out.push("<div class=\"heading\">Modules</div>");
            out.push("<ul>");
            for id in self.modules.clone() {
                let item = self.parser.item(id);
                if item.empty && item.implicit {
                    continue;
                }
                let selected = self.selected(id, &name);
                out.line(format_args!(
                    "<li{selected}><a href=\"{}\">{}</a></li>",
                    self.href(id),
                    self.parser.item(id).name
                ));
            }
            out.push("</ul>");
            out.push("</div>");
        }
        out.push("</div>");
    }

    fn selected(&self, id: ItemId, current: &str) -> &'static str {
        if self.parser.item(id).name == current {
            " class=\"selected\""
        } else {
            ""
        }
    }

    // -- pages -----------------------------------------------------------------

    fn manual(&mut self, topref: ItemId, out: &mut Lines) {
        out.push("<div class=\"manual\">");
        if !self.parser.item(topref).content.is_empty() {
            self.parser.resolve_item_content(topref);
            let content = &self.parser.item(topref).content;
            out.push(&self.content_html(content));
        }
        for sec in self.parser.item(topref).collections.clone() {
            // The context stays on the manual page: a manual section's markdown resolves
            // against the page, where a class collection's resolves against itself.
            self.parser.resolve_item_content(sec);
            let item = self.parser.item(sec);
            let level = item.heading_level.unwrap_or(0);
            let (symbol, heading) = (item.symbol.clone(), item.heading.clone());
            // The heading is emitted raw -- it has been through the reference rewrite, so
            // a `@{ref}` in a heading lands as literal markdown.
            out.line(format_args!("<h{level} id=\"{symbol}\">{heading}"));
            out.push(&self.permalink(&symbol));
            out.line(format_args!("</h{level}>"));
            let content = &self.parser.item(sec).content;
            out.push(&self.content_html(content));
        }
        out.push("</div>");
    }

    fn classmod(&mut self, topref: ItemId, out: &mut Lines) {
        for col in self.parser.item(topref).collections.clone() {
            self.parser.focus(col);
            let item = self.parser.item(col);
            let (kind, symbol, heading) = (item.kind, item.symbol.clone(), item.heading.clone());

            let heading = if kind.is_top() {
                format!("{} <code>{heading}</code>", title_case(kind.as_str()))
            } else {
                // A heading converted from markdown carries paragraph tags, and a heading
                // may not contain block elements. The trailing newline survives the
                // strip, which is the extra blank line before the permalink.
                self.markdown(&heading)
                    .replace("<p>", "")
                    .replace("</p>", "")
            };
            out.push("<div class=\"section\">");
            out.line(format_args!(
                "<h2 class=\"{}\" id=\"{symbol}\">{heading}",
                kind.as_str()
            ));
            out.push(&self.since(col));
            out.push(&self.permalink(&symbol));
            out.push("</h2>");
            out.push("<div class=\"inner\">");

            if kind == Kind::Class {
                self.hierarchy(col, out);
            }

            self.parser.resolve_item_content(col);
            if !self.parser.item(col).content.is_empty() {
                let content = &self.parser.item(col).content;
                out.push(&self.content_html(content));
            }

            let columns = self.columns(col);
            self.synopsis(col, &columns, out);
            self.field_details(col, &columns, out);
            self.function_details(col, &columns, out);

            out.push("</div>");
            out.push("</div>");
        }
    }

    fn since(&self, id: ItemId) -> String {
        match &self.parser.item(id).flags.since {
            Some(version) => format!("<span class=\"tag since\">since {version}</span>"),
            None => String::new(),
        }
    }

    fn hierarchy(&mut self, col: ItemId, out: &mut Lines) {
        let chain = self.parser.hierarchy(col);
        if chain.len() > 1 {
            out.push("<div class=\"hierarchy\">");
            out.push("<div class=\"heading\">Class Hierarchy</div>");
            out.push("<ul>");
            for (n, cls) in chain.iter().enumerate() {
                let (html, self_class) = if *cls == col {
                    (self.parser.item(*cls).name.clone(), " self")
                } else {
                    let name = self.parser.item(*cls).name.clone();
                    (self.types_html(&[name]), "")
                };
                let prefix = if n > 0 {
                    "&nbsp;".repeat((n - 1) * 6) + "&nbsp;\u{2514}\u{2500} "
                } else {
                    String::new()
                };
                out.line(format_args!(
                    "<li class=\"class{self_class}\">{prefix}<span>{html}</span></li>"
                ));
            }
            out.push("</ul>");
            out.push("</div>");
        }
        // The hierarchy above shows only the first parent; list them all when there are
        // several, so multiple inheritance is visible.
        let parents = self.parser.parents(col);
        if parents.len() > 1 {
            let links = parents
                .iter()
                .map(|id| {
                    let name = self.parser.item(*id).name.clone();
                    self.types_html(&[name])
                })
                .collect::<Vec<_>>()
                .join(", ");
            out.push("<div class=\"inherits\">");
            out.push("<div class=\"heading\">Inherits</div>");
            out.line(format_args!("<div>{links}</div>"));
            out.push("</div>");
        }
    }

    // -- synopsis and detail lists ---------------------------------------------

    fn columns(&self, col: ItemId) -> Columns {
        let item = self.parser.item(col);
        let mut columns = Columns {
            fields_title: "Fields",
            fields_meta: 0,
            fields_has_type: false,
            functions_title: "Functions",
            functions_meta: 0,
            fields_compact: item.flags.compact.contains(&Member::Fields),
            functions_compact: item.flags.compact.contains(&Member::Functions),
        };
        for id in &item.fields {
            let field = self.parser.item(*id);
            if field
                .scope()
                .is_some_and(|s| self.parser.item(s).kind == Kind::Class)
            {
                columns.fields_title = "Attributes";
            }
            if field.meta.as_deref().is_some_and(|m| !m.is_empty()) {
                columns.fields_meta = columns.fields_meta.max(1);
            }
            if !field.types.is_empty() {
                columns.fields_has_type = true;
            }
        }
        for id in &item.functions {
            let function = self.parser.item(*id);
            let in_class = function
                .scope()
                .is_some_and(|s| self.parser.item(s).kind == Kind::Class);
            if in_class && function.symbol.contains(':') {
                columns.functions_title = "Methods";
            }
            // Note this reads the *flag*, where the fields column reads the rendered
            // `meta`; they differ for a function, whose meta is resolved in prerender.
            if function.flags.meta.is_some() {
                columns.functions_meta = columns.functions_meta.max(1);
            }
        }
        columns
    }

    fn synopsis(&mut self, col: ItemId, columns: &Columns, out: &mut Lines) {
        let fields = self.parser.item(col).fields.clone();
        let functions = self.parser.item(col).functions.clone();
        if fields.is_empty() && functions.is_empty() {
            return;
        }
        out.push("<div class=\"synopsis\">");
        if !columns.fields_compact {
            out.push("<h3>Synopsis</h3>");
        }

        if !fields.is_empty() {
            if !functions.is_empty() || !columns.fields_compact {
                out.line(format_args!(
                    "<div class=\"heading\">{}</div>",
                    columns.fields_title
                ));
            }
            out.line(format_args!(
                "<table class=\"fields {}\">",
                if columns.fields_compact {
                    "compact"
                } else {
                    ""
                }
            ));
            for id in fields {
                out.push("<tr>");
                let enum_value = self.enum_value(col, id);
                let item = self.parser.item(id);
                let (name, title) = (item.name.clone(), item.title.clone());
                if columns.fields_compact {
                    let marker = self.deprecated_marker(id);
                    let link = self.permalink(&name);
                    out.line(format_args!(
                        "<td class=\"name\"><var id=\"{name}\">{title}</var>{enum_value}{marker}{link}</td>"
                    ));
                } else {
                    out.line(format_args!(
                        "<td class=\"name\"><a href=\"#{name}\"><var>{title}</var></a>{enum_value}</td>"
                    ));
                }
                let mut nmeta = columns.fields_meta;
                let types = self.parser.item(id).types.clone();
                if !types.is_empty() {
                    let html = self.types_html(&types);
                    out.line(format_args!("<td class=\"meta types\">{html}</td>"));
                } else if columns.fields_has_type {
                    out.push("<td class=\"meta\"></td>");
                }
                if let Some(meta) = self.parser.item(id).meta.clone().filter(|m| !m.is_empty()) {
                    let html = self.markdown(&meta);
                    out.line(format_args!("<td class=\"meta\">{html}</td>"));
                    nmeta = nmeta.saturating_sub(1);
                }
                for _ in 0..nmeta {
                    out.push("<td class=\"meta\"></td>");
                }
                let html = if columns.fields_compact {
                    self.synopsis_whole(id)
                } else {
                    self.synopsis_first_sentence(id)
                };
                if !html.is_empty() {
                    out.line(format_args!("<td class=\"doc\">{html}</td>"));
                }
                out.push("</tr>");
            }
            out.push("</table>");
        }

        if !functions.is_empty() {
            if !self.parser.item(col).fields.is_empty() || !columns.functions_compact {
                out.line(format_args!(
                    "<div class=\"heading\">{}</div>",
                    columns.functions_title
                ));
            }
            out.line(format_args!(
                "<table class=\"functions {}\">",
                if columns.functions_compact {
                    "compact"
                } else {
                    ""
                }
            ));
            for id in functions {
                out.push("<tr>");
                let item = self.parser.item(id);
                let in_class = item
                    .scope()
                    .is_some_and(|s| self.parser.item(s).kind == Kind::Class);
                let display = if in_class {
                    self.parser.display_compact(id)
                } else {
                    item.title.clone()
                };
                let name = self.parser.item(id).name.clone();
                if columns.functions_compact {
                    let params = self
                        .parser
                        .item(id)
                        .params
                        .iter()
                        .map(|p| format!("<em>{}</em>", p.name))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let marker = self.deprecated_marker(id);
                    let link = self.permalink(&name);
                    out.line(format_args!(
                        "<td class=\"name\"><var id=\"{name}\">{display}</var>({params}){marker}{link}</td>"
                    ));
                } else {
                    out.line(format_args!(
                        "<td class=\"name\"><a href=\"#{name}\"><var>{display}</var></a>()</td>"
                    ));
                }
                let mut nmeta = columns.functions_meta;
                if let Some(meta) = self.parser.item(id).meta.clone().filter(|m| !m.is_empty()) {
                    // Raw, unlike a field's, which goes through markdown.
                    out.line(format_args!("<td class=\"meta\">{meta}</td>"));
                    nmeta = nmeta.saturating_sub(1);
                }
                for _ in 0..nmeta {
                    out.push("<td class=\"meta\"></td>");
                }
                let html = if columns.functions_compact {
                    self.synopsis_whole(id)
                } else {
                    self.synopsis_first_sentence(id)
                };
                out.line(format_args!("<td class=\"doc\">{html}</td>"));
                out.push("</tr>");
            }
            out.push("</table>");
        }
        out.push("</div>");
    }

    /// A synopsis cell of a row that has a detail box below it: the first sentence.
    fn synopsis_first_sentence(&mut self, id: ItemId) -> String {
        self.parser.resolve_item_content(id);
        let first = self
            .parser
            .peek_first_sentence(&self.parser.item(id).content);
        self.markdown(&first)
    }

    /// A synopsis cell of a compact row, which has no detail box to hold the rest: the
    /// whole documentation minus a leading Deprecated box, whose marker already carries
    /// that signal.
    fn synopsis_whole(&mut self, id: ItemId) -> String {
        self.parser.resolve_item_content(id);
        let content = &self.parser.item(id).content;
        let deprecated = self.parser.item(id).flags.deprecated.is_some();
        let leading_admonition = matches!(content.0.first(), Some(Fragment::Admonition { .. }));
        if deprecated && leading_admonition {
            return self.fragments_html(content.0.get(1..).unwrap_or(&[]));
        }
        self.content_html(content)
    }

    fn enum_value(&self, col: ItemId, id: ItemId) -> String {
        let is_enum = self.parser.item(col).flags.is_enum;
        match self
            .parser
            .item(id)
            .value
            .as_deref()
            .filter(|v| !v.is_empty())
        {
            Some(value) if is_enum => format!(" = <span class=\"value\">{value}</span>"),
            _ => String::new(),
        }
    }

    fn deprecated_marker(&self, id: ItemId) -> String {
        if self.parser.item(id).flags.deprecated.is_some() {
            "<span class=\"tag deprecated\">deprecated</span>".to_string()
        } else {
            String::new()
        }
    }

    fn field_details(&mut self, col: ItemId, columns: &Columns, out: &mut Lines) {
        let fields = self.parser.item(col).fields.clone();
        if fields.is_empty() || columns.fields_compact {
            return;
        }
        if !self.parser.item(col).functions.is_empty() {
            out.line(format_args!(
                "<h3 class=\"fields\">{}</h3>",
                columns.fields_title
            ));
        }
        out.push("<dl class=\"fields\">");
        for id in fields {
            let enum_value = self.enum_value(col, id);
            let item = self.parser.item(id);
            let (name, display) = (item.name.clone(), item.display.clone());
            out.line(format_args!("<dt id=\"{name}\">"));
            out.line(format_args!(
                "<span class=\"icon\"></span><var>{display}</var>{enum_value}"
            ));
            let types = self.parser.item(id).types.clone();
            if !types.is_empty() {
                let html = self.types_html(&types);
                out.line(format_args!("<span class=\"tag type\">{html}</span>"));
            }
            if let Some(meta) = self.parser.item(id).meta.clone().filter(|m| !m.is_empty()) {
                out.line(format_args!("<span class=\"tag meta\">{meta}</span>"));
            }
            out.push(&self.since(id));
            out.push(&self.permalink(&name));
            out.push("</dt>");
            out.push("<dd>");
            self.parser.resolve_item_content(id);
            let content = &self.parser.item(id).content;
            out.push(&self.content_html(content));
            out.push("</dd>");
        }
        out.push("</dl>");
    }

    fn function_details(&mut self, col: ItemId, columns: &Columns, out: &mut Lines) {
        let functions = self.parser.item(col).functions.clone();
        if functions.is_empty() || columns.functions_compact {
            return;
        }
        if !self.parser.item(col).fields.is_empty() {
            out.line(format_args!(
                "<h3 class=\"functions\">{}</h3>",
                columns.functions_title
            ));
        }
        out.push("<dl class=\"functions\">");
        for id in functions {
            let item = self.parser.item(id);
            let (name, display) = (item.name.clone(), item.display.clone());
            let params = item
                .params
                .iter()
                .map(|p| format!("<em>{}</em>", p.name))
                .collect::<Vec<_>>()
                .join(", ");
            out.line(format_args!("<dt id=\"{name}\">"));
            out.line(format_args!(
                "<span class=\"icon\"></span><var>{display}</var>({params})"
            ));
            if let Some(meta) = self.parser.item(id).meta.clone().filter(|m| !m.is_empty()) {
                out.line(format_args!("<span class=\"tag meta\">{meta}</span>"));
            }
            out.push(&self.since(id));
            out.push(&self.permalink(&name));
            out.push("</dt>");
            out.push("<dd>");
            self.parser.resolve_item_content(id);
            let content = &self.parser.item(id).content;
            out.push(&self.content_html(content));

            let params = self.parser.item(id).params.clone();
            // Only when at least one parameter carries a type or a description.
            if params
                .iter()
                .any(|p| !p.types.is_empty() || !p.content.is_empty())
            {
                out.push("<div class=\"heading\">Parameters</div>");
                out.push("<table class=\"parameters\">");
                for param in &params {
                    out.push("<tr>");
                    out.line(format_args!(
                        "<td class=\"name\"><var>{}</var></td>",
                        param.name
                    ));
                    let types = self.types_html(&param.types);
                    out.line(format_args!("<td class=\"types\">({types})</td>"));
                    let mut content = param.content.clone();
                    self.parser.resolve_content(&mut content);
                    let html = self.content_html(&content);
                    out.line(format_args!("<td class=\"doc\">{html}</td>"));
                    out.push("</tr>");
                }
                out.push("</table>");
            }

            let returns = self.parser.item(id).returns.clone();
            if !returns.is_empty() {
                out.push("<div class=\"heading\">Return Values</div>");
                out.push("<table class=\"returns\">");
                for (n, ret) in returns.iter().enumerate() {
                    out.push("<tr>");
                    if returns.len() > 1 {
                        out.line(format_args!("<td class=\"name\">{}.</td>", n + 1));
                    }
                    let types = self.types_html(&ret.types);
                    out.line(format_args!("<td class=\"types\">({types})</td>"));
                    let mut content = ret.content.clone();
                    self.parser.resolve_content(&mut content);
                    let html = self.content_html(&content);
                    out.line(format_args!("<td class=\"doc\">{html}</td>"));
                    out.push("</tr>");
                }
                out.push("</table>");
            }
            out.push("</dd>");
        }
        out.push("</dl>");
    }

    // -- the search page, the landing page and the index ------------------------

    fn search_page(&mut self) -> String {
        let mut lines = Lines::new();
        self.frame_open(self.search, &mut lines);
        let root = self.root();
        let fields = BTreeMap::from([("root", root.as_str()), ("version", self.version.as_str())]);
        lines.push(&format_template(&self.templates.search, &fields));
        self.frame_close(&mut lines);
        lines.finish()
    }

    /// The same frame with an empty body, reusing the search pseudo-page so the link
    /// paths come out right.
    fn landing_page(&mut self) -> String {
        let mut lines = Lines::new();
        self.frame_open(self.search, &mut lines);
        self.frame_close(&mut lines);
        lines.finish()
    }

    fn search_index(&mut self) -> String {
        self.parser.focus(self.search);
        let mut lines = Lines::new();
        lines.push("var docs = [");
        for kind in [
            Kind::Class,
            Kind::Module,
            Kind::Field,
            Kind::Function,
            Kind::Section,
        ] {
            let ids = self.parser.of_kind(kind).to_vec();
            for id in ids {
                lines.push(&self.search_entry(id, kind));
            }
        }
        lines.push("];");
        lines.finish()
    }

    fn search_entry(&mut self, id: ItemId, kind: Kind) -> String {
        self.parser.resolve_item_content(id);
        let href = self.href(id);
        let content = &self.parser.item(id).content;
        let mut text = self.content_text(content);
        let mut title = self.parser.item(id).display.clone();

        let on_manual = self.parser.item(self.parser.topref(id)).kind == Kind::Manual;
        if kind == Kind::Section && !on_manual {
            // A non-manual section usually reads better summarised by its first sentence.
            // A leading admonition is not the summary, so it stays in the body text.
            let at = content
                .0
                .iter()
                .position(|f| !matches!(f, Fragment::Admonition { .. }))
                .unwrap_or(content.0.len());
            let lead = content.0.get(..at).unwrap_or(&[]);
            let body = content.0.get(at..).unwrap_or(&[]);
            let (first, remaining) = {
                let flattened = self.fragments_text(body);
                let (first, rest) = util::first_sentence(&flattened);
                (first.to_string(), rest.to_string())
            };
            if first.chars().count() < 80 {
                title = first;
                text = format!("{} {remaining}", self.fragments_text(lead))
                    .trim()
                    .to_string();
            }
        }
        let escape = |s: &str| s.replace('"', "\\\"").replace('\n', " ");
        text = escape(&text);
        title = escape(&title);
        if kind == Kind::Module {
            title = title
                .split_once('.')
                .map(|(_, rest)| rest.to_string())
                .unwrap_or(title);
        }
        format!(
            "{{path:\"{href}\", type:\"{}\", title:\"{title}\", text:\"{text}\"}},",
            kind.as_str()
        )
    }
}

struct Columns {
    fields_title: &'static str,
    fields_meta: usize,
    fields_has_type: bool,
    functions_title: &'static str,
    functions_meta: usize,
    fields_compact: bool,
    functions_compact: bool,
}

/// `<p>…</p>` with the tags sliced off, which is what the Python's `[3:-4]` does.
fn strip_paragraph(html: &str) -> String {
    let body = html.strip_prefix("<p>").unwrap_or(html);
    body.strip_suffix("</p>").unwrap_or(body).to_string()
}

fn basename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// The favicon's media type. `mimetypes.guess_type` in the Python is the host's, seeded
/// from the registry or `/etc/mime.types`, which spec/html.md section 12.9 names as a real
/// portability hazard: the same favicon can produce a different `type=` on another
/// machine. This table is fixed, which is the fix rather than the bug.
fn mimetype_of(path: &str) -> Option<&'static str> {
    let extension = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())?;
    Some(match extension.as_str() {
        "png" => "image/png",
        "gif" => "image/gif",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "ico" => "image/vnd.microsoft.icon",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_template_field_is_substituted_and_an_unknown_one_is_left_alone() {
        let fields = BTreeMap::from([("root", "../"), ("version", "abc1234")]);
        assert_eq!(
            format_template("<a href=\"{root}x.css?{version}\">{unknown}", &fields),
            "<a href=\"../x.css?abc1234\">{unknown}"
        );
    }

    #[test]
    fn the_see_also_slice_removes_one_paragraph() {
        assert_eq!(strip_paragraph("<p>a, b</p>"), "a, b");
        // An empty `@see` still prints the label, which is what the Python's
        // unconditional slice does.
        assert_eq!(strip_paragraph(""), "");
    }

    #[test]
    fn the_favicon_type_is_a_fixed_table() {
        assert_eq!(mimetype_of("favicon96x96.png"), Some("image/png"));
        assert_eq!(mimetype_of("a/b/icon.SVG"), Some("image/svg+xml"));
        assert_eq!(mimetype_of("icon.unknown"), None);
    }

    #[test]
    fn a_basename_is_taken_with_either_separator() {
        assert_eq!(basename("../../a/b/c.css"), "c.css");
        assert_eq!(basename("a\\b\\c.css"), "c.css");
        assert_eq!(basename("c.css"), "c.css");
    }
}
