//! Scanning documentation blocks out of Lua source, and the registry every later stage
//! reads.
//!
//! The scan itself stays line-driven, because that is what luadox's semantics are: a
//! block of `---` comments documents whatever is written on the next line, and a `@table`
//! stays open until its braces balance. What changed is where the facts come from. "Is
//! there a declaration on this line, and what does it say" is now answered by `lua`, from
//! a parse tree, instead of by a regular expression over the raw text.

use std::collections::{HashMap, HashSet};

use crate::diag::{Category, Diagnostics};
use crate::ir::{Flags, Item, ItemId, Kind, Order, RawLine, RefId, SeeRef};
use crate::lua::{self, SourceFile};
use crate::settings::Settings;
use crate::tags::{self, Tag};
use crate::util;

/// The file and element a message is about. The Python keeps one of these on the parser
/// and mutates it from everywhere; it is carried explicitly here because the *value at
/// the time of a call* decides how a cross reference resolves.
#[derive(Debug, Default, Clone)]
pub struct Context {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub item: Option<ItemId>,
}

/// A documentation block while it is being scanned: the comment lines seen so far and
/// what their tags said, before anything says which element it documents.
///
/// It is not in the arena. A block that turns out to document nothing is reported and
/// dropped, so no element exists that was never registered, and it becomes an `Item` at
/// exactly one point -- `Parser::enter` -- once a declaration names it.
struct Block {
    /// The line the block opened on, where a disconnected block is reported.
    line: u32,
    raw_content: Vec<RawLine>,
    flags: Flags,
    within: Option<String>,
    /// `@alias` names, bound to the element the block declares.
    aliases: Vec<String>,
}

impl Block {
    fn new(line: u32) -> Self {
        Self {
            line,
            raw_content: Vec::new(),
            flags: Flags::default(),
            within: None,
            aliases: Vec::new(),
        }
    }

    /// Whether the block said anything a reader would miss if it were dropped.
    fn said_something(&self) -> bool {
        self.raw_content
            .iter()
            .any(|l| !l.text().trim_start_matches('-').trim().is_empty())
    }

    /// The element this block documents, now that a declaration names it.
    fn declare(self, kind: Kind, symbol: &str, file: &str, line: u32) -> Item {
        let mut item = Item::new(kind, file, Some(line), symbol);
        item.raw_content = self.raw_content;
        item.flags = self.flags;
        item.within = self.within;
        item
    }
}

/// The block a comment line belongs to. A collection tag declares the block on the spot,
/// because from that line on it is the scope or collection that its own `@field`s and
/// every later block refer to; it is registered when the block ends.
enum Current {
    Block(Box<Block>),
    Declared(ItemId),
}

/// The parts of a block a tag writes to, borrowed from wherever the block lives.
struct Scanned<'a> {
    flags: &'a mut Flags,
    within: &'a mut Option<String>,
    raw_content: &'a mut Vec<RawLine>,
}

/// How a cross reference's link text is set: as code, when the reference was written in
/// backticks, or as plain text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefStyle {
    Plain,
    Code,
}

pub struct Parser {
    pub items: Vec<Item>,
    /// Fully qualified name -> element, including `@alias` names.
    pub refs: HashMap<String, ItemId>,
    /// Opaque id -> element, for a `@see` list and the LuaLS mixin phrase.
    by_id: HashMap<RefId, ItemId>,
    /// Top-level elements, in the order they were registered.
    pub topsyms: Vec<ItemId>,
    topsym_index: HashMap<String, ItemId>,
    /// Top-level symbol -> its collections, in declaration order. A class or module is
    /// its own first collection, which is what makes enumerating a page uniform.
    pub collections: Vec<(String, Vec<ItemId>)>,
    collection_index: HashMap<String, usize>,
    /// Every registered element by kind, in registration order.
    by_kind: HashMap<Kind, Vec<ItemId>>,
    /// Elements `add_reference` has registered. An implicit module whose name conflicts
    /// never owns a top-level symbol, so every element of its file that nothing else
    /// anchors asks for it to be registered again; the second time is a report, not a
    /// second registration.
    registered: HashSet<ItemId>,
    /// Elements whose `name`/`display` and `topsym` have been derived. The Python caches
    /// both behind lazy properties, and the laziness is load-bearing: an element asks its
    /// scope for a name long before that scope is itself registered.
    named: HashSet<ItemId>,
    topsymed: HashSet<ItemId>,
    /// Elements whose `@within` target was traced to a page, once per element.
    pub within_topsym: HashMap<ItemId, String>,
    pub diagnostics: Diagnostics,
    pub settings: Settings,
    pub ctx: Context,
    /// Modules discovered through `require()`, for a run that follows them.
    pub requires: Vec<String>,
    /// `@alias` names, bound once the block they were written in has become an element.
    aliases: Vec<(String, ItemId)>,
}

impl Parser {
    pub fn new(settings: Settings) -> Self {
        Self {
            items: Vec::new(),
            refs: HashMap::new(),
            by_id: HashMap::new(),
            topsyms: Vec::new(),
            topsym_index: HashMap::new(),
            collections: Vec::new(),
            collection_index: HashMap::new(),
            by_kind: HashMap::new(),
            registered: HashSet::new(),
            named: HashSet::new(),
            topsymed: HashSet::new(),
            within_topsym: HashMap::new(),
            diagnostics: Diagnostics::allowing(settings.allow_incomplete.clone()),
            settings,
            ctx: Context::default(),
            requires: Vec::new(),
            aliases: Vec::new(),
        }
    }

    // -- arena ------------------------------------------------------------------

    fn push(&mut self, item: Item) -> ItemId {
        let id = ItemId(self.items.len() as u32);
        self.items.push(item);
        id
    }

    pub fn item(&self, id: ItemId) -> &Item {
        // Every ItemId comes from `push`, so the index is always in range; an empty
        // placeholder beats panicking in a documentation tool.
        self.items.get(id.index()).unwrap_or(&EMPTY)
    }

    pub fn item_mut(&mut self, id: ItemId) -> &mut Item {
        match self.items.get_mut(id.index()) {
            Some(item) => item,
            None => unreachable!("every ItemId comes from push()"),
        }
    }

    /// Points the context at an element, the way the Python's `ctx.update(ref=...)`
    /// does. What is focused when a cross reference is resolved decides what it resolves
    /// to, so the focus is set explicitly at every point the Python sets it.
    pub fn focus(&mut self, id: ItemId) {
        let (file, line) = {
            let item = self.item(id);
            (item.file.clone(), item.line)
        };
        self.ctx.file = Some(file);
        self.ctx.line = line;
        self.ctx.item = Some(id);
    }

    pub fn of_kind(&self, kind: Kind) -> &[ItemId] {
        self.by_kind.get(&kind).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The page an element belongs to, honouring neither `@within` nor `@order`.
    pub fn topref(&self, id: ItemId) -> ItemId {
        let item = self.item(id);
        if item.scopes.is_empty() {
            return id;
        }
        self.topsym_index.get(&item.topsym).copied().unwrap_or(id)
    }

    pub fn topsym_item(&self, name: &str) -> Option<ItemId> {
        self.topsym_index.get(name).copied()
    }

    fn collections_of(&self, topsym: &str) -> &[ItemId] {
        self.collection_index
            .get(topsym)
            .and_then(|i| self.collections.get(*i))
            .map(|(_, v)| v.as_slice())
            .unwrap_or(&[])
    }

    // -- registration -----------------------------------------------------------

    /// Registers an element, reporting a duplicate rather than replacing one.
    ///
    /// `modref` is the file's implicit module: when nothing in the element's scopes is a
    /// page yet, the module is registered first so the element has somewhere to live.
    fn add_reference(&mut self, id: ItemId, modref: Option<ItemId>) {
        if self.registered.contains(&id) {
            let (name, file, line) = self.locate(id);
            self.diagnostics.add(
                Category::Conflicts,
                format!("reference \"{name}\" with the same name already exists"),
                Some(&file),
                line,
            );
            return;
        }

        // A field documented inside a method is written `self.x`; the prefix is the
        // method's receiver, not part of the name.
        if matches!(self.item(id).kind, Kind::Field | Kind::Function) {
            if let Some(rest) = self.item(id).symbol.strip_prefix("self.") {
                let rest = rest.to_string();
                self.item_mut(id).symbol = rest;
            }
        }

        self.ensure_name(id);
        self.ensure_topsym(id);

        let kind = self.item(id).kind;
        if kind.is_top() {
            let name = self.item(id).name.clone();
            match self.topsym_index.entry(name.clone()) {
                std::collections::hash_map::Entry::Occupied(_) => {
                    let (_, file, line) = self.locate(id);
                    self.diagnostics.add(
                        Category::Conflicts,
                        format!("{name} conflicts with another class or module"),
                        Some(&file),
                        line,
                    );
                }
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(id);
                    self.topsyms.push(id);
                }
            }
        } else {
            let anchored = self.item(id).scopes.clone().into_iter().rev().any(|s| {
                self.ensure_name(s);
                self.topsym_index.contains_key(&self.item(s).name)
            });
            if !anchored {
                if let Some(modref) = modref {
                    self.add_reference(modref, None);
                }
            }
        }

        if kind.is_collection() && kind != Kind::Manual {
            let topsym = self.item(id).topsym.clone();
            let symbol = self.item(id).symbol.clone();
            let slot = match self.collection_index.get(&topsym) {
                Some(i) => *i,
                None => {
                    self.collections.push((topsym.clone(), Vec::new()));
                    let i = self.collections.len() - 1;
                    self.collection_index.insert(topsym, i);
                    i
                }
            };
            let exists = self
                .collections
                .get(slot)
                .is_some_and(|(_, v)| v.iter().any(|c| self.item(*c).symbol == symbol));
            if !exists {
                if let Some((_, v)) = self.collections.get_mut(slot) {
                    v.push(id);
                }
            }
        }

        self.by_kind.entry(kind).or_default().push(id);
        self.registered.insert(id);

        let name = self.item(id).name.clone();
        match self.refs.get(&name).copied() {
            Some(existing) => self.report_name_conflict(id, existing, &name),
            None => {
                self.refs.insert(name, id);
            }
        }
    }

    /// A name that is already taken is a conflict unless both are sections, which are not
    /// qualified by their page and so may repeat across pages.
    fn report_name_conflict(&mut self, id: ItemId, existing: ItemId, name: &str) {
        let topsym = self.item(id).topsym.clone();
        let mut conflict = self
            .collections_of(&topsym)
            .iter()
            .copied()
            .find(|c| *c != id && self.item(*c).name == name);
        if conflict.is_none() && self.item(id).kind != Kind::Section {
            conflict = Some(existing);
        }
        let Some(conflict) = conflict.filter(|c| *c != id) else {
            return;
        };
        let other = self.item(conflict);
        let message = format!(
            "{} \"{}\" conflicts with {} name at {}:{}",
            self.item(id).kind.as_str(),
            name,
            other.kind.as_str(),
            other.file,
            other.line.map(|l| l.to_string()).unwrap_or_default()
        );
        let (_, file, line) = self.locate(id);
        self.diagnostics
            .add(Category::Conflicts, message, Some(&file), line);
    }

    fn locate(&self, id: ItemId) -> (String, String, Option<u32>) {
        let item = self.item(id);
        (item.name.clone(), item.file.clone(), item.line)
    }

    // -- derived names ----------------------------------------------------------

    /// Applies `@rename` to the symbol, keeping the original so a bare new name can be
    /// re-qualified under the old one's scope.
    fn apply_rename(&mut self, id: ItemId) {
        let item = self.item(id);
        if item.original_symbol.is_none() {
            let symbol = item.symbol.clone();
            self.item_mut(id).original_symbol = Some(symbol);
        }
        let Some(rename) = self.item(id).flags.rename.clone() else {
            return;
        };
        if rename.contains('.') {
            self.item_mut(id).symbol = rename;
            return;
        }
        let original = self.item(id).original_symbol.clone().unwrap_or_default();
        // Everything up to and including the last `.` or `:` of the original name.
        let head = match original.rfind(['.', ':']) {
            Some(i) => original.get(..=i).unwrap_or("").to_string(),
            None => String::new(),
        };
        self.item_mut(id).symbol = head + &rename;
    }

    /// Derives `name` and `display` once, the way the Python's cached property does.
    pub fn ensure_name(&mut self, id: ItemId) {
        if !self.named.insert(id) {
            return;
        }
        self.apply_rename(id);
        match self.item(id).kind {
            Kind::Field | Kind::Function => self.derive_member_name(id),
            Kind::Section => self.derive_section_name(id),
            _ => {
                let item = self.item(id);
                let name = item.symbol.clone();
                let display = item.flags.display.clone().unwrap_or_else(|| name.clone());
                let item = self.item_mut(id);
                item.name = name;
                item.display = display;
            }
        }
    }

    /// A section is qualified by its manual page, if it is on one. `@section`s are not
    /// qualified at all, so cross-page uniqueness is the source's to arrange.
    fn derive_section_name(&mut self, id: ItemId) {
        if let Some(scope) = self.item(id).scope() {
            self.ensure_name(scope);
        }
        let scope_is_manual = self
            .item(id)
            .scope()
            .is_some_and(|s| self.item(s).kind == Kind::Manual);
        let item = self.item(id);
        let symbol = item.symbol.clone();
        let display = item.flags.display.clone().unwrap_or_else(|| symbol.clone());
        let name = if scope_is_manual {
            let scope_symbol = item
                .scope()
                .map(|s| self.item(s).symbol.clone())
                .unwrap_or_default();
            format!("{scope_symbol}.{symbol}")
        } else {
            symbol
        };
        let item = self.item_mut(id);
        item.name = name;
        item.display = display;
    }

    /// Qualifies a field or a function relative to what contains it.
    fn derive_member_name(&mut self, id: ItemId) {
        let scope = self.item(id).scope();
        if let Some(scope) = scope {
            self.ensure_name(scope);
        }
        let scope_is_class = scope.is_some_and(|s| self.item(s).kind == Kind::Class);

        // A field under a class's `static` table is a metaclass static, and `static` is
        // not part of its name.
        if scope_is_class && self.item(id).symbol.contains(".static.") {
            let stripped = self.item(id).symbol.replace(".static", "");
            self.item_mut(id).symbol = stripped;
        }

        let mut display = self.item(id).flags.display.clone();
        let mut name = self.item(id).symbol.clone();

        // An explicit `@scope`, or the one the containing collection declares, moves the
        // element under a different qualifier entirely.
        let scope_tag = self.item(id).flags.scope.clone().or_else(|| {
            self.item(id)
                .collection
                .and_then(|c| self.item(c).flags.scope.clone())
        });

        if let Some(scope_tag) = scope_tag {
            let symbol = self.item(id).symbol.clone();
            let tail = symbol
                .rsplit(['.', ':'])
                .next()
                .unwrap_or(&symbol)
                .to_string();
            let requalified = if scope_tag == "." {
                tail
            } else {
                let delim = if symbol.contains(':') { ':' } else { '.' };
                format!("{scope_tag}{delim}{tail}")
            };
            self.item_mut(id).symbol = requalified.clone();
            name = requalified.clone();
            display = display.or(Some(requalified));
        } else if !name.contains('.') && !name.contains(':') {
            // An unqualified symbol takes the name of what contains it. A symbol already
            // written `Class:method` is scoped by its colon: re-qualifying it would
            // produce `Class.Class.method`, which no cross reference can target.
            let scope_symbol = scope
                .map(|s| self.item(s).symbol.clone())
                .unwrap_or_default();
            name = format!("{scope_symbol}.{name}");
            display = display.or(Some(name.clone()));
        }

        let symbol = self.item(id).symbol.clone();
        let item = self.item_mut(id);
        item.name = name.replace(':', ".");
        item.display = display.unwrap_or(symbol);
    }

    /// Derives `topsym` once. A top-level element is its own; anything else takes the
    /// name of the nearest enclosing page.
    pub fn ensure_topsym(&mut self, id: ItemId) {
        if !self.topsymed.insert(id) {
            return;
        }
        self.ensure_name(id);
        if self.item(id).kind.is_top() {
            let name = self.item(id).name.clone();
            self.item_mut(id).topsym = name;
            return;
        }
        let found = self
            .item(id)
            .scopes
            .clone()
            .into_iter()
            .rev()
            .find(|s| self.item(*s).kind.is_top())
            .map(|s| {
                self.ensure_name(s);
                self.item(s).name.clone()
            });
        match found {
            Some(name) => self.item_mut(id).topsym = name,
            None => {
                let (name, file, line) = self.locate(id);
                self.diagnostics.add(
                    Category::Conflicts,
                    format!("could not determine which class or module {name} belongs to"),
                    Some(&file),
                    line,
                );
            }
        }
    }

    /// Gives every registered element its opaque id.
    ///
    /// Deferred to one pass because the id names the *page* an element is on, and a page
    /// is only certainly registered once every file has been read.
    pub fn assign_ids(&mut self) {
        let mut registered: Vec<ItemId> = self.by_kind.values().flatten().copied().collect();
        registered.sort_unstable();
        for id in registered {
            let topref = self.topref(id);
            let item = self.item(id);
            let hash = util::ref_id(self.item(topref).kind.as_str(), &item.topsym, &item.name);
            self.item_mut(id).id = hash.clone();
            // Only an element that owns its own name is reachable by id. One whose name
            // was already taken lost that registration and is absent from this map too,
            // so a `@see` pointing at it falls back to the raw id -- which is what the
            // Python does, for the same reason.
            if self.refs.get(&self.item(id).name) == Some(&id) {
                self.by_id.entry(hash).or_insert(id);
            }
        }
    }

    /// Registers the html renderer's search pseudo-page.
    ///
    /// It exists so the relative paths on the search and landing pages come out right,
    /// and it is deliberately *not* a top-level symbol: it must never appear in a sidebar
    /// list, and nothing can link to it. Its symbol is the Python's `--search`, which the
    /// body class reduces to `other-search`.
    pub fn add_search_ref(&mut self) -> ItemId {
        let mut item = Item::new(Kind::Pseudo, "search.html", None, "--search");
        item.flags.display = Some("Search".to_string());
        let id = self.push(item);
        self.ensure_name(id);
        self.ensure_topsym(id);
        id
    }

    /// Resolves an element's content in place, against whatever is focused.
    pub fn resolve_item_content(&mut self, id: ItemId) {
        let mut content = std::mem::take(&mut self.item_mut(id).content);
        self.resolve_content(&mut content);
        self.item_mut(id).content = content;
    }

    /// The compact display name of an element: its `@display`, or its symbol with the
    /// page's own name stripped off the front.
    pub fn display_compact(&self, id: ItemId) -> String {
        let item = self.item(id);
        if let Some(display) = &item.flags.display {
            return display.clone();
        }
        match item.symbol.strip_prefix(&item.topsym) {
            Some(rest) => rest.trim_start_matches([':', '.']).to_string(),
            None => item.symbol.clone(),
        }
    }

    /// The element an opaque id names, for a `luadox:` link target or a mixin phrase.
    pub fn item_by_id(&self, id: &RefId) -> Option<ItemId> {
        self.by_id.get(id).copied()
    }

    /// What a `@see` entry that resolved to `found` reaches by id. Two elements can share
    /// an id, and the first registered owns it, so this may be another element than
    /// `found` -- or none, when `found` lost its name to a conflict.
    pub fn see_ref(&self, found: ItemId) -> SeeRef {
        let id = self.item(found).id.clone();
        match self.by_id.get(&id) {
            Some(reached) => SeeRef::Item(*reached),
            None => SeeRef::Unreachable(id),
        }
    }

    // -- scanning ---------------------------------------------------------------

    /// Reads one Lua source file, registering everything it documents.
    pub fn parse_source(&mut self, path: &str, source: &str) {
        let file = lua::parse(path, source);
        for (line, what) in &file.errors {
            self.diagnostics.add(
                Category::Structure,
                format!("could not parse: {what}"),
                Some(path),
                Some(*line),
            );
        }

        let modname = module_name_for(path);
        let mut modref_item = Item::new(Kind::Module, path, Some(1), &modname);
        modref_item.implicit = true;
        modref_item.depth = -1;
        let modref = self.push(modref_item);

        let mut scopes: Vec<ItemId> = vec![modref];
        let mut collection = modref;
        let mut current: Option<Current> = None;
        let mut parse_next_code_line = true;
        let mut table_level: i32 = 0;

        self.ctx.file = Some(path.to_string());
        for (n, source_line) in (1u32..).zip(&file.lines) {
            // A `--[[ ]]` block is a comment, all of it. The Python has no notion of one,
            // which is how it documents a function that is commented out.
            if source_line.in_long_comment {
                continue;
            }
            let line = source_line.text.as_str();
            self.ctx.line = Some(n);

            if current.is_none() && opens_block(line) {
                current = Some(Current::Block(Box::new(Block::new(n))));
            }

            let is_comment = line.starts_with("--");
            if is_comment {
                if let Some(block) = current.take() {
                    current = Some(self.scan_comment_line(
                        block,
                        line,
                        n,
                        path,
                        &mut scopes,
                        &mut collection,
                        &mut parse_next_code_line,
                        table_level,
                    ));
                }
                continue;
            }

            let code = source_line.code.as_str();
            if !code.is_empty() {
                table_level += code.matches('{').count() as i32;
                table_level -= code.matches('}').count() as i32;
                while scopes.last().is_some_and(|s| {
                    self.item(*s).kind == Kind::Table && table_level <= self.item(*s).depth
                }) {
                    scopes.pop();
                    if let Some(top) = scopes.last() {
                        collection = *top;
                    }
                }
            }

            if !parse_next_code_line {
                if let Some(block) = current.take() {
                    self.finish(block, Some(modref), path);
                    parse_next_code_line = true;
                }
                continue;
            }

            if let Some(module) = require_target(code) {
                self.requires.push(module);
            }

            let Some(block) = current.take() else {
                self.synthesize_member(&file, n, code, &scopes, collection, modref, path);
                continue;
            };

            let block = self.attach_declaration(block, &file, n, &scopes, collection, path);
            self.finish(block, Some(modref), path);
        }

        if let Some(block) = current {
            // An explicitly declared collection that holds nothing but its own docstring
            // still deserves its page.
            self.finish(block, None, path);
        }
    }

    /// Puts a block that now has a declaration into the arena, binding its `@alias` names
    /// to the new element. This is the one way a documentation block becomes an `Item`.
    fn enter(&mut self, mut block: Block, kind: Kind, symbol: &str, path: &str, n: u32) -> ItemId {
        let aliases = std::mem::take(&mut block.aliases);
        let id = self.push(block.declare(kind, symbol, path, n));
        self.aliases
            .extend(aliases.into_iter().map(|name| (name, id)));
        id
    }

    /// Registers the element a finished block declared. A block that declared nothing is
    /// dropped, and reported when it actually said something.
    fn finish(&mut self, block: Current, modref: Option<ItemId>, path: &str) {
        match block {
            Current::Declared(id) => self.add_reference(id, modref),
            Current::Block(block) => {
                if block.said_something() {
                    self.diagnostics.add(
                        Category::Structure,
                        "comment block is not connected with any section, ignoring",
                        Some(path),
                        Some(block.line),
                    );
                }
            }
        }
    }

    /// Turns the line of code below a documentation block into the element it documents.
    fn attach_declaration(
        &mut self,
        block: Current,
        file: &SourceFile,
        n: u32,
        scopes: &[ItemId],
        collection: ItemId,
        path: &str,
    ) -> Current {
        let scope_is_module = scopes
            .last()
            .is_some_and(|s| self.item(*s).kind == Kind::Module);
        let scope_name = scopes
            .last()
            .map(|s| self.item(*s).name.clone())
            .unwrap_or_default();

        // The Python tries a field first and a function second, and skips a field whose
        // name is the module's own -- assigning the module table to itself is a common
        // pattern, not a documented member.
        let field = file
            .fields
            .get(&n)
            .filter(|decl| !(scope_is_module && scope_name == decl.symbol));
        let (kind, symbol, args, value) = match (field, file.functions.get(&n)) {
            (Some(decl), _) => (Kind::Field, &decl.symbol, Vec::new(), decl.value.clone()),
            (None, Some(decl)) => (Kind::Function, &decl.symbol, decl.args.clone(), None),
            (None, None) => return block,
        };

        let id = match block {
            Current::Block(block) => self.enter(*block, kind, symbol, path, n),
            // A collection tag already typed the block, and its declaration should have
            // ended it; the code line re-types the element in place, as the Python does.
            Current::Declared(id) => {
                let (name, _, _) = self.locate(id);
                let kind_name = self.item(id).kind.as_str();
                self.diagnostics.add(
                    Category::Structure,
                    format!(
                        "{} defined before {} {} has terminated; separate with a blank line",
                        kind.as_str(),
                        kind_name,
                        name
                    ),
                    Some(&self.item(id).file.clone()),
                    self.item(id).line,
                );
                let item = self.item_mut(id);
                item.kind = kind;
                item.file = path.to_string();
                item.line = Some(n);
                item.symbol = symbol.clone();
                id
            }
        };
        let item = self.item_mut(id);
        item.scopes = scopes.to_vec();
        item.collection = Some(collection);
        item.args = args;
        item.value = value;
        Current::Declared(id)
    }

    /// A name assigned inside an `@enum` or an explicit `@section` is a member of it even
    /// with no doc comment.
    ///
    /// Both are declared memberships: the enum mirrors a C++ enumeration and a section
    /// names the group it contains, and both are usually generated from C++ whose Doxygen
    /// comments are often absent. Dropping the undocumented ones hid 35 property and
    /// message types across 10 pages of the production API docs.
    #[allow(clippy::too_many_arguments)]
    fn synthesize_member(
        &mut self,
        file: &SourceFile,
        n: u32,
        code: &str,
        scopes: &[ItemId],
        collection: ItemId,
        modref: ItemId,
        path: &str,
    ) {
        if code.is_empty() {
            return;
        }
        let Some(member_scope) = scopes.last().copied() else {
            return;
        };
        let in_enum =
            self.item(member_scope).kind == Kind::Table && self.item(member_scope).flags.is_enum;
        let in_section = self.item(collection).kind == Kind::Section;
        if !in_enum && !in_section {
            return;
        }
        let Some(decl) = file.fields.get(&n) else {
            return;
        };
        let name = decl.symbol.clone();
        self.ensure_name(member_scope);
        // A section stays open to the end of the file, so an assignment in a later
        // function body sits inside it too. A member is written `<collection>.<name>`;
        // a local is not, which separates the two without reading the code's structure.
        let qualified = name.starts_with(&format!("{}.", self.item(member_scope).name));
        // A `__`-prefixed name is a metamethod or internal by Lua convention.
        let internal = name
            .rsplit('.')
            .next()
            .is_some_and(|tail| tail.starts_with("__"));
        if internal || !(in_enum || qualified) {
            return;
        }

        let mut item = Item::new(Kind::Field, path, Some(n), &name);
        item.scopes = scopes.to_vec();
        item.collection = Some(collection);
        item.value = decl.value.clone();
        let id = self.push(item);
        self.add_reference(id, Some(modref));
        if in_section {
            self.diagnostics.add(
                Category::UndocumentedSectionMembers,
                format!("{name} has no doc comment"),
                Some(path),
                Some(n),
            );
        }
    }

    /// The parts of the block being scanned that a tag writes to, wherever the block
    /// lives.
    fn scanned<'a>(&'a mut self, block: &'a mut Current) -> Scanned<'a> {
        match block {
            Current::Block(block) => Scanned {
                flags: &mut block.flags,
                within: &mut block.within,
                raw_content: &mut block.raw_content,
            },
            Current::Declared(id) => {
                let item = self.item_mut(*id);
                Scanned {
                    flags: &mut item.flags,
                    within: &mut item.within,
                    raw_content: &mut item.raw_content,
                }
            }
        }
    }

    /// Records an `@alias` for the element the block declares, or will declare.
    fn alias(&mut self, block: &mut Current, name: String) {
        match block {
            Current::Block(block) => block.aliases.push(name),
            Current::Declared(id) => self.aliases.push((name, *id)),
        }
    }

    /// Handles one `---` comment line: every tag on it that changes what the block is,
    /// and otherwise the line itself as content. Returns the block, which a collection
    /// tag turns into a declared element.
    #[allow(clippy::too_many_arguments)]
    fn scan_comment_line(
        &mut self,
        mut block: Current,
        line: &str,
        n: u32,
        path: &str,
        scopes: &mut Vec<ItemId>,
        collection: &mut ItemId,
        parse_next_code_line: &mut bool,
        table_level: i32,
    ) -> Current {
        let parsed = match tags::parse_comment(line) {
            Ok(tags) => tags,
            Err(err) => {
                self.diagnostics
                    .add(Category::Structure, err.to_string(), Some(path), Some(n));
                Vec::new()
            }
        };

        let mut handled = 0usize;
        let mut unprocessed: Vec<Tag> = Vec::new();
        for tag in parsed {
            handled += 1;
            if let Some(name) = tag.collection_name() {
                let kind = match tag {
                    Tag::Class(_) => Kind::Class,
                    Tag::Module(_) => Kind::Module,
                    Tag::Section(_) => Kind::Section,
                    _ => Kind::Table,
                };
                // A class replaces any class already in scope: nested classes are not a
                // thing, and the new one must not be scoped inside the old.
                if kind == Kind::Class
                    && scopes
                        .last()
                        .is_some_and(|s| self.item(*s).kind == Kind::Class)
                {
                    scopes.pop();
                }
                let id = match block {
                    Current::Block(block) => self.enter(*block, kind, name, path, n),
                    Current::Declared(id) => {
                        let item = self.item_mut(id);
                        item.kind = kind;
                        item.line = Some(n);
                        item.symbol = name.to_string();
                        id
                    }
                };
                block = Current::Declared(id);
                let item = self.item_mut(id);
                item.depth = table_level;
                item.scopes = scopes.clone();
                item.collection = Some(*collection);
                *collection = id;

                match kind {
                    Kind::Class => {
                        let root = scopes.first().copied().unwrap_or(id);
                        *scopes = vec![root, id];
                        *parse_next_code_line = false;
                    }
                    Kind::Module => {
                        let root = scopes.first().copied().unwrap_or(id);
                        *scopes = vec![root, id];
                    }
                    Kind::Table => {
                        if matches!(tag, Tag::Enum(_)) {
                            self.item_mut(id).flags.is_enum = true;
                        }
                        scopes.push(id);
                        *parse_next_code_line = false;
                    }
                    _ => {}
                }
                continue;
            }

            match tag {
                Tag::Within(name) => *self.scanned(&mut block).within = Some(name),
                Tag::Field { name, desc } => {
                    let mut item = Item::new(Kind::Field, path, Some(n), &name);
                    item.scopes = scopes.clone();
                    item.collection = Some(*collection);
                    item.raw_content.push(RawLine::Source {
                        line: n,
                        text: desc,
                        tags: Vec::new(),
                    });
                    let field = self.push(item);
                    let modref = scopes.first().copied();
                    self.add_reference(field, modref);
                }
                // The Python registers the alias against the still-untyped reference,
                // which a later clone then replaces; here the alias is recorded and bound
                // to the element the block turns out to declare.
                Tag::Alias(name) => self.alias(&mut block, name),
                Tag::Compact(members) => self.scanned(&mut block).flags.compact = members,
                Tag::Fullnames => self.scanned(&mut block).flags.fullnames = true,
                Tag::Deprecated(desc) => self
                    .scanned(&mut block)
                    .flags
                    .deprecated
                    .get_or_insert_default()
                    .explain(desc),
                Tag::Meta(value) => self.scanned(&mut block).flags.meta = Some(value),
                Tag::Since(version) => {
                    if version.is_empty() {
                        self.diagnostics.add(
                            Category::Structure,
                            "@since requires a version, ignoring",
                            Some(path),
                            Some(n),
                        );
                    } else {
                        self.scanned(&mut block).flags.since = Some(version);
                    }
                }
                Tag::Inherits(names) => {
                    let parents: Vec<String> = names
                        .iter()
                        .flat_map(|n| n.split(','))
                        .filter(|p| !p.is_empty())
                        .map(str::to_string)
                        .collect();
                    self.scanned(&mut block).flags.inherits.extend(parents);
                }
                // The Python also renames the current scope when it is the same element
                // as the block; a block is only ever its own scope once a collection tag
                // has declared it, so that is the same flag written twice.
                Tag::Rename(name) => self.scanned(&mut block).flags.rename = Some(name),
                Tag::Scope(name) => self.scanned(&mut block).flags.scope = Some(name),
                Tag::Display(name) => self.scanned(&mut block).flags.display = Some(name),
                Tag::Type(types) => self.scanned(&mut block).flags.types = Some(types),
                Tag::Order(order) => self.scanned(&mut block).flags.order = Some(order),
                Tag::Unrecognized(name) => self.diagnostics.add(
                    Category::Structure,
                    format!("unrecognized tag @{name}, ignoring"),
                    Some(path),
                    Some(n),
                ),
                other => {
                    // A content tag, handled when the block is assembled rather than now.
                    unprocessed.push(other);
                    handled -= 1;
                }
            }
        }

        if handled == 0 {
            self.scanned(&mut block).raw_content.push(RawLine::Source {
                line: n,
                text: line.to_string(),
                tags: unprocessed,
            });
        }
        block
    }

    // -- manual pages -----------------------------------------------------------

    /// Reads a markdown file as a manual page, turning its headings into elements that a
    /// cross reference can target.
    pub fn parse_manual(&mut self, name: &str, path: &str, content: &str) {
        let mut top = Item::new(Kind::Manual, path, Some(1), name);
        top.depth = -1;
        let top = self.push(top);
        self.add_reference(top, None);

        // Markdown headings need not be unique, so a repeated one gets a numeric suffix.
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut current = top;
        let mut fences = 0usize;
        for (index, line) in content.lines().enumerate() {
            let n = index as u32 + 1;
            fences += line.matches("```").count();
            let heading = heading_of(line).filter(|_| fences % 2 == 0);
            if let Some((level, text)) = heading {
                if level <= 3 {
                    if current == top {
                        self.item_mut(top).heading = text.to_string();
                    }
                    let mut symbol = slugify(text);
                    let count = seen.entry(symbol.clone()).or_insert(0);
                    if *count > 0 {
                        symbol = format!("{symbol}{}", *count + 1);
                    }
                    *count += 1;

                    let mut section = Item::new(Kind::Section, path, Some(n), &symbol);
                    section.scopes = vec![top];
                    section.heading = text.to_string();
                    section.heading_level = u8::try_from(level).ok();
                    let id = self.push(section);
                    self.add_reference(id, None);
                    current = id;
                    continue;
                }
            }
            self.item_mut(current).raw_content.push(RawLine::Manual {
                line: n,
                text: line.to_string(),
            });
        }
    }

    // -- enum validation --------------------------------------------------------

    /// Reports `@enum` tables that cannot form a closed enumeration.
    ///
    /// Membership mirrors a C++ enum: a member exists because it is assigned a value, not
    /// because it is documented. An undocumented member is not malformed, just
    /// undocumented, so it gets its own granular category and a project whose enums are
    /// generated can accept it without loosening any other check.
    pub fn validate_enums(&mut self) {
        let tables = self.of_kind(Kind::Table).to_vec();
        for colref in tables {
            if !self.item(colref).flags.is_enum {
                continue;
            }
            let members = self.elements_in_collection(Kind::Field, colref);
            if members.is_empty() {
                let (name, file, line) = self.locate(colref);
                self.diagnostics.add(
                    Category::Structure,
                    format!(
                        "@enum {name} has no members; the tag must be on a table of \
                         integer constants"
                    ),
                    Some(&file),
                    line,
                );
                continue;
            }
            for member in members {
                let (name, file, line) = self.locate(member);
                if self.item(member).raw_content.is_empty() {
                    self.diagnostics.add(
                        Category::UndocumentedEnumMembers,
                        format!("@enum member {name} has no doc comment"),
                        Some(&file),
                        line,
                    );
                }
                if !is_integer_literal(self.item(member).value.as_deref()) {
                    self.diagnostics.add(
                        Category::Structure,
                        format!("@enum member {name} is not assigned an integer value"),
                        Some(&file),
                        line,
                    );
                }
            }
        }
    }

    // -- ordering and lookup ----------------------------------------------------

    /// Applies `@order` to a list of elements.
    fn reorder(&mut self, refs: Vec<ItemId>, topref: Option<ItemId>) -> Vec<ItemId> {
        let mut ordered = refs.clone();
        if let Some(topref) = topref {
            ordered.retain(|r| self.topref(*r) == topref);
        }
        for r in refs {
            if topref.is_some_and(|t| self.topref(r) != t) {
                continue;
            }
            let Some(order) = self.item(r).flags.order.clone() else {
                continue;
            };
            let (anchor, offset) = match order {
                Order::First => {
                    ordered.retain(|x| *x != r);
                    ordered.insert(0, r);
                    continue;
                }
                Order::Last => {
                    ordered.retain(|x| *x != r);
                    ordered.push(r);
                    continue;
                }
                Order::Before(anchor) => (anchor, 0),
                Order::After(anchor) => (anchor, 1),
            };
            match ordered.iter().position(|o| self.item(*o).symbol == anchor) {
                Some(at) => {
                    let at = at + offset;
                    ordered.retain(|x| *x != r);
                    let at = at.min(ordered.len());
                    ordered.insert(at, r);
                }
                None => {
                    let (_, file, line) = self.locate(r);
                    self.diagnostics.add(
                        Category::References,
                        format!("unknown @order anchor reference {anchor}"),
                        Some(&file),
                        line,
                    );
                }
            }
        }
        ordered
    }

    /// The collections of a page, in render order.
    pub fn collections_for(&mut self, topref: ItemId) -> Vec<ItemId> {
        let name = self.item(topref).name.clone();
        let sections = self.collections_of(&name).to_vec();
        if sections.is_empty() {
            return Vec::new();
        }
        self.reorder(sections, Some(topref))
    }

    /// The fields or functions of a collection, honouring `@within` and `@order`.
    pub fn elements_in_collection(&mut self, kind: Kind, colref: ItemId) -> Vec<ItemId> {
        let colname = self.item(colref).name.clone();
        // `@section` names need not be globally unique, so first find which pages have a
        // collection of this name.
        let mut found: Vec<String> = Vec::new();
        for (_, refs) in &self.collections {
            for r in refs {
                if self.item(*r).name == colname {
                    let topsym = self.item(*r).topsym.clone();
                    if !found.contains(&topsym) {
                        found.push(topsym);
                    }
                }
            }
        }

        let mut topsym = Some(self.item(colref).topsym.clone());
        if found.len() <= 1 {
            topsym = None;
        } else if !found.contains(&self.item(colref).topsym) {
            let owner = self.item(colref).topsym.clone();
            let (_, file, line) = self.locate(colref);
            found.sort();
            self.diagnostics.add(
                Category::References,
                format!(
                    "collection \"{colname}\" referenced by {owner} is ambiguous as it \
                     exists in multiple classes or modules ({}) but {owner} lacks \
                     documented {}s",
                    found.join(", "),
                    kind.as_str()
                ),
                Some(&file),
                line,
            );
        }

        let mut elems = Vec::new();
        for r in self.of_kind(kind) {
            let item = self.item(*r);
            if let Some(topsym) = &topsym {
                if *topsym != item.topsym {
                    continue;
                }
            }
            match &item.within {
                Some(within) => {
                    if *within == colname {
                        elems.push(*r);
                    }
                }
                None => {
                    if item
                        .collection
                        .is_some_and(|c| self.item(c).name == colname)
                    {
                        elems.push(*r);
                    }
                }
            }
        }
        self.reorder(elems, None)
    }

    // -- cross references -------------------------------------------------------

    /// Finds the element a `@{name}` or `` `name` `` refers to.
    ///
    /// The name is relative to the current context: its own scopes first, then the global
    /// space, then -- if the page is a class -- up the inheritance chain.
    pub fn resolve_ref(&mut self, name: &str) -> Option<ItemId> {
        let name: String = name
            .chars()
            .filter(|c| *c != '(' && *c != ')')
            .map(|c| if c == ':' { '.' } else { c })
            .collect();

        let mut found = None;
        if let Some(ctx) = self.ctx.item {
            let mut candidates = vec![self.item(ctx).name.clone()];
            candidates.extend(
                self.item(ctx)
                    .scopes
                    .iter()
                    .map(|s| self.item(*s).name.clone()),
            );
            for scope in candidates {
                found = self.refs.get(&format!("{scope}.{name}")).copied();
                if found.is_some() {
                    break;
                }
            }
        }
        if found.is_none() {
            found = self.refs.get(&name).copied();
        }
        if found.is_none() {
            if let Some(ctx) = self.ctx.item {
                let topref = self.topref(ctx);
                if self.item(topref).kind == Kind::Class {
                    for clsref in self.hierarchy(topref).into_iter().rev() {
                        let qualified = format!("{}.{name}", self.item(clsref).name);
                        found = self.refs.get(&qualified).copied();
                        if found.is_some() {
                            break;
                        }
                    }
                }
            }
        }

        if let Some(id) = found {
            self.trace_within(id, &name);
        }
        found
    }

    /// Records which page an element's `@within` target lives on, reporting a target that
    /// several pages could satisfy.
    fn trace_within(&mut self, id: ItemId, name: &str) {
        let Some(within) = self.item(id).within.clone() else {
            return;
        };
        if self.within_topsym.contains_key(&id) {
            return;
        }
        let topsym = self.item(id).topsym.clone();
        let here = self
            .collections_of(&topsym)
            .iter()
            .any(|c| self.item(*c).symbol == within);
        if here {
            self.within_topsym.insert(id, topsym);
            return;
        }
        let mut candidates: Vec<String> = Vec::new();
        for (owner, refs) in &self.collections {
            if refs.iter().any(|c| self.item(*c).symbol == within) && !candidates.contains(owner) {
                candidates.push(owner.clone());
            }
        }
        if candidates.len() > 1 {
            candidates.sort();
            let (_, file, line) = self.locate(id);
            self.diagnostics.add(
                Category::References,
                format!(
                    "{name} is @within {within} which is ambiguous (in {})",
                    candidates.join(", ")
                ),
                Some(&file),
                line,
            );
        } else if let Some(owner) = candidates.pop() {
            self.within_topsym.insert(id, owner);
        }
    }

    /// A class and every ancestor along its first parent, oldest first.
    pub fn hierarchy(&self, id: ItemId) -> Vec<ItemId> {
        let mut chain = vec![id];
        let mut seen: HashSet<ItemId> = HashSet::from([id]);
        while let Some(first) = chain.first().copied() {
            let Some(parent) = self.item(first).flags.inherits.first() else {
                break;
            };
            let Some(sup) = self.refs.get(parent).copied() else {
                break;
            };
            if !seen.insert(sup) {
                break;
            }
            chain.insert(0, sup);
        }
        chain
    }

    /// Every class named in `@inherits`, in order, de-duplicated on the element rather
    /// than the name -- `@alias` means two names can reach the same class.
    pub fn parents(&self, id: ItemId) -> Vec<ItemId> {
        let mut seen: HashSet<ItemId> = HashSet::new();
        let mut out = Vec::new();
        for name in &self.item(id).flags.inherits {
            let Some(parent) = self.refs.get(name).copied() else {
                continue;
            };
            if parent != id && seen.insert(parent) {
                out.push(parent);
            }
        }
        out
    }

    /// The markdown link for an element: a `luadox:<id>` target the renderer resolves.
    pub fn ref_markdown(&self, id: ItemId, text: Option<&str>, style: RefStyle) -> String {
        let tick = match style {
            RefStyle::Code => "`",
            RefStyle::Plain => "",
        };
        let parens = if self.item(id).kind == Kind::Function && text.is_none() {
            "()"
        } else {
            ""
        };
        let label = text.unwrap_or(&self.item(id).name);
        format!("[{tick}{label}{parens}{tick}](luadox:{})", self.item(id).id)
    }

    /// Binds every `@alias` name recorded during the scan.
    pub fn bind_aliases(&mut self) {
        for (name, id) in std::mem::take(&mut self.aliases) {
            self.refs.insert(name, id);
        }
    }
}

static EMPTY: Item = Item {
    kind: Kind::Field,
    file: String::new(),
    line: None,
    symbol: String::new(),
    original_symbol: None,
    implicit: false,
    depth: 0,
    heading_level: None,
    scopes: Vec::new(),
    within: None,
    collection: None,
    raw_content: Vec::new(),
    flags: Flags {
        display: None,
        rename: None,
        scope: None,
        since: None,
        deprecated: None,
        inherits: Vec::new(),
        compact: Vec::new(),
        fullnames: false,
        meta: None,
        types: None,
        order: None,
        is_enum: false,
    },
    args: Vec::new(),
    value: None,
    name: String::new(),
    display: String::new(),
    topsym: String::new(),
    id: RefId::UNASSIGNED,
    content: crate::ir::Content(Vec::new()),
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
};

/// A documentation block opens with three dashes, and may use two or three thereafter.
fn opens_block(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("---") else {
        return false;
    };
    match rest.chars().next() {
        None => true,
        Some('-') => rest.chars().all(|c| c == '-'),
        Some(_) => true,
    }
}

/// The module a file declares by being where it is: its stem, or its directory when it is
/// an `init.lua`.
fn module_name_for(path: &str) -> String {
    let path = path.replace('\\', "/");
    let (dir, file) = match path.rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", path.as_str()),
    };
    if file == "init.lua" {
        return dir.rsplit('/').next().unwrap_or("").to_string();
    }
    file.strip_suffix(".lua").unwrap_or(file).to_string()
}

/// `require "x"` or `require("x")`.
fn require_target(code: &str) -> Option<String> {
    let at = code.find("require")?;
    let rest = code.get(at + "require".len()..)?.trim_start();
    let rest = rest.strip_prefix('(').unwrap_or(rest).trim_start();
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let body = rest.get(1..)?;
    let end = body.find(quote)?;
    Some(body.get(..end)?.to_string())
}

/// `# Heading` up to any depth; only the first three levels become elements.
fn heading_of(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 {
        return None;
    }
    Some((hashes, line.get(hashes..)?.trim()))
}

/// A heading's URL fragment: lower case, punctuation dropped, spaces to underscores.
fn slugify(heading: &str) -> String {
    let kept: String = heading
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == ' ')
        .collect();
    let mut out = String::new();
    let mut in_space = false;
    for c in kept.chars() {
        if c == ' ' {
            in_space = true;
            continue;
        }
        if in_space && !out.is_empty() {
            out.push('_');
        }
        in_space = false;
        out.push(c);
    }
    if in_space && !out.is_empty() {
        out.push('_');
    }
    out.replace("_-_", "-")
}

/// A decimal or hexadecimal integer, optionally signed: what a C++ enumerator is.
fn is_integer_literal(value: Option<&str>) -> bool {
    let Some(value) = value else { return false };
    let body = value.strip_prefix(['+', '-']).unwrap_or(value);
    if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        return !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    !body.is_empty() && body.chars().all(|c| c.is_ascii_digit())
}
