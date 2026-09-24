//! What a run was configured with, read once from the `.conf` and typed.
//!
//! `Config` stays the reader, because its section order is what puts manual pages in
//! file order. Everything after it asks this for a value instead, so each fallback rule
//! -- which of `out` and `outdir` wins, what a page is titled when no `title` is set --
//! is written in one place rather than once per renderer.

use std::path::PathBuf;

use crate::config::Config;
use crate::util;
use crate::{Error, Renderer};

/// What a page is titled when the project sets neither `title` nor `name`.
const DEFAULT_TITLE: &str = "Lua Project";

#[derive(Debug, Clone)]
pub struct Settings {
    pub name: Option<String>,
    pub title: Option<String>,
    /// `project.out`, or the older `project.outdir` when only that is set.
    pub out: Option<String>,
    pub renderer: Renderer,
    /// One glob pattern per token of `project.files`, without the `alias=` prefix a token
    /// may carry.
    pub files: Vec<String>,
    pub snippet_path: Option<String>,
    pub css: Vec<String>,
    pub js: Vec<String>,
    pub favicon: Vec<String>,
    pub templates: Templates,
    pub allow_incomplete: String,
    pub encoding: String,
    pub follow: bool,
    /// The `[manual]` pages as `(name, path)`, in file order, which is their render
    /// order.
    pub manual: Vec<(String, String)>,
    pub luals: Luals,
    /// The `[link*]` sections, in section-name order, which is their order on the topbar.
    pub links: Vec<Link>,
}

/// Paths of the html templates a project supplies; `None` uses the bundled one.
#[derive(Debug, Clone, Default)]
pub struct Templates {
    pub head: Option<String>,
    pub foot: Option<String>,
    pub search: Option<String>,
    pub sidebar: Option<String>,
}

/// The `[luals]` section. `globals` is kept as written: the LuaLS renderer parses it,
/// and a malformed token is its error to report.
#[derive(Debug, Clone, Default)]
pub struct Luals {
    pub mixin_suffix: String,
    pub mixin_doc_phrase: String,
    pub globals: String,
}

/// One topbar link. The Python has no fallback for a missing `text` and dies on it; an
/// empty label is a better answer than a traceback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub icon: Option<String>,
    pub url: String,
    pub tooltip: String,
    pub text: String,
}

impl Settings {
    pub fn from_config(config: &Config) -> Result<Self, Error> {
        let project = |key: &str| config.get("project", key).map(str::to_string);
        let listed = |key: &str| -> Vec<String> {
            config
                .get("project", key)
                .unwrap_or("")
                .lines()
                .flat_map(util::shlex_split)
                .collect()
        };
        let mut links: Vec<(&str, Link)> = config
            .sections_with_prefix("link")
            .into_iter()
            .map(|(name, options)| {
                let get = |key: &str| {
                    options
                        .iter()
                        .find(|(k, _)| k == key)
                        .map(|(_, v)| v.clone())
                };
                let link = Link {
                    icon: get("icon"),
                    url: get("url").unwrap_or_default(),
                    tooltip: get("tooltip").unwrap_or_default(),
                    text: get("text").unwrap_or_default(),
                };
                (name, link)
            })
            .collect();
        links.sort_by(|a, b| a.0.cmp(b.0));

        Ok(Self {
            name: project("name"),
            title: project("title"),
            out: project("out").or_else(|| project("outdir")),
            renderer: config.get_or("project", "renderer", "html").parse()?,
            files: listed("files")
                .into_iter()
                .map(|spec| strip_alias(&spec))
                .collect(),
            snippet_path: project("snippet_path"),
            css: listed("css"),
            js: listed("js"),
            favicon: listed("favicon"),
            templates: Templates {
                head: project("head_template"),
                foot: project("foot_template"),
                search: project("search_template"),
                sidebar: project("sidebar_template"),
            },
            allow_incomplete: config.get_or("project", "allow_incomplete", ""),
            encoding: config.get_or("project", "encoding", "utf8"),
            follow: config.get_bool("project", "follow", true),
            manual: config.items("manual").to_vec(),
            luals: Luals {
                mixin_suffix: config.get_or("luals", "mixin_suffix", ""),
                mixin_doc_phrase: config.get_or("luals", "mixin_doc_phrase", ""),
                globals: config.get_or("luals", "globals", ""),
            },
            links: links.into_iter().map(|(_, link)| link).collect(),
        })
    }

    /// `title`, then `name`, and nothing when neither is set: what the LuaLS header
    /// prints, or leaves out.
    pub fn title_or_name(&self) -> Option<&str> {
        self.title.as_deref().or(self.name.as_deref())
    }

    /// What every html page title ends with: `title`, then `name`, then the default.
    pub fn project_title(&self) -> &str {
        self.title_or_name().unwrap_or(DEFAULT_TITLE)
    }

    /// The topbar's home label: `name`, then `title`, then the default.
    pub fn hometext(&self) -> &str {
        self.name
            .as_deref()
            .or(self.title.as_deref())
            .unwrap_or(DEFAULT_TITLE)
    }

    /// Whether a `[manual] index` page is configured, which decides whether the topbar's
    /// home label is a link. A repeated key counts by its last value, as `ConfigParser`
    /// reads one.
    pub fn has_manual_index(&self) -> bool {
        self.manual
            .iter()
            .rev()
            .find(|(name, _)| name == "index")
            .is_some_and(|(_, path)| !path.is_empty())
    }

    /// Where the html renderer writes its tree.
    pub fn html_out_dir(&self) -> PathBuf {
        self.out
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("out"))
    }

    /// Where a single-file renderer writes: the configured path when it names a file or
    /// carries the renderer's extension, otherwise `luadox<ext>` inside it, and
    /// `./luadox<ext>` when nothing is configured.
    pub fn out_path(&self, extension: &str) -> PathBuf {
        let Some(dst) = &self.out else {
            return PathBuf::from(format!("./luadox{extension}"));
        };
        let path = PathBuf::from(dst);
        if path.is_file() || dst.ends_with(extension) {
            path
        } else {
            path.join(format!("luadox{extension}"))
        }
    }
}

/// `alias=path` becomes `path`; an alias may not contain a path separator, which is what
/// keeps a Windows drive letter from reading as one.
fn strip_alias(spec: &str) -> String {
    match spec.split_once('=') {
        Some((alias, rest)) if !alias.contains('/') && !alias.contains('\\') => rest.to_string(),
        _ => spec.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(text: &str) -> Settings {
        let config = Config::parse(text).unwrap_or_default();
        match Settings::from_config(&config) {
            Ok(settings) => settings,
            Err(e) => panic!("{text:?} must be a valid configuration: {e}"),
        }
    }

    /// The html renderer's rule, as `render::html::render` wrote it: the page title is
    /// `title`, falling back to `name`, falling back to "Lua Project".
    #[test]
    fn the_html_project_title_is_title_then_name_then_the_default() {
        assert_eq!(
            settings("[project]\nname = N\ntitle = T\n").project_title(),
            "T"
        );
        assert_eq!(settings("[project]\nname = N\n").project_title(), "N");
        assert_eq!(settings("[project]\n").project_title(), "Lua Project");
    }

    /// The html topbar's home label prefers `name`, where the page title prefers `title`;
    /// with neither set both read "Lua Project".
    #[test]
    fn the_html_hometext_is_name_then_title_then_the_default() {
        assert_eq!(settings("[project]\nname = N\ntitle = T\n").hometext(), "N");
        assert_eq!(settings("[project]\ntitle = T\n").hometext(), "T");
        assert_eq!(settings("[project]\n").hometext(), "Lua Project");
    }

    /// The LuaLS header's rule, as `render::luals::render` wrote it: `title`, then
    /// `name`, and no line at all when neither is set -- there is no default here.
    #[test]
    fn the_luals_header_title_is_title_then_name_with_no_default() {
        assert_eq!(
            settings("[project]\nname = N\ntitle = T\n").title_or_name(),
            Some("T")
        );
        assert_eq!(settings("[project]\nname = N\n").title_or_name(), Some("N"));
        assert_eq!(settings("[project]\n").title_or_name(), None);
    }

    /// The json renderer writes `name` and `title` as they are, each only when set, with
    /// no fallback from one to the other.
    #[test]
    fn json_reads_name_and_title_independently() {
        let both = settings("[project]\nname = N\ntitle = T\n");
        assert_eq!(
            (both.name.as_deref(), both.title.as_deref()),
            (Some("N"), Some("T"))
        );
        let name_only = settings("[project]\nname = N\n");
        assert_eq!(
            (name_only.name.as_deref(), name_only.title.as_deref()),
            (Some("N"), None)
        );
    }

    /// `out` wins over `outdir`; with neither, the html tree goes to `out` and a
    /// single-file renderer to `./luadox<ext>`.
    #[test]
    fn out_falls_back_to_outdir_and_then_to_the_defaults() {
        assert_eq!(
            settings("[project]\nout = a\noutdir = b\n").html_out_dir(),
            PathBuf::from("a")
        );
        assert_eq!(
            settings("[project]\noutdir = b\n").html_out_dir(),
            PathBuf::from("b")
        );
        assert_eq!(settings("[project]\n").html_out_dir(), PathBuf::from("out"));
        assert_eq!(
            settings("[project]\n").out_path(".json"),
            PathBuf::from("./luadox.json")
        );
    }

    /// A configured path that already carries the renderer's extension is the file;
    /// anything else is a directory the file goes into.
    #[test]
    fn a_single_file_output_is_the_path_or_a_file_inside_it() {
        assert_eq!(
            settings("[project]\nout = docs/api.lua\n").out_path(".lua"),
            PathBuf::from("docs/api.lua")
        );
        assert_eq!(
            settings("[project]\noutdir = docs\n").out_path(".lua"),
            PathBuf::from("docs").join("luadox.lua")
        );
    }

    #[test]
    fn a_manual_index_counts_only_when_it_names_a_file() {
        assert!(settings("[manual]\nindex = a.md\n").has_manual_index());
        assert!(!settings("[manual]\nindex =\n").has_manual_index());
        assert!(!settings("[manual]\nintro = a.md\n").has_manual_index());
        assert!(!settings("[project]\n").has_manual_index());
    }

    #[test]
    fn manual_pages_keep_file_order() {
        let s = settings("[manual]\nzeta = z.md\nalpha = a.md\n");
        assert_eq!(
            s.manual,
            vec![
                ("zeta".to_string(), "z.md".to_string()),
                ("alpha".to_string(), "a.md".to_string())
            ]
        );
    }

    /// `files` is one or more shell-split globs per line, each optionally prefixed with a
    /// module alias, which is dropped.
    #[test]
    fn files_are_split_per_line_and_lose_their_alias() {
        let s = settings("[project]\nfiles = a/*.lua m=b/*.lua\n        \"c d/*.lua\"\n");
        assert_eq!(s.files, vec!["a/*.lua", "b/*.lua", "c d/*.lua"]);
        let s = settings("[project]\nfiles = C:\\\\x=y/*.lua\n");
        assert_eq!(s.files, vec!["C:\\x=y/*.lua"]);
    }

    #[test]
    fn css_js_and_favicon_are_split_the_same_way() {
        let s = settings("[project]\ncss = a.css\n      b.css c.css\nfavicon = f.png\n");
        assert_eq!(s.css, vec!["a.css", "b.css", "c.css"]);
        assert!(s.js.is_empty());
        assert_eq!(s.favicon, vec!["f.png"]);
    }

    /// Link sections come out in section-name order whatever the file order, and a
    /// section missing `text` gets an empty label rather than an error.
    #[test]
    fn links_are_sorted_by_section_name() {
        let s = settings(concat!(
            "[link2]\nurl = u2\ntext = second\n",
            "[link1]\nicon = github\nurl = u1\ntooltip = tip\n",
        ));
        assert_eq!(
            s.links,
            vec![
                Link {
                    icon: Some("github".to_string()),
                    url: "u1".to_string(),
                    tooltip: "tip".to_string(),
                    text: String::new(),
                },
                Link {
                    icon: None,
                    url: "u2".to_string(),
                    tooltip: String::new(),
                    text: "second".to_string(),
                },
            ]
        );
    }

    #[test]
    fn the_renderer_defaults_to_html_and_an_unknown_one_is_an_error() {
        assert_eq!(settings("[project]\n").renderer, Renderer::Html);
        assert_eq!(
            settings("[project]\nrenderer = luals\n").renderer,
            Renderer::Luals
        );
        let config = Config::parse("[project]\nrenderer = pdf\n").unwrap_or_default();
        assert!(matches!(
            Settings::from_config(&config),
            Err(Error::UnknownRenderer(name)) if name == "pdf"
        ));
    }

    #[test]
    fn follow_and_encoding_have_their_defaults() {
        let s = settings("[project]\n");
        assert!(s.follow);
        assert_eq!(s.encoding, "utf8");
        assert!(!settings("[project]\nfollow = no\n").follow);
    }
}
