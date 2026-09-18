//! The document: every documentable element, in one arena.
//!
//! The Python's `Reference` is a mutable dataclass that is re-typed in place, caches its
//! derived names lazily behind a `clear_cache()` dance, and carries a `Dict[str, Any]`
//! flag bag inside a cyclic object graph. Here an element is an index, its kind is an
//! enum, its derived names are computed once when it is registered, and every flag is a
//! named field -- the `Any` bag existed only because every consumer already knew which
//! flag it wanted.

use crate::tags::Tag;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(pub u32);

impl ItemId {
    pub fn index(self) -> usize {
        self.0 as usize
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
            Kind::Class => "class",
            Kind::Field => "field",
            Kind::Function => "function",
            Kind::Manual => "manual",
            Kind::Module => "module",
            Kind::Pseudo => "",
            Kind::Section => "section",
            Kind::Table => "table",
        }
    }

    /// Classes, modules and manual pages each render to their own page, and every other
    /// element traces up to one of them.
    pub fn is_top(self) -> bool {
        matches!(
            self,
            Kind::Class | Kind::Module | Kind::Manual | Kind::Pseudo
        )
    }

    /// A collection can hold fields and functions. Classes and modules count themselves
    /// as their own first collection, which is what makes enumerating a page uniform.
    pub fn is_collection(self) -> bool {
        matches!(
            self,
            Kind::Class | Kind::Module | Kind::Manual | Kind::Pseudo | Kind::Section | Kind::Table
        )
    }

    /// Modules, classes, tables and manual pages can be a scope; a section cannot.
    pub fn is_scope(self) -> bool {
        matches!(
            self,
            Kind::Class | Kind::Module | Kind::Manual | Kind::Table
        )
    }
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
    pub compact: Option<Vec<String>>,
    pub fullnames: bool,
    pub meta: Option<String>,
    pub types: Option<Vec<String>>,
    pub order: Option<Order>,
    pub is_enum: bool,
    /// Heading level, for a section of a manual page.
    pub level: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub whence: String,
    pub anchor: Option<String>,
}

/// One line of an unparsed documentation block: where it came from, its text, and the
/// tags found on it that the scanner did not consume. `tags: None` marks a manual page,
/// whose lines carry tags without a comment prefix.
#[derive(Debug, Clone)]
pub struct RawLine {
    pub line: u32,
    pub text: String,
    pub tags: Option<Vec<Tag>>,
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
    pub fn new(resolve: bool) -> Markdown {
        Markdown {
            lines: Vec::new(),
            value: None,
            resolve,
        }
    }

    pub fn resolved(value: String) -> Markdown {
        Markdown {
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
    pub fn raw(&self) -> String {
        match &self.value {
            Some(v) => v.clone(),
            None => self.lines.join("\n"),
        }
    }

    pub fn is_resolved(&self) -> bool {
        self.value.is_some()
    }

    pub fn set_resolved(&mut self, value: String) {
        self.lines.clear();
        self.value = Some(value);
    }

    pub fn get(&self) -> String {
        self.raw()
    }
}

#[derive(Debug, Clone)]
pub enum Fragment {
    Markdown(Markdown),
    Admonition {
        level: String,
        title: String,
        content: Content,
    },
    SeeAlso(Vec<String>),
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
    /// Nesting depth of the scopes this element sits in; -1 for an implicit module.
    pub level: i32,
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
    pub id: String,

    // Filled in by the prerender stage.
    pub content: Content,
    pub heading: String,
    pub title: String,
    pub types: Vec<String>,
    pub meta: Option<String>,
    pub params: Vec<Param>,
    pub returns: Vec<Returned>,
    pub compact: Vec<String>,
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
    pub fn new(kind: Kind, file: &str, line: Option<u32>, symbol: &str) -> Item {
        Item {
            kind,
            file: file.to_string(),
            line,
            symbol: symbol.to_string(),
            original_symbol: None,
            implicit: false,
            level: 0,
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
            id: String::new(),
            content: Content::default(),
            heading: String::new(),
            title: String::new(),
            types: Vec::new(),
            meta: None,
            params: Vec::new(),
            returns: Vec::new(),
            compact: Vec::new(),
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
}
