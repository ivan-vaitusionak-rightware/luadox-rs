//! A JSON value whose object keys stay in the order they were inserted, written exactly
//! the way Python's `json.dump(obj, f, indent=2)` writes one.
//!
//! Key order is not cosmetic here: `doc.json` is compared structurally against the
//! Python's output, and the Python's dicts are insertion-ordered, so a renderer that sorts its
//! keys would produce a document that is equal but not identical. `ensure_ascii` is
//! matched too -- the corpus is pure ASCII today, so nothing would show, and a silent
//! divergence waiting for the first non-ASCII character is worse than a one-line setting.

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Bool(bool),
    Int(i64),
    Str(String),
    Arr(Vec<Self>),
    Obj(Vec<(String, Self)>),
}

impl Json {
    pub fn obj() -> Self {
        Self::Obj(Vec::new())
    }

    /// Appends a key, keeping insertion order.
    pub fn set(&mut self, key: &str, value: Self) {
        if let Self::Obj(fields) = self {
            fields.push((key.to_string(), value));
        }
    }

    /// Appends a key only when the value is not empty, which is how the Python's
    /// `{k: v for k, v in kwargs.items() if v}` and its `if content:` guards behave.
    pub fn set_if(&mut self, key: &str, value: Self) {
        if !value.is_falsy() {
            self.set(key, value);
        }
    }

    pub fn is_falsy(&self) -> bool {
        match self {
            Self::Bool(b) => !b,
            Self::Int(n) => *n == 0,
            Self::Str(s) => s.is_empty(),
            Self::Arr(v) => v.is_empty(),
            Self::Obj(v) => v.is_empty(),
        }
    }

    pub fn write(&self) -> String {
        let mut out = String::new();
        self.write_into(&mut out, 0);
        out
    }

    fn write_into(&self, out: &mut String, depth: usize) {
        match self {
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Int(n) => {
                let _ = write!(out, "{n}");
            }
            Self::Str(s) => write_string(out, s),
            Self::Arr(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(",\n");
                    }
                    indent(out, depth + 1);
                    item.write_into(out, depth + 1);
                }
                out.push('\n');
                indent(out, depth);
                out.push(']');
            }
            Self::Obj(fields) => {
                if fields.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{\n");
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(",\n");
                    }
                    indent(out, depth + 1);
                    write_string(out, key);
                    out.push_str(": ");
                    value.write_into(out, depth + 1);
                }
                out.push('\n');
                indent(out, depth);
                out.push('}');
            }
        }
    }
}

impl From<&str> for Json {
    fn from(s: &str) -> Self {
        Self::Str(s.to_string())
    }
}

impl From<String> for Json {
    fn from(s: String) -> Self {
        Self::Str(s)
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth * 2 {
        out.push(' ');
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c if c.is_ascii() => out.push(c),
            c => {
                // ensure_ascii: astral characters become a surrogate pair, the way
                // Python's encoder writes them.
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_what_python_writes() {
        let mut root = Json::obj();
        root.set("apiVersion", "v1alpha1".into());
        root.set("classes", Json::Arr(vec![]));
        let mut one = Json::obj();
        one.set("name", "A".into());
        one.set("n", Json::Int(3));
        root.set("modules", Json::Arr(vec![one]));
        assert_eq!(
            root.write(),
            "{\n  \"apiVersion\": \"v1alpha1\",\n  \"classes\": [],\n  \"modules\": [\n    {\n      \"name\": \"A\",\n      \"n\": 3\n    }\n  ]\n}"
        );
    }

    #[test]
    fn escapes_the_way_ensure_ascii_does() {
        assert_eq!(
            Json::Str("a\"b\\c\nd".into()).write(),
            "\"a\\\"b\\\\c\\nd\""
        );
        assert_eq!(Json::Str("caf\u{e9}".into()).write(), "\"caf\\u00e9\"");
        assert_eq!(Json::Str("\u{1f600}".into()).write(), "\"\\ud83d\\ude00\"");
    }
}
