//! The document: every documentable element, in one arena.
//!
//! The Python's `Reference` is a mutable dataclass that is re-typed in place, caches its
//! derived names lazily behind a `clear_cache()` dance, and carries a `Dict[str, Any]`
//! flag bag inside a cyclic object graph. Here an element is an index, its kind is an
//! enum, its derived names are computed once when it is registered, and every flag is a
//! named field -- the `Any` bag existed only because every consumer already knew which
//! flag it wanted.

use crate::tags::Tag;
use std::borrow::Cow;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(pub u32);

impl ItemId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// An element's opaque id: the BLAKE2b-160 digest `util::ref_id` computes, hex encoded.
/// It is what `doc.json` writes for an element and what every `luadox:<id>` link target
/// carries; the link protocol is the one place it is read back from a string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RefId(String);

impl RefId {
    /// The id of an element `assign_ids` has not reached.
    pub const UNASSIGNED: Self = Self(String::new());

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RefId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for RefId {
    fn from(hex: String) -> Self {
        Self(hex)
    }
}

impl From<&str> for RefId {
    fn from(hex: &str) -> Self {
        Self(hex.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Class,
    Field,
    Function,
    Manual,
    Module,
    /// The html renderer's search pseudo-page, whose type really is the empty string:
    /// it is what makes the search and landing pages' body class `other-search`.
    Pseudo,
    Section,
    Table,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Field => "field",
            Self::Function => "function",
            Self::Manual => "manual",
            Self::Module => "module",
            Self::Pseudo => "",
            Self::Section => "section",
            Self::Table => "table",
        }
    }

    /// Classes, modules and manual pages each render to their own page, and every other
    /// element traces up to one of them.
    pub fn is_top(self) -> bool {
        matches!(
            self,
            Self::Class | Self::Module | Self::Manual | Self::Pseudo
        )
    }

    /// A collection can hold fields and functions. Classes and modules count themselves
    /// as their own first collection, which is what makes enumerating a page uniform.
    pub fn is_collection(self) -> bool {
        matches!(
            self,
            Self::Class | Self::Module | Self::Manual | Self::Pseudo | Self::Section | Self::Table
        )
    }

    /// Modules, classes, tables and manual pages can be a scope; a section cannot.
    pub fn is_scope(self) -> bool {
        matches!(
            self,
            Self::Class | Self::Module | Self::Manual | Self::Table
        )
    }
}

/// What a top-level element is in the html output. The landing page is the manual page
/// named `index` and the search page is the one `Kind::Pseudo` element; both sit at the
/// document root, where every other page sits one directory down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Landing,
    Search,
    Class,
    Module,
    Manual,
}

/// Modifiers accumulated from tags. One named field per flag, so a consumer asks for the
/// flag it wants instead of indexing a bag of `Any`.
#[derive(Debug, Clone, Default)]
pub struct Flags {
    pub display: Option<String>,
    pub rename: Option<String>,
    pub scope: Option<String>,
    pub since: Option<String>,
    /// Present when `@deprecated` was given; the string is the explanation, empty for a
    /// bare tag.
    pub deprecated: Option<String>,
    pub inherits: Vec<String>,
    /// The member kinds `@compact` named; empty when the tag was not given.
    pub compact: Vec<Member>,
    pub fullnames: bool,
    pub meta: Option<String>,
    pub types: Option<Vec<String>>,
    pub order: Option<Order>,
    pub is_enum: bool,
}

/// The kinds of member a collection has, which is what `@compact` names: the members
/// rendered as one-line rows without a detail box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Member {
    Fields,
    Functions,
}

impl Member {
    pub const ALL: [Self; 2] = [Self::Fields, Self::Functions];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fields => "fields",
            Self::Functions => "functions",
        }
    }
}

/// Where `@order` puts an element among its siblings. The anchor is another sibling's
/// symbol, and only the relative placements have one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Order {
    First,
    Last,
    Before(String),
    After(String),
}

/// One line of an unparsed documentation block: where it came from and its text.
#[derive(Debug, Clone)]
pub enum RawLine {
    /// A `--` comment line of a Lua source, with the tags found on it that the scanner
    /// did not consume.
    Source {
        line: u32,
        text: String,
        tags: Vec<Tag>,
    },
    /// A line of a manual page, whose tags carry no comment prefix and are found when the
    /// block is assembled.
    Manual { line: u32, text: String },
}

impl RawLine {
    pub fn line(&self) -> u32 {
        match self {
            Self::Source { line, .. } | Self::Manual { line, .. } => *line,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::Source { text, .. } | Self::Manual { text, .. } => text,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Markdown {
    lines: Vec<String>,
    value: Option<String>,
    /// Whether cross references in the text still have to be resolved to links. False for
    /// text that was resolved before being split, such as the remainder left behind by
    /// `first_sentence`.
    pub resolve: bool,
}

impl Markdown {
    pub fn new(resolve: bool) -> Self {
        Self {
            lines: Vec::new(),
            value: None,
            resolve,
        }
    }

    pub fn resolved(value: String) -> Self {
        Self {
            lines: Vec::new(),
            value: Some(value),
            resolve: false,
        }
    }

    pub fn append(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    /// Drops trailing whitespace from everything accumulated so far, which is how a code
    /// block avoids a blank line before its closing fence.
    pub fn rstrip(&mut self) {
        let joined = self.lines.join("\n");
        self.lines = vec![joined.trim_end().to_string()];
    }

    /// The text as written, before cross references are resolved.
    pub fn raw(&self) -> Cow<'_, str> {
        match &self.value {
            Some(v) => Cow::Borrowed(v),
            None => Cow::Owned(self.lines.join("\n")),
        }
    }

    pub fn is_resolved(&self) -> bool {
        self.value.is_some()
    }

    pub fn set_resolved(&mut self, value: String) {
        self.lines.clear();
        self.value = Some(value);
    }

    pub fn get(&self) -> Cow<'_, str> {
        self.raw()
    }
}

/// The kinds of admonition a block can carry: the html renderer's CSS class, and the
/// json renderer's `level`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmonitionLevel {
    Note,
    Warning,
    Deprecated,
}

impl AdmonitionLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Warning => "warning",
            Self::Deprecated => "deprecated",
        }
    }
}

/// What one `@see` entry resolved to. An element whose name was already taken lost its
/// registration and cannot be reached by id, so the id is carried raw: the json renderer
/// still writes it, the LuaLS renderer prints it in place of a name, and the html renderer
/// drops it -- which is what the Python does, for the same reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeeRef {
    Item(ItemId),
    Unreachable(RefId),
}

impl SeeRef {
    pub fn item(&self) -> Option<ItemId> {
        match self {
            Self::Item(id) => Some(*id),
            Self::Unreachable(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Fragment {
    Markdown(Markdown),
    Admonition {
        level: AdmonitionLevel,
        title: String,
        content: Content,
    },
    SeeAlso(Vec<SeeRef>),
}

#[derive(Debug, Clone, Default)]
pub struct Content(pub Vec<Fragment>);

impl Content {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn push(&mut self, fragment: Fragment) {
        self.0.push(fragment);
    }

    pub fn insert(&mut self, at: usize, fragment: Fragment) {
        self.0.insert(at.min(self.0.len()), fragment);
    }

    /// The trailing markdown fragment, appending a new one when the last fragment is
    /// something else.
    pub fn md(&mut self, resolve: bool) -> &mut Markdown {
        if !matches!(self.0.last(), Some(Fragment::Markdown(_))) {
            self.0.push(Fragment::Markdown(Markdown::new(resolve)));
        }
        match self.0.last_mut() {
            Some(Fragment::Markdown(md)) => md,
            _ => unreachable!("the last fragment is markdown by construction"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Param {
    pub name: String,
    pub types: Vec<String>,
    pub content: Content,
}

#[derive(Debug, Clone, Default)]
pub struct Returned {
    pub types: Vec<String>,
    pub content: Content,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub kind: Kind,
    pub file: String,
    pub line: Option<u32>,
    /// As written, so `Class:method` keeps its colon.
    pub symbol: String,
    /// The symbol before `@rename` replaced it.
    pub original_symbol: Option<String>,
    pub implicit: bool,
    /// Nesting depth of the table braces this element sits in, which is what closes a
    /// `@table` scope; -1 for an implicit module or a manual page, which no brace opened.
    pub depth: i32,
    /// The markdown heading level of a section of a manual page. `None` for a section a
    /// Lua source declared, which has no heading of its own.
    pub heading_level: Option<u8>,
    pub scopes: Vec<ItemId>,
    pub within: Option<String>,
    pub collection: Option<ItemId>,
    pub raw_content: Vec<RawLine>,
    pub flags: Flags,
    /// Parameter names as the source writes them, for a function.
    pub args: Vec<String>,
    /// The literal right-hand side of a field's assignment.
    pub value: Option<String>,

    // Derived once when the element is registered, rather than cached lazily.
    pub name: String,
    pub display: String,
    pub topsym: String,
    pub id: RefId,

    // Filled in by the prerender stage.
    pub content: Content,
    pub heading: String,
    pub title: String,
    pub types: Vec<String>,
    pub meta: Option<String>,
    pub params: Vec<Param>,
    pub returns: Vec<Returned>,
    /// For a top-level element: its collections, in render order.
    pub collections: Vec<ItemId>,
    /// This element's documentation reduced to what a one-line context can present,
    /// when it is in a `@compact` collection and therefore gets no detail box. `None`
    /// when it is not in one, or when the documentation did not fit -- see
    /// `markdown::RowContent`.
    pub row: Option<crate::markdown::RowContent>,
    /// For a collection: the elements it contains, in render order.
    pub fields: Vec<ItemId>,
    pub functions: Vec<ItemId>,
    /// Whether the page this element is on has anything to render.
    pub empty: bool,
}

impl Item {
    pub fn new(kind: Kind, file: &str, line: Option<u32>, symbol: &str) -> Self {
        Self {
            kind,
            file: file.to_string(),
            line,
            symbol: symbol.to_string(),
            original_symbol: None,
            implicit: false,
            depth: 0,
            heading_level: None,
            scopes: Vec::new(),
            within: None,
            collection: None,
            raw_content: Vec::new(),
            flags: Flags::default(),
            args: Vec::new(),
            value: None,
            name: String::new(),
            display: String::new(),
            topsym: String::new(),
            id: RefId::UNASSIGNED,
            content: Content::default(),
            heading: String::new(),
            title: String::new(),
            types: Vec::new(),
            meta: None,
            params: Vec::new(),
            returns: Vec::new(),
            collections: Vec::new(),
            row: None,
            fields: Vec::new(),
            functions: Vec::new(),
            empty: false,
        }
    }

    pub fn scope(&self) -> Option<ItemId> {
        self.scopes.last().copied()
    }

    /// The page this element is, for a top-level element; `None` for one that lives on
    /// another's page.
    pub fn page(&self) -> Option<Page> {
        Some(match self.kind {
            Kind::Class => Page::Class,
            Kind::Module => Page::Module,
            Kind::Manual if self.name == "index" => Page::Landing,
            Kind::Manual => Page::Manual,
            Kind::Pseudo => Page::Search,
            Kind::Field | Kind::Function | Kind::Section | Kind::Table => return None,
        })
    }
}
