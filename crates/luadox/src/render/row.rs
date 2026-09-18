//! The one-line contexts: a compact synopsis cell, a summary line, a tooltip.
//!
//! Every renderer that lays content out on a single line goes through here, and this
//! module's functions take [`RowContent`] and nothing else. `RowContent` has no
//! constructor but the fallible conversion in `markdown`, so there is no route by which a
//! code block, a heading or a list can reach a row: an attempt to pass one does not
//! compile.
//!
//! That is the whole point. The bug this prevents shipped in the production docs for a year --
//! 240 elements whose documentation was emitted into a one-line table cell and rendered
//! unreadable -- and it was found by reading a page in a browser, not by any check. The
//! next tag combination that does the same thing fails `cargo check` instead.

use crate::markdown::RowContent;

/// The text of a one-line cell.
pub fn cell(content: &RowContent) -> String {
    content.text()
}

/// The text of a one-line cell, cut to `width` characters at a word boundary and closed
/// with an ellipsis.
pub fn summary(content: &RowContent, width: usize) -> String {
    let text = content.text();
    if text.chars().count() <= width {
        return text;
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    let kept = match kept.rsplit_once(char::is_whitespace) {
        Some((head, _)) if !head.is_empty() => head,
        _ => kept.as_str(),
    };
    format!("{}\u{2026}", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Content, Fragment, Markdown};

    fn row(text: &str) -> RowContent {
        let mut md = Markdown::new(false);
        md.append(text);
        let content = Content(vec![Fragment::Markdown(md)]);
        match RowContent::try_from(&content) {
            Ok(row) => row,
            Err(e) => panic!("prose must fit a row, got {e}"),
        }
    }

    #[test]
    fn a_cell_is_the_prose_it_was_given() {
        assert_eq!(cell(&row("Retrieves the child.")), "Retrieves the child.");
    }

    #[test]
    fn a_summary_is_cut_to_width() {
        assert_eq!(
            summary(&row("Retrieves the child."), 12),
            "Retrieves\u{2026}"
        );
        assert_eq!(summary(&row("Short."), 12), "Short.");
    }
}
