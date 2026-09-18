//! Renders markdown fragments with comrak, so the Python's rendering of the same
//! fragments can be compared one by one.
//!
//! Reads a JSON array of strings on the path given as the first argument, writes a JSON
//! array of rendered HTML strings to the second. The options are the ones spec/html.md
//! section 10.3 settles on: raw HTML through, no GFM extensions, no smart punctuation.

use comrak::{markdown_to_html, Options};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: md-probe <in.json> <out.json>";
    let input = args.next().ok_or(usage)?;
    let output = args.next().ok_or(usage)?;

    let fragments: Vec<String> = serde_json::from_str(&std::fs::read_to_string(input)?)?;
    let options = options();
    let rendered: Vec<String> = fragments
        .iter()
        .map(|md| markdown_to_html(md, &options))
        .collect();
    std::fs::write(output, serde_json::to_string(&rendered)?)?;
    Ok(())
}

/// The settings the oracle's `commonmark` run is equivalent to.
pub fn options() -> Options<'static> {
    let mut options = Options::default();
    // The corpus writes `<b>` in a doc comment and the oracle passes it through.
    options.render.r#unsafe = true;
    // 0.49 spells the field `unsafe`, which needs the raw-identifier escape; 0.50+
    // renamed it to `unsafe_`.
    options
}
