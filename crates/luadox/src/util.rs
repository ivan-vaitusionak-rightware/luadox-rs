//! Small text utilities the port needs to reproduce exactly.

use std::fmt::Write;

use blake2::digest::consts::U20;
use blake2::{Blake2b, Digest};

/// Common abbreviations whose period does not end a sentence.
const ABBREV: [(char, &[&str]); 3] = [
    ('e', &["e.g.", "eg.", "etc.", "et al."]),
    ('i', &["i.e.", "ie."]),
    ('v', &["vs."]),
];

/// Splits markdown into its first sentence and the rest.
///
/// A direct port: two consecutive newlines end a sentence, a period ends one when the
/// next character is a space, a newline, or the end of the text, and the abbreviations
/// above are skipped over. The offsets are character offsets in the Python, so this walks
/// characters rather than bytes.
pub fn first_sentence(s: &str) -> (&str, &str) {
    let chars: Vec<char> = s.chars().collect();
    // Byte offset of each character, plus the end, so a character index can address the
    // original string without re-encoding it.
    let mut offsets: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
    offsets.push(s.len());

    let lower: Vec<char> = chars.iter().flat_map(|c| c.to_lowercase()).collect();
    // to_lowercase can expand a character into several, which would desynchronise the
    // two vectors; the corpus never does, and a mismatch means falling back to the
    // simple case rather than reporting a wrong offset.
    let lower = if lower.len() == chars.len() {
        lower
    } else {
        chars.clone()
    };

    if chars.is_empty() {
        return (s, "");
    }
    let end = chars.len() - 1;
    let mut last = '\0';
    let mut n = 0usize;
    let mut found = None;
    while n <= end {
        let Some(&c) = lower.get(n) else { break };
        if c == '\n' && last == '\n' {
            found = Some(n);
            break;
        } else if c == '.' {
            if n == end || matches!(lower.get(n + 1), Some(' ') | Some('\n')) {
                found = Some(n);
                break;
            }
        } else if !last.is_ascii_lowercase() {
            if let Some((_, variants)) = ABBREV.iter().find(|(k, _)| *k == c) {
                for abbr in *variants {
                    let len = abbr.chars().count();
                    let window: String = lower.iter().skip(n).take(len).collect();
                    if window == *abbr {
                        n += len - 1;
                        break;
                    }
                }
            }
        }
        last = *lower.get(n).unwrap_or(&'\0');
        n += 1;
    }

    let Some(n) = found else {
        return (s, "");
    };
    let cut = offsets.get(n).copied().unwrap_or(s.len());
    let rest_start = offsets.get(n + 1).copied().unwrap_or(s.len());
    (
        s.get(..cut).unwrap_or(""),
        s.get(rest_start..).unwrap_or("").trim(),
    )
}

/// The number of leading spaces, matching `^( *)` -- a tab is not indentation here.
pub fn indent_level(s: &str) -> usize {
    s.chars().take_while(|c| *c == ' ').count()
}

/// Everything from the first `--` to the end of the line.
///
/// Kept only for what it is used for in the port -- nothing lexical -- because it is the
/// Python's defect (c): a `--` inside a string literal truncates the line.
pub fn strip_trailing_comment(line: &str) -> &str {
    match line.find("--") {
        Some(i) => line.get(..i).unwrap_or(""),
        None => line,
    }
}

/// `shlex.split` in POSIX mode, which is how one line of the `files` option becomes
/// several globs.
pub fn shlex_split(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '\'' => {
                started = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    current.push(c);
                }
            }
            '"' => {
                started = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => {
                            if let Some(next) = chars.next() {
                                if !matches!(next, '"' | '\\' | '$' | '`') {
                                    current.push('\\');
                                }
                                current.push(next);
                            }
                        }
                        _ => current.push(c),
                    }
                }
            }
            '\\' => {
                started = true;
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            _ => {
                started = true;
                current.push(c);
            }
        }
    }
    if started {
        out.push(current);
    }
    out
}

/// A reference's globally unique opaque id: BLAKE2b-160 over
/// `<topref type>#<topsym>#<name>`, hex encoded.
///
/// The ids appear in `doc.json` and in every `luadox:` link target, so this must stay
/// byte-for-byte what `hashlib.blake2b(s, digest_size=20).hexdigest()` produces.
pub fn ref_id(topref_type: &str, topsym: &str, name: &str) -> String {
    let mut hasher = Blake2b::<U20>::new();
    hasher.update(topref_type.as_bytes());
    hasher.update(b"#");
    hasher.update(topsym.as_bytes());
    hasher.update(b"#");
    hasher.update(name.as_bytes());
    let mut hex = String::with_capacity(40);
    for byte in hasher.finalize() {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sentence_splits_on_a_period_before_whitespace() {
        assert_eq!(
            first_sentence("One two. Three four."),
            ("One two", "Three four.")
        );
        assert_eq!(
            first_sentence("No terminator here"),
            ("No terminator here", "")
        );
        assert_eq!(
            first_sentence("Para one.\n\nPara two."),
            ("Para one", "Para two.")
        );
    }

    #[test]
    fn first_sentence_skips_abbreviations() {
        assert_eq!(
            first_sentence("Use e.g. this one. Then that."),
            ("Use e.g. this one", "Then that.")
        );
    }

    #[test]
    fn first_sentence_breaks_on_a_blank_line() {
        assert_eq!(first_sentence("Heading\n\nBody"), ("Heading\n", "Body"));
    }

    #[test]
    fn ref_id_matches_the_python() {
        // What the Python computes for a class whose top symbol is its own name:
        // hashlib.blake2b(b"class#Widget#Widget", digest_size=20).hexdigest().
        assert_eq!(
            ref_id("class", "Widget", "Widget"),
            "b325db938a25407e452fbf426e16590ec91f88ec"
        );
    }

    #[test]
    fn shlex_split_handles_the_files_option() {
        assert_eq!(
            shlex_split("  ../a/*.lua   ../b/*.lua "),
            vec!["../a/*.lua".to_string(), "../b/*.lua".to_string()]
        );
        assert_eq!(
            shlex_split(r#""with space.lua" plain"#),
            vec!["with space.lua".to_string(), "plain".to_string()]
        );
    }
}
