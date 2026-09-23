//! One test per defect of the Python scanner that this parser is meant to remove.
//!
//! The Python fork has no test suite anywhere, which is how these survived. Each test
//! below names the Python's behaviour so a future reader knows the divergence is a fix
//! and not an accident, and so nobody can reintroduce the line-based design without a
//! red test.

// Test-only, matching the workspace convention in scripts/rust/crates/*/tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use luadox_spike::lua::{Kind, LuaParser};

/// (line, kind, symbol, value, args) -- the whole of what a parser decides.
type Decl = (usize, Kind, String, Option<String>, Option<Vec<String>>);

fn decls(source: &str) -> Vec<Decl> {
    let mut parser = LuaParser::new().expect("grammar loads");
    let (decls, _) = parser.declarations("t.lua", source).expect("parses");
    decls
        .into_iter()
        .map(|d| (d.line, d.kind, d.symbol, d.value, d.args))
        .collect()
}

fn stats(source: &str) -> luadox_spike::lua::FileStats {
    let mut parser = LuaParser::new().expect("grammar loads");
    let (_, stats) = parser.declarations("t.lua", source).expect("parses");
    stats
}

/// Defect (b): a `--[[ ]]` block is not a comment to the line scanner, so a `---` line
/// inside one declares a function that does not exist. Python emits `L.ghost` here.
#[test]
fn a_function_commented_out_in_a_long_comment_is_not_declared() {
    let source = r#"
local L = {}
--[[
--- Not a real function.
function L:ghost() end
]]
--- A real one.
function L:real() end
"#;
    let found = decls(source);
    assert!(
        !found.iter().any(|d| d.2 == "L:ghost"),
        "a function inside a long comment must not be declared, got {found:?}"
    );
    assert!(
        found.iter().any(|d| d.2 == "L:real"),
        "the real function must still be declared, got {found:?}"
    );
}

/// Defect (c): `strip_trailing_comment` is `re.sub(r'--.*', '', line)`, so a `--` inside
/// a string literal truncates the line and the value is lost. Python records `None`.
#[test]
fn a_double_dash_inside_a_string_is_not_a_comment() {
    let source = r#"
--- The separator.
S.sep = "a--b"
"#;
    let found = decls(source);
    let sep = found.iter().find(|d| d.2 == "S.sep").expect("S.sep found");
    assert_eq!(sep.1, Kind::Field);
    assert_eq!(sep.3.as_deref(), Some("\"a--b\""));
}

/// Defect (a): `RE_BLOCK_OPEN` matches `for` and `do` on the same line, so a body scan
/// counting block openings against `end`s never returns to depth zero and leaks into the
/// next function. The defect was only ever reachable from the `@treturn` body scan (now
/// closed), but the *cause* -- deciding block structure by counting keywords on a line --
/// is what must not come back. A parser knows where a function ends; assert that it does.
#[test]
fn a_for_do_loop_does_not_extend_a_function_past_its_end() {
    let source = r#"
--- Loops but returns nothing.
function T:loop(items)
    for i = 1, #items do local x = i end
end
--- Counts.
function T:count() return 42 end
"#;
    let mut parser = LuaParser::new().expect("grammar loads");
    let tree = parser.parse(source).expect("parses");
    let root = tree.root_node();
    let mut cursor = root.walk();
    let functions: Vec<_> = root
        .named_children(&mut cursor)
        .filter(|n| n.kind() == "function_declaration")
        .collect();
    assert_eq!(functions.len(), 2, "two functions, not one run together");
    let loop_fn = functions[0];
    // Lines are 1-based; `function T:loop` opens on 3 and its `end` closes on 5.
    assert_eq!(loop_fn.start_position().row + 1, 3);
    assert_eq!(loop_fn.end_position().row + 1, 5);

    let found = decls(source);
    assert_eq!(
        found
            .iter()
            .map(|d| (d.0, d.2.as_str()))
            .collect::<Vec<_>>(),
        vec![(3, "T:loop"), (7, "T:count")],
        "each doc block attaches to its own function"
    );
}

/// The line scanner gives up on a value whose expression continues past the line and
/// records `None`. The parser knows where the expression ends, so the value survives.
#[test]
fn a_value_spanning_several_lines_is_recovered() {
    let source = r#"
--- A message type.
M.Activated = MessageType:find(
    "Concept.Activated",
    M.Arguments
)
"#;
    let found = decls(source);
    let m = found
        .iter()
        .find(|d| d.2 == "M.Activated")
        .expect("M.Activated found");
    assert_eq!(
        m.3.as_deref(),
        Some("MessageType:find( \"Concept.Activated\", M.Arguments )")
    );
}

/// The corpus's Lua sources are preprocessed, so `#ifdef` lines reach the parser. They are
/// not Lua; tree-sitter localises them to an ERROR node and parses the rest, which is
/// the error-recovery property the choice of parser was made for.
#[test]
fn preprocessor_directives_do_not_stop_the_parse() {
    let source = r#"
--- Before.
function K:before() end
#ifdef DEBUG_BUILD
--- Guarded.
function K:guarded() end
#endif
--- After.
function K:after() end
"#;
    let s = stats(source);
    assert_eq!(s.error_nodes, 2, "one ERROR per directive");
    let found = decls(source);
    assert_eq!(
        found.iter().map(|d| d.2.as_str()).collect::<Vec<_>>(),
        vec!["K:before", "K:guarded", "K:after"],
        "every declaration around and inside the guard is still found"
    );
}

/// A long comment must not join a `---` run: the Python has no notion of one, which is
/// the same root cause as defect (b).
#[test]
fn a_long_comment_terminates_a_doc_block() {
    let source = r#"
--- Documented.
--[[ not part of the block ]]
function K:thing() end
"#;
    let s = stats(source);
    assert_eq!(s.doc_blocks, 1);
    // The block ends at line 2, the long comment occupies line 3, so nothing attaches.
    assert_eq!(s.attached, 0);
    assert_eq!(s.unattached, 1);
}
