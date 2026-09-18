//! Problems that leave the rendered documentation incomplete.
//!
//! A category is an enum, not a string: the Python's two branches disagreed on which
//! categories exist, and a typo in `allow_incomplete` there silently accepted nothing.

use std::collections::BTreeSet;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Conflicts,
    References,
    Snippets,
    Structure,
    Types,
    UndocumentedEnumMembers,
    UndocumentedSectionMembers,
    Untyped,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Conflicts,
        Category::References,
        Category::Snippets,
        Category::Structure,
        Category::Types,
        Category::UndocumentedEnumMembers,
        Category::UndocumentedSectionMembers,
        Category::Untyped,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Conflicts => "conflicts",
            Category::References => "references",
            Category::Snippets => "snippets",
            Category::Structure => "structure",
            Category::Types => "types",
            Category::UndocumentedEnumMembers => "undocumented-enum-members",
            Category::UndocumentedSectionMembers => "undocumented-section-members",
            Category::Untyped => "untyped",
        }
    }

    pub fn parse(name: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.as_str() == name)
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub category: Category,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Diagnostics {
    allowed: BTreeSet<Category>,
    entries: Vec<Entry>,
    /// Names in `allow_incomplete` that are not categories, kept so the run can say so
    /// once rather than silently accepting nothing.
    pub unknown_allowed: Vec<String>,
}

impl Diagnostics {
    /// Parses `allow_incomplete`, which is a comma- or whitespace-separated list.
    pub fn from_allow_incomplete(value: &str) -> Diagnostics {
        let mut allowed = BTreeSet::new();
        let mut unknown = Vec::new();
        for name in value
            .split([',', ' ', '\t', '\n', '\r'])
            .filter(|s| !s.is_empty())
        {
            match Category::parse(name) {
                Some(cat) => {
                    allowed.insert(cat);
                }
                None => unknown.push(name.to_string()),
            }
        }
        Diagnostics {
            allowed,
            entries: Vec::new(),
            unknown_allowed: unknown,
        }
    }

    pub fn add(
        &mut self,
        category: Category,
        message: impl Into<String>,
        file: Option<&str>,
        line: Option<u32>,
    ) {
        self.entries.push(Entry {
            category,
            file: file.map(str::to_string),
            line,
            message: message.into(),
        });
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn allowed(&self) -> impl Iterator<Item = Category> + '_ {
        self.allowed.iter().copied()
    }

    pub fn count(&self, category: Category) -> usize {
        self.entries
            .iter()
            .filter(|e| e.category == category)
            .count()
    }

    /// 1 if any category with entries was not accepted with `allow_incomplete`.
    pub fn exit_code(&self) -> i32 {
        let failing = self
            .entries
            .iter()
            .any(|e| !self.allowed.contains(&e.category));
        i32::from(failing)
    }

    /// One line per category, allowed ones first marked as such: what a run reports on
    /// the way out.
    pub fn summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        for category in Category::ALL {
            let n = self.count(category);
            if n == 0 {
                continue;
            }
            let how = if self.allowed.contains(&category) {
                "allowed"
            } else {
                "ERROR"
            };
            out.push(format!(
                "{how}: {n} {category} problem(s) leave the documentation incomplete"
            ));
        }
        out
    }
}
