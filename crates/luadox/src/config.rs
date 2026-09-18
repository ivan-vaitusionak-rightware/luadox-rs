//! The `.conf` reader.
//!
//! The file format is Python `ConfigParser` with `inline_comment_prefixes='#'`, and two
//! of its behaviours are load-bearing rather than incidental:
//!
//!   * a `#` preceded by whitespace starts a comment in the middle of a value;
//!   * a value continues onto the following lines while they are indented deeper than
//!     the line that opened it -- which is how `files` in `engine-lua-api.conf` lists five
//!     globs.
//!
//! No Rust INI crate claims either, so this is written by hand against the dialect rather
//! than against "INI".

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug)]
pub struct ConfigError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ConfigError {}

/// Sections and options in the order the file declares them; `ConfigParser` preserves
/// insertion order and `[manual]` depends on it for page order.
#[derive(Debug, Default, Clone)]
pub struct Config {
    sections: Vec<Section>,
}

#[derive(Debug, Default, Clone)]
struct Section {
    name: String,
    options: Vec<(String, String)>,
}

impl Config {
    pub fn parse(text: &str) -> Result<Config, ConfigError> {
        let mut config = Config::default();
        // Accumulated value lines for the option currently open, if any.
        let mut open: Option<(String, Vec<String>)> = None;
        // Indent of the line that opened the current option.  A deeper-indented line
        // continues it; anything else closes it.
        let mut indent_level = 0usize;

        for (index, raw) in text.lines().enumerate() {
            let lineno = index + 1;
            let (value, had_comment) = strip_comments(raw);
            let first_nonspace = raw.find(|c: char| !c.is_whitespace());

            if value.is_empty() {
                // A blank line inside a value keeps the paragraph break, unless the line
                // was blank only because a comment was removed from it.
                if !had_comment {
                    if let Some((_, lines)) = open.as_mut() {
                        lines.push(String::new());
                    }
                }
                continue;
            }

            let cur_indent = first_nonspace.unwrap_or(0);
            if open.is_some() && cur_indent > indent_level {
                if let Some((_, lines)) = open.as_mut() {
                    lines.push(value.to_string());
                }
                continue;
            }

            indent_level = cur_indent;
            config.close(&mut open);

            if let Some(name) = value.strip_prefix('[') {
                let Some(name) = name.strip_suffix(']') else {
                    return Err(ConfigError {
                        line: lineno,
                        message: format!("unterminated section header: {value}"),
                    });
                };
                config.sections.push(Section {
                    name: name.to_string(),
                    options: Vec::new(),
                });
                continue;
            }

            let Some((key, rest)) = split_option(value) else {
                return Err(ConfigError {
                    line: lineno,
                    message: format!("expected `key = value` or a [section]: {value}"),
                });
            };
            if config.sections.is_empty() {
                return Err(ConfigError {
                    line: lineno,
                    message: format!("option \"{key}\" appears before any [section]"),
                });
            }
            open = Some((key.to_ascii_lowercase(), vec![rest.to_string()]));
        }
        config.close(&mut open);
        Ok(config)
    }

    fn close(&mut self, open: &mut Option<(String, Vec<String>)>) {
        let Some((key, lines)) = open.take() else {
            return;
        };
        let value = lines.join("\n").trim_end().to_string();
        if let Some(section) = self.sections.last_mut() {
            section.options.push((key, value));
        }
    }

    pub fn has_section(&self, name: &str) -> bool {
        self.sections.iter().any(|s| s.name == name)
    }

    pub fn add_section(&mut self, name: &str) {
        if !self.has_section(name) {
            self.sections.push(Section {
                name: name.to_string(),
                options: Vec::new(),
            });
        }
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let key = key.to_ascii_lowercase();
        self.sections
            .iter()
            .find(|s| s.name == section)?
            .options
            .iter()
            .rev()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn get_or(&self, section: &str, key: &str, fallback: &'static str) -> String {
        self.get(section, key).unwrap_or(fallback).to_string()
    }

    /// True unless the option is set to something that reads as false, matching
    /// `main.py`'s `in ('true', '1', 'yes')` test on a lowercased value.
    pub fn get_bool(&self, section: &str, key: &str, fallback: bool) -> bool {
        match self.get(section, key) {
            None => fallback,
            Some(v) => matches!(v.to_ascii_lowercase().as_str(), "true" | "1" | "yes"),
        }
    }

    pub fn set(&mut self, section: &str, key: &str, value: impl Into<String>) {
        self.add_section(section);
        let key = key.to_ascii_lowercase();
        let value = value.into();
        if let Some(s) = self.sections.iter_mut().find(|s| s.name == section) {
            match s.options.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => s.options.push((key, value)),
            }
        }
    }

    /// Every option of a section, in file order.
    pub fn items(&self, section: &str) -> &[(String, String)] {
        self.sections
            .iter()
            .find(|s| s.name == section)
            .map(|s| s.options.as_slice())
            .unwrap_or(&[])
    }

    /// Sections whose name starts with `prefix`, in file order: how `[link*]` sections
    /// are enumerated.
    pub fn sections_with_prefix(&self, prefix: &str) -> Vec<(&str, &[(String, String)])> {
        self.sections
            .iter()
            .filter(|s| s.name.starts_with(prefix))
            .map(|s| (s.name.as_str(), s.options.as_slice()))
            .collect()
    }

    /// Every option of every section, for reporting what a run was configured with.
    pub fn to_map(&self) -> BTreeMap<String, BTreeMap<String, String>> {
        self.sections
            .iter()
            .map(|s| {
                (
                    s.name.clone(),
                    s.options.iter().cloned().collect::<BTreeMap<_, _>>(),
                )
            })
            .collect()
    }
}

/// Removes a full-line or inline comment, returning the value and whether anything was
/// removed. `#` only opens an inline comment at the start of the line or after
/// whitespace, which is what lets a `#` appear inside a path.
fn strip_comments(line: &str) -> (&str, bool) {
    let trimmed = line.trim();
    if trimmed.starts_with('#') || trimmed.starts_with(';') {
        return ("", true);
    }
    let mut cut = None;
    for (i, c) in line.char_indices() {
        if c != '#' {
            continue;
        }
        let preceded_by_space = line
            .get(..i)
            .and_then(|s| s.chars().next_back())
            .is_none_or(char::is_whitespace);
        if i == 0 || preceded_by_space {
            cut = Some(i);
            break;
        }
    }
    match cut {
        Some(i) => (line.get(..i).unwrap_or("").trim(), true),
        None => (trimmed, false),
    }
}

/// Splits `key = value` or `key : value` on the first delimiter.
fn split_option(line: &str) -> Option<(&str, &str)> {
    let eq = line.find('=');
    let colon = line.find(':');
    let at = match (eq, colon) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }?;
    let key = line.get(..at)?.trim();
    let value = line.get(at + 1..)?.trim();
    if key.is_empty() {
        return None;
    }
    Some((key, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact shape `engine-lua-api.conf` uses: a five-line `files` value, a `#`
    /// comment column, and a second section whose order matters.
    #[test]
    fn production_config_shape() {
        let text = concat!(
            "[project]\n",
            "# Project name that is displayed on the top bar of each page.\n",
            "name = Engine Lua API\n",
            "files = ../../../lua/src/*.lua\n",
            "        ../../../lua/src/autogen/*.lua\n",
            "        ../../../lua/src/math-docs/*.lua\n",
            "outdir = ../../_build/html\n",
            "follow = false\n",
            "\n",
            "[manual]\n",
            "index = ../luadox/manual/index.md\n",
        );
        let config = Config::parse(text).unwrap_or_default();
        assert_eq!(config.get("project", "name"), Some("Engine Lua API"));
        assert_eq!(
            config.get("project", "files"),
            Some(concat!(
                "../../../lua/src/*.lua\n",
                "../../../lua/src/autogen/*.lua\n",
                "../../../lua/src/math-docs/*.lua"
            ))
        );
        assert!(!config.get_bool("project", "follow", true));
        assert_eq!(config.items("manual").len(), 1);
        assert_eq!(
            config.get("manual", "index"),
            Some("../luadox/manual/index.md")
        );
    }

    #[test]
    fn inline_comment_needs_whitespace_before_it() {
        let config =
            Config::parse("[project]\nfavicon = a#b.png  # the icon\n").unwrap_or_default();
        assert_eq!(config.get("project", "favicon"), Some("a#b.png"));
    }

    #[test]
    fn blank_line_inside_a_value_is_kept_but_a_comment_line_is_not() {
        let config = Config::parse("[p]\nk = one\n  two\n\n  three\n").unwrap_or_default();
        assert_eq!(config.get("p", "k"), Some("one\ntwo\n\nthree"));
        let config = Config::parse("[p]\nk = one\n  # note\n  two\n").unwrap_or_default();
        assert_eq!(config.get("p", "k"), Some("one\ntwo"));
    }

    #[test]
    fn keys_are_lowercased_and_a_colon_delimits_too() {
        let config = Config::parse("[p]\nSnippet_Path: /tmp\n").unwrap_or_default();
        assert_eq!(config.get("p", "snippet_path"), Some("/tmp"));
        assert_eq!(config.get("p", "SNIPPET_PATH"), Some("/tmp"));
    }

    #[test]
    fn an_option_before_any_section_is_an_error() {
        let Err(err) = Config::parse("k = v\n") else {
            panic!("an option before any section must be rejected");
        };
        assert!(err.message.contains("before any [section]"), "{err}");
    }
}
