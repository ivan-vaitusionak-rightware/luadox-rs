//! Problems that leave the rendered documentation incomplete.
//!
//! A category is an enum, not a string: the Python's two branches disagreed on which
//! categories exist, and a typo in `allow_incomplete` there silently accepted nothing.
//! Here it is an error at the edge, where the name is parsed.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    CompactBlockContent,
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
    pub const ALL: [Self; 9] = [
        Self::CompactBlockContent,
        Self::Conflicts,
        Self::References,
        Self::Snippets,
        Self::Structure,
        Self::Types,
        Self::UndocumentedEnumMembers,
        Self::UndocumentedSectionMembers,
        Self::Untyped,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CompactBlockContent => "compact-block-content",
            Self::Conflicts => "conflicts",
            Self::References => "references",
            Self::Snippets => "snippets",
            Self::Structure => "structure",
            Self::Types => "types",
            Self::UndocumentedEnumMembers => "undocumented-enum-members",
            Self::UndocumentedSectionMembers => "undocumented-section-members",
            Self::Untyped => "untyped",
        }
    }
}

impl FromStr for Category {
    type Err = Error;

    fn from_str(name: &str) -> Result<Self, Error> {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str() == name)
            .ok_or_else(|| Error::UnknownCategory(name.to_string()))
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
}

impl Diagnostics {
    /// Diagnostics whose `allowed` categories leave the documentation incomplete without
    /// failing the run.
    pub fn allowing(allowed: BTreeSet<Category>) -> Self {
        Self {
            allowed,
            entries: Vec::new(),
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
