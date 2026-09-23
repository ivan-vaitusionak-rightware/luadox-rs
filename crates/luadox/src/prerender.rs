//! Filling in everything a renderer needs that parsing could not decide.
//!
//! Parsing knows what was written; this stage knows what it means once every file has
//! been read -- which collection an element belongs to, what its heading says, which
//! parameters a function documents and which it forgot.

use crate::content::{self, Parsed};
use crate::diag::Category;
use crate::ir::{AdmonitionLevel, Content, Fragment, ItemId, Kind, Member};
use crate::markdown::RowContent;
use crate::parse::Parser;

/// Prepares every page for rendering and returns them in render order: classes, then
/// manual pages, then modules, each group by symbol.
pub fn process(parser: &mut Parser) -> Vec<ItemId> {
    let mut toprefs = Vec::new();
    for topref in parser.topsyms.clone() {
        match parser.item(topref).kind {
            Kind::Class | Kind::Module => classmod(parser, topref),
            Kind::Manual => manual(parser, topref),
            _ => {}
        }
        if parser.item(topref).kind == Kind::Class {
            // A parent that resolves to no documented class leaves the inheritance
            // incomplete; report it rather than dropping it silently.
            for name in parser.item(topref).flags.inherits.clone() {
                if !parser.refs.contains_key(&name) {
                    let item = parser.item(topref);
                    let (file, line) = (item.file.clone(), item.line);
                    parser.diagnostics.add(
                        Category::References,
                        format!("@inherits parent \"{name}\" could not be resolved"),
                        Some(&file),
                        line,
                    );
                }
            }
        }
        toprefs.push(topref);
    }
    toprefs.sort_by_key(|id| {
        let item = parser.item(*id);
        (item.kind.as_str(), item.symbol.clone())
    });
    toprefs
}

fn classmod(parser: &mut Parser, topref: ItemId) {
    let mut has_content = false;
    for colref in parser.collections_for(topref) {
        parser.focus(colref);
        let parsed = parse_block(parser, colref);
        let mut content = parsed.content;

        // A class or a module is its own first collection and is titled by its symbol.
        // Any other collection takes its heading from its first sentence, which is then
        // no longer part of its body.
        let kind = parser.item(colref).kind;
        let heading = if matches!(kind, Kind::Class | Kind::Module) {
            parser.item(colref).symbol.clone()
        } else {
            let first = parser.take_first_sentence(&mut content);
            let first = first.trim().to_string();
            if first.is_empty() {
                parser.item(colref).name.clone()
            } else {
                first
            }
        };
        let fullnames = parser.item(colref).flags.fullnames;
        {
            let item = parser.item_mut(colref);
            item.heading = heading;
            item.content = content;
        }
        apply_deprecated(parser, colref);
        parser.item_mut(topref).collections.push(colref);

        let functions = parser.elements_in_collection(Kind::Function, colref);
        let fields = parser.elements_in_collection(Kind::Field, colref);
        has_content = has_content
            || !parser.item(colref).content.is_empty()
            || !functions.is_empty()
            || !fields.is_empty();

        for id in fields {
            parser.focus(id);
            let parsed = parse_block(parser, id);
            let item = parser.item(id);
            let title = item.flags.display.clone().unwrap_or_else(|| {
                if fullnames {
                    item.name.clone()
                } else {
                    item.symbol.clone()
                }
            });
            let types = item.flags.types.clone().unwrap_or_default();
            let meta = item.flags.meta.clone();
            {
                let item = parser.item_mut(id);
                item.title = title;
                item.types = types;
                item.meta = meta;
                item.content = parsed.content;
            }
            apply_deprecated(parser, id);
            fit_to_row(
                parser,
                id,
                &parser.item(colref).flags.compact.clone(),
                Member::Fields,
            );
            parser.item_mut(colref).fields.push(id);
        }

        for id in functions {
            parser.focus(id);
            let parsed = parse_block(parser, id);
            let args = parser.item(id).args.clone();
            let params = content::params_for(parser, id, &parsed, &args);
            let title = parser.item(id).display.clone();
            // A function's `@meta` is resolved here, against the function; a field's is
            // carried through as written, which is what the Python does.
            let meta = parser
                .item(id)
                .flags
                .meta
                .clone()
                .map(|meta| parser.resolve_text(&meta));
            {
                let item = parser.item_mut(id);
                item.title = title;
                item.params = params;
                item.returns = parsed.returns;
                item.meta = meta;
                item.content = parsed.content;
            }
            apply_deprecated(parser, id);
            fit_to_row(
                parser,
                id,
                &parser.item(colref).flags.compact.clone(),
                Member::Functions,
            );
            parser.item_mut(colref).functions.push(id);
        }
    }
    parser.item_mut(topref).empty = !has_content;
}

fn manual(parser: &mut Parser, topref: ItemId) {
    if !parser.item(topref).raw_content.is_empty() {
        parser.focus(topref);
        let parsed = parse_block(parser, topref);
        let heading = parser.item(topref).heading.clone();
        let heading = parser.resolve_text(&heading);
        let item = parser.item_mut(topref);
        item.content = parsed.content;
        item.heading = heading;
    }
    for colref in parser.collections_for(topref) {
        parser.focus(colref);
        let parsed = parse_block(parser, colref);
        let heading = parser.item(colref).heading.clone();
        let heading = parser.resolve_text(&heading);
        {
            let item = parser.item_mut(colref);
            item.heading = heading;
            item.content = parsed.content;
        }
        parser.item_mut(topref).collections.push(colref);
    }
}

/// Reduces an element's documentation to what a one-line row can present, when the
/// collection it is in is `@compact` for its kind.
///
/// A compact row carries the element's whole documentation, because there is no detail
/// box to put the rest in. So a code block, a heading or a list lands inside a one-line
/// table cell, where the stylesheet has no rule for it -- 240 elements over 50 files on
/// the pinned corpus, all of them rendered and none of them readable.
///
/// This is the one place `RowContent` can be built, and therefore the one place the
/// invariant can be broken. Its `Err` is the report; the fix belongs to whoever wrote the
/// tag -- drop `@compact` for that collection, or keep its members' documentation to
/// prose.
fn fit_to_row(parser: &mut Parser, id: ItemId, compact: &[Member], member: Member) {
    if !compact.contains(&member) {
        return;
    }
    match RowContent::try_from(&parser.item(id).content) {
        Ok(row) => parser.item_mut(id).row = Some(row),
        Err(too_big) => {
            let item = parser.item(id);
            let (name, file, line) = (item.name.clone(), item.file.clone(), item.line);
            parser.diagnostics.add(
                Category::CompactBlockContent,
                format!(
                    "{name} is in a compact collection, so its {too_big} renders inside a \
                     one-line table row"
                ),
                Some(&file),
                line,
            );
        }
    }
}

fn parse_block(parser: &mut Parser, id: ItemId) -> Parsed {
    let raw = std::mem::take(&mut parser.item_mut(id).raw_content);
    let parsed = parser.parse_raw_content(&raw);
    parser.item_mut(id).raw_content = raw;
    parsed
}

/// Renders a `@deprecated` flag as a leading admonition, so every renderer shows it
/// without knowing about the flag. The flag stays for renderers with a native
/// representation of deprecation.
fn apply_deprecated(parser: &mut Parser, id: ItemId) {
    let Some(explanation) = parser.item(id).flags.deprecated.clone() else {
        return;
    };
    let mut body = Content::default();
    if !explanation.is_empty() {
        body.md(true).append(explanation);
    }
    parser.item_mut(id).content.insert(
        0,
        Fragment::Admonition {
            level: AdmonitionLevel::Deprecated,
            title: "Deprecated".to_string(),
            content: body,
        },
    );
}
