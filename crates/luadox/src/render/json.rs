//! The json renderer: the document made inspectable.
//!
//! It is not a feature so much as the IR written down, which is why it comes first. If
//! this output matches the oracle's, the whole front half of the tool matches, and any
//! later difference in html or LuaLS definitions is a rendering bug rather than a
//! resolution one.
//!
//! Cross references are resolved *here*, in exactly this traversal order, because that is
//! where the Python resolves them: its `Markdown.get()` runs the first time a renderer
//! asks, with whatever element the renderer had most recently focused. A field's `@{Foo}`
//! therefore resolves relative to the collection it is rendered under, not to the field.

use crate::ir::{Content, Fragment, ItemId, Kind};
use crate::json::Json;
use crate::parse::Parser;

pub fn render(parser: &mut Parser, toprefs: &[ItemId]) -> Json {
    let mut project = Json::obj();
    project.set("apiVersion", "v1alpha1".into());
    project.set("kind", "luadox".into());
    if let Some(name) = parser.config.get("project", "name").map(str::to_string) {
        project.set("name", name.into());
    }
    if let Some(title) = parser.config.get("project", "title").map(str::to_string) {
        project.set("title", title.into());
    }

    let mut classes = Vec::new();
    let mut modules = Vec::new();
    let mut manuals = Vec::new();
    for topref in toprefs {
        match parser.item(*topref).kind {
            Kind::Class => classes.push(classmod(parser, *topref)),
            Kind::Module => modules.push(classmod(parser, *topref)),
            Kind::Manual => manuals.push(manual(parser, *topref)),
            _ => {}
        }
    }
    project.set("classes", Json::Arr(classes));
    project.set("modules", Json::Arr(modules));
    project.set("manuals", Json::Arr(manuals));
    project
}

fn top(parser: &Parser, id: ItemId, extra: Vec<(&str, Json)>) -> Json {
    let item = parser.item(id);
    let mut out = Json::obj();
    out.set("id", item.id.clone().into());
    out.set("type", item.kind.as_str().into());
    out.set("name", item.name.clone().into());
    for (key, value) in extra {
        out.set_if(key, value);
    }
    out
}

fn classmod(parser: &mut Parser, topref: ItemId) -> Json {
    let mut extra: Vec<(&str, Json)> = Vec::new();
    if parser.item(topref).kind == Kind::Class {
        let hierarchy = parser.hierarchy(topref);
        if hierarchy.len() > 1 {
            extra.push(("hierarchy", named_refs(parser, &hierarchy)));
        }
        let parents = parser.parents(topref);
        if parents.len() > 1 {
            extra.push(("parents", named_refs(parser, &parents)));
        }
    }
    let mut out = top(parser, topref, extra);

    let mut sections = Vec::new();
    for colref in parser.item(topref).collections.clone() {
        parser.focus(colref);
        let compact = Json::Arr(
            parser
                .item(colref)
                .flags
                .compact
                .iter()
                .map(|m| Json::Str(m.as_str().to_string()))
                .collect(),
        );
        let is_enum = Json::Bool(parser.item(colref).flags.is_enum);
        let mut section = self_section(
            parser,
            colref,
            vec![("compact", compact), ("enum", is_enum)],
        );

        let fields: Vec<Json> = parser
            .item(colref)
            .fields
            .clone()
            .into_iter()
            .map(|id| field(parser, id))
            .collect();
        if !fields.is_empty() {
            section.set("fields", Json::Arr(fields));
        }

        let functions: Vec<Json> = parser
            .item(colref)
            .functions
            .clone()
            .into_iter()
            .map(|id| function(parser, id))
            .collect();
        if !functions.is_empty() {
            section.set("functions", Json::Arr(functions));
        }
        sections.push(section);
    }
    out.set("sections", Json::Arr(sections));
    out
}

fn manual(parser: &mut Parser, topref: ItemId) -> Json {
    let mut out = top(parser, topref, Vec::new());
    let mut sections = Vec::new();
    for colref in parser.item(topref).collections.clone() {
        parser.focus(colref);
        let level = Json::Int(parser.item(colref).level as i64);
        sections.push(self_section(parser, colref, vec![("level", level)]));
    }
    out.set("sections", Json::Arr(sections));
    out
}

fn self_section(parser: &mut Parser, colref: ItemId, extra: Vec<(&str, Json)>) -> Json {
    let mut section = Json::obj();
    {
        let item = parser.item(colref);
        section.set("id", item.id.clone().into());
        section.set("type", item.kind.as_str().into());
        section.set("symbol", item.symbol.clone().into());
        section.set("heading", item.heading.clone().into());
    }
    if let Some(explanation) = parser.item(colref).flags.deprecated.clone() {
        // Presence signals deprecation; the value is the explanation, or true when bare.
        section.set(
            "deprecated",
            if explanation.is_empty() {
                Json::Bool(true)
            } else {
                Json::Str(explanation)
            },
        );
    }
    if let Some(since) = parser.item(colref).flags.since.clone() {
        section.set_if("since", Json::Str(since));
    }
    for (key, value) in extra {
        section.set_if(key, value);
    }
    let content = render_content(parser, colref);
    if !content.is_falsy() {
        section.set("content", content);
    }
    section
}

fn field(parser: &mut Parser, id: ItemId) -> Json {
    let mut out = Json::obj();
    {
        let item = parser.item(id);
        out.set("id", item.id.clone().into());
        out.set("name", item.name.clone().into());
        out.set("display", item.display.clone().into());
    }
    let types = parser.item(id).types.clone();
    if !types.is_empty() {
        out.set("types", render_types(parser, &types));
    }
    if let Some(meta) = parser.item(id).meta.clone() {
        out.set_if("meta", Json::Str(meta));
    }
    if let Some(value) = parser.item(id).value.clone() {
        out.set("value", Json::Str(value));
    }
    if let Some(explanation) = parser.item(id).flags.deprecated.clone() {
        out.set(
            "deprecated",
            if explanation.is_empty() {
                Json::Bool(true)
            } else {
                Json::Str(explanation)
            },
        );
    }
    if let Some(since) = parser.item(id).flags.since.clone() {
        out.set_if("since", Json::Str(since));
    }
    let content = render_content(parser, id);
    if !content.is_falsy() {
        out.set("content", content);
    }
    out
}

fn function(parser: &mut Parser, id: ItemId) -> Json {
    let mut out = field(parser, id);

    let params = parser.item(id).params.clone();
    if !params.is_empty() {
        let mut rendered = Vec::new();
        for param in params {
            let mut entry = Json::obj();
            entry.set("name", param.name.clone().into());
            if !param.types.is_empty() {
                entry.set("types", render_types(parser, &param.types));
            }
            let mut content = param.content;
            parser.resolve_content(&mut content);
            let content = content_json(&content);
            if !content.is_falsy() {
                entry.set("content", content);
            }
            rendered.push(entry);
        }
        out.set("params", Json::Arr(rendered));
    }

    let returns = parser.item(id).returns.clone();
    if !returns.is_empty() {
        let mut rendered = Vec::new();
        for ret in returns {
            let mut entry = Json::obj();
            if !ret.types.is_empty() {
                entry.set("types", render_types(parser, &ret.types));
            }
            let mut content = ret.content;
            parser.resolve_content(&mut content);
            let content = content_json(&content);
            if !content.is_falsy() {
                entry.set("content", content);
            }
            rendered.push(entry);
        }
        out.set("returns", Json::Arr(rendered));
    }
    out
}

fn named_refs(parser: &Parser, ids: &[ItemId]) -> Json {
    Json::Arr(
        ids.iter()
            .map(|id| {
                let item = parser.item(*id);
                let mut out = Json::obj();
                out.set("name", item.name.clone().into());
                out.set("refid", item.id.clone().into());
                out
            })
            .collect(),
    )
}

/// A type name, with the element it names when there is one.
fn render_types(parser: &mut Parser, types: &[String]) -> Json {
    Json::Arr(
        types
            .iter()
            .map(|name| {
                let mut out = Json::obj();
                out.set("name", name.clone().into());
                if let Some(found) = parser.resolve_ref(name) {
                    out.set("refid", parser.item(found).id.clone().into());
                }
                out
            })
            .collect(),
    )
}

/// Resolves an element's content against the element currently focused, then writes it.
fn render_content(parser: &mut Parser, id: ItemId) -> Json {
    let mut content = std::mem::take(&mut parser.item_mut(id).content);
    parser.resolve_content(&mut content);
    let out = content_json(&content);
    parser.item_mut(id).content = content;
    out
}

fn content_json(content: &Content) -> Json {
    let mut out = Vec::new();
    for fragment in &content.0 {
        match fragment {
            Fragment::Markdown(md) => {
                let value = md.get().trim().to_string();
                if value.is_empty() {
                    continue;
                }
                let mut entry = Json::obj();
                entry.set("type", "markdown".into());
                entry.set("value", value.into());
                out.push(entry);
            }
            Fragment::Admonition {
                level,
                title,
                content,
            } => {
                let mut entry = Json::obj();
                entry.set("type", "admonition".into());
                entry.set("level", level.clone().into());
                entry.set("title", title.clone().into());
                entry.set("content", content_json(content));
                out.push(entry);
            }
            Fragment::SeeAlso(refs) => {
                let mut entry = Json::obj();
                entry.set("type", "see".into());
                entry.set(
                    "refs",
                    Json::Arr(
                        refs.iter()
                            .map(|refid| {
                                let mut one = Json::obj();
                                one.set("refid", refid.clone().into());
                                one
                            })
                            .collect(),
                    ),
                );
                out.push(entry);
            }
        }
    }
    Json::Arr(out)
}
