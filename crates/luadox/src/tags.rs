//! luadox's annotations.
//!
//! The Python builds these out of a `TAGMAP` of `typing` annotations coerced at runtime;
//! here the argument shape is the enum variant, so a tag that parsed is a tag whose
//! arguments exist.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    // Collection tags: each opens a new collection and becomes the current one.
    Module(String),
    Class(String),
    Section(String),
    Table(String),
    Enum(String),

    Within(String),
    Field {
        name: String,
        desc: String,
    },
    Alias(String),
    Compact(Vec<String>),
    Fullnames,
    Deprecated(Option<String>),
    Inherits(Vec<String>),
    Meta(String),
    Since(String),
    Scope(String),
    Rename(String),
    Display(String),
    Type(Vec<String>),
    Order {
        whence: String,
        anchor: Option<String>,
    },

    // Content tags: handled when the content block is assembled, not when it is scanned.
    Code {
        lang: Option<String>,
        snippet: Option<String>,
    },
    Usage {
        lang: Option<String>,
        snippet: Option<String>,
    },
    Example {
        lang: Option<String>,
        snippet: Option<String>,
    },
    Note(Option<String>),
    Warning(Option<String>),
    See(Vec<String>),
    Param {
        types: Vec<String>,
        name: String,
        desc: Option<String>,
    },
    Return {
        types: Vec<String>,
        desc: Option<String>,
    },

    /// A `@something` luadox does not know. Carried rather than dropped so the line it
    /// was on can still be reported.
    Unrecognized(String),
}

impl Tag {
    /// The name the Python derives from the tag class, used in messages, in flag keys and
    /// as an admonition's level.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Module(_) => "module",
            Self::Class(_) => "class",
            Self::Section(_) => "section",
            Self::Table(_) => "table",
            Self::Enum(_) => "enum",
            Self::Within(_) => "within",
            Self::Field { .. } => "field",
            Self::Alias(_) => "alias",
            Self::Compact(_) => "compact",
            Self::Fullnames => "fullnames",
            Self::Deprecated(_) => "deprecated",
            Self::Inherits(_) => "inherits",
            Self::Meta(_) => "meta",
            Self::Since(_) => "since",
            Self::Scope(_) => "scope",
            Self::Rename(_) => "rename",
            Self::Display(_) => "display",
            Self::Type(_) => "type",
            Self::Order { .. } => "order",
            Self::Code { .. } => "code",
            Self::Usage { .. } => "usage",
            Self::Example { .. } => "example",
            Self::Note(_) => "note",
            Self::Warning(_) => "warning",
            Self::See(_) => "see",
            Self::Param { .. } => "param",
            Self::Return { .. } => "return",
            Self::Unrecognized(_) => "unrecognized",
        }
    }

    /// The name a diagnostic should print: an unrecognized tag's own spelling, otherwise
    /// the tag type.
    pub fn reported_name(&self) -> &str {
        match self {
            Self::Unrecognized(name) => name,
            other => other.type_name(),
        }
    }

    pub fn is_collection(&self) -> bool {
        matches!(
            self,
            Self::Module(_) | Self::Class(_) | Self::Section(_) | Self::Table(_) | Self::Enum(_)
        )
    }

    pub fn collection_name(&self) -> Option<&str> {
        match self {
            Self::Module(n)
            | Self::Class(n)
            | Self::Section(n)
            | Self::Table(n)
            | Self::Enum(n) => Some(n),
            _ => None,
        }
    }

    /// `@usage` and `@example` print a heading above their code block; `@code` does not.
    pub fn code_heading(&self) -> Option<&'static str> {
        match self {
            Self::Usage { .. } => Some("Usage"),
            Self::Example { .. } => Some("Example"),
            _ => None,
        }
    }

    pub fn as_code(&self) -> Option<(Option<&str>, Option<&str>)> {
        match self {
            Self::Code { lang, snippet }
            | Self::Usage { lang, snippet }
            | Self::Example { lang, snippet } => Some((lang.as_deref(), snippet.as_deref())),
            _ => None,
        }
    }

    pub fn as_admonition(&self) -> Option<(&'static str, Option<&str>)> {
        match self {
            Self::Note(title) => Some(("note", title.as_deref())),
            Self::Warning(title) => Some(("warning", title.as_deref())),
            _ => None,
        }
    }

    /// True for the tags whose body is the indented text that follows them.
    pub fn takes_content(&self) -> bool {
        matches!(
            self,
            Self::Note(_)
                | Self::Warning(_)
                | Self::Deprecated(_)
                | Self::Param { .. }
                | Self::Return { .. }
        )
    }
}

#[derive(Debug, Clone)]
pub struct TagError {
    pub tag: String,
    pub detail: String,
}

impl fmt::Display for TagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{} is invalid: {}", self.tag, self.detail)
    }
}

impl std::error::Error for TagError {}

/// Finds a `@tag` at the head of a line.
///
/// `require_comment` mirrors the Python's two patterns: source lines must open with
/// dashes, manual pages need not. Both require at least two characters after the `@` and
/// reject `@{`, which is a cross reference rather than a tag.
pub fn parse(line: &str, require_comment: bool) -> Result<Vec<Tag>, TagError> {
    let rest = if require_comment {
        let dashes = line.trim_start_matches('-');
        if dashes.len() + 2 > line.len() {
            // Fewer than the two dashes `--+` demands.
            return Ok(Vec::new());
        }
        dashes
    } else {
        line
    };
    let rest = rest.trim_start_matches(' ');
    let Some(rest) = rest.strip_prefix('@') else {
        return Ok(Vec::new());
    };
    // `@([^{]\S+)`: a first character that is not `{`, then at least one more.
    let mut chars = rest.chars();
    match (chars.next(), chars.next()) {
        (Some(c), Some(_)) if c != '{' && !c.is_whitespace() => {}
        _ => return Ok(Vec::new()),
    }
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let (name, argstr) = (rest.get(..end).unwrap_or(""), rest.get(end..).unwrap_or(""));
    if name.chars().any(char::is_whitespace) {
        return Ok(Vec::new());
    }
    let args: Vec<&str> = argstr.split_whitespace().collect();
    build(name, &args)
}

fn need<'a>(tag: &str, args: &'a [&str], n: usize) -> Result<&'a str, TagError> {
    args.get(n).copied().ok_or_else(|| TagError {
        tag: tag.to_string(),
        detail: format!("requires at least {} arguments", n + 1),
    })
}

/// Everything from argument `n` on, joined with single spaces: the `VarString` type.
fn var_string(args: &[&str], n: usize) -> Option<String> {
    let rest = args.get(n..)?;
    if rest.is_empty() {
        return None;
    }
    Some(rest.join(" "))
}

fn pipe_list(arg: &str) -> Vec<String> {
    arg.split('|').map(str::to_string).collect()
}

fn build(name: &str, args: &[&str]) -> Result<Vec<Tag>, TagError> {
    let one = |n: usize| need(name, args, n).map(str::to_string);
    let tag = match name {
        "module" => Tag::Module(one(0)?),
        "section" => Tag::Section(one(0)?),
        "table" => Tag::Table(one(0)?),
        "enum" => Tag::Enum(one(0)?),
        "within" => Tag::Within(one(0)?),
        "alias" => Tag::Alias(one(0)?),
        "meta" => Tag::Meta(one(0)?),
        "scope" => Tag::Scope(one(0)?),
        "rename" => Tag::Rename(one(0)?),
        "display" => Tag::Display(one(0)?),
        "fullnames" => Tag::Fullnames,
        "class" => return class(args),
        "field" => Tag::Field {
            name: one(0)?,
            desc: var_string(args, 1).unwrap_or_default(),
        },
        "compact" => Tag::Compact(if args.is_empty() {
            vec!["fields".to_string(), "functions".to_string()]
        } else {
            args.iter().map(|s| s.to_string()).collect()
        }),
        "deprecated" => Tag::Deprecated(var_string(args, 0)),
        "inherits" => Tag::Inherits(args.iter().map(|s| s.to_string()).collect()),
        "since" => Tag::Since(var_string(args, 0).unwrap_or_default()),
        "type" => Tag::Type(pipe_list(need(name, args, 0)?)),
        "order" => Tag::Order {
            whence: one(0)?,
            anchor: args.get(1).map(|s| s.to_string()),
        },
        "code" => Tag::Code {
            lang: args.first().map(|s| s.to_string()),
            snippet: args.get(1).map(|s| s.to_string()),
        },
        "usage" => Tag::Usage {
            lang: args.first().map(|s| s.to_string()),
            snippet: args.get(1).map(|s| s.to_string()),
        },
        "example" => Tag::Example {
            lang: args.first().map(|s| s.to_string()),
            snippet: args.get(1).map(|s| s.to_string()),
        },
        "note" => Tag::Note(var_string(args, 0)),
        "warning" => Tag::Warning(var_string(args, 0)),
        "see" => Tag::See(args.iter().map(|s| s.to_string()).collect()),
        "tparam" => Tag::Param {
            types: pipe_list(need(name, args, 0)?),
            name: one(1)?,
            desc: var_string(args, 2),
        },
        "treturn" => Tag::Return {
            types: pipe_list(need(name, args, 0)?),
            desc: var_string(args, 1),
        },
        other => Tag::Unrecognized(other.to_string()),
    };
    Ok(vec![tag])
}

/// `@class` also accepts the LuaCATS form `@class Name: Parent`, which becomes a class
/// plus an implicit `@inherits`.
fn class(args: &[&str]) -> Result<Vec<Tag>, TagError> {
    let name = need("class", args, 0)?;
    let extra: Vec<String> = args
        .get(1..)
        .unwrap_or(&[])
        .iter()
        .map(|s| s.to_string())
        .collect();
    if let Some(stripped) = name.strip_suffix(':') {
        if extra.is_empty() {
            return Err(TagError {
                tag: "class".to_string(),
                detail: "class name ends with colon but tag is missing parent class argument"
                    .to_string(),
            });
        }
        return Ok(vec![Tag::Class(stripped.to_string()), Tag::Inherits(extra)]);
    }
    if !extra.is_empty() {
        let joined = extra.join(" ");
        let detail = if joined.trim_matches(':').is_empty() {
            format!("the colon must be attached to the class name, as \"@class {name}: parent\"")
        } else {
            format!(
                "write \"@class {name}: {joined}\" to declare a parent, or use a separate \
                 @inherits tag"
            )
        };
        return Err(TagError {
            tag: "class".to_string(),
            detail: format!("unexpected argument after the class name; {detail}"),
        });
    }
    Ok(vec![Tag::Class(name.to_string())])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(line: &str) -> Option<Tag> {
        parse(line, true).ok()?.into_iter().next()
    }

    #[test]
    fn a_tag_needs_two_characters_and_may_not_be_a_cross_reference() {
        assert_eq!(one("--- @{Widget}"), None);
        assert_eq!(one("--- plain text"), None);
        assert_eq!(one("--- @class Widget"), Some(Tag::Class("Widget".into())));
        assert_eq!(one("---@class Widget"), Some(Tag::Class("Widget".into())));
    }

    #[test]
    fn tparam_splits_types_on_pipes_and_keeps_the_rest_as_prose() {
        assert_eq!(
            one("-- @tparam number|nil index Index of the child."),
            Some(Tag::Param {
                types: vec!["number".into(), "nil".into()],
                name: "index".into(),
                desc: Some("Index of the child.".into()),
            })
        );
    }

    #[test]
    fn compact_defaults_to_both_element_kinds() {
        assert_eq!(
            one("--- @compact"),
            Some(Tag::Compact(vec!["fields".into(), "functions".into()]))
        );
        assert_eq!(
            one("--- @compact fields"),
            Some(Tag::Compact(vec!["fields".into()]))
        );
    }

    #[test]
    fn the_luacats_class_form_yields_an_implicit_inherits() {
        assert_eq!(
            parse("--- @class Widget: Node", true).unwrap_or_default(),
            vec![
                Tag::Class("Widget".into()),
                Tag::Inherits(vec!["Node".into()])
            ]
        );
    }

    #[test]
    fn a_class_with_a_stray_argument_says_what_to_write_instead() {
        let Err(err) = parse("--- @class Widget Node", true) else {
            panic!("a stray argument after the class name must be rejected");
        };
        assert!(err.detail.contains("@class Widget: Node"), "{err}");
    }

    #[test]
    fn an_unknown_tag_is_carried_rather_than_dropped() {
        assert_eq!(
            one("--- @nonsense x"),
            Some(Tag::Unrecognized("nonsense".into()))
        );
    }
}
