# luadox-rs

Rewriting luadox in Rust.

| phase | | state |
|---|---|---|
| 0 | oracle and differential harness | done |
| 1 | parser spike, go/no-go | done -- **full_moon 2.2.0** |
| 2 | IR + json renderer | **L1 green on the corpus and on 16/16 fixtures** |
| 3 | LuaLS renderer | specified (`spec/luals.md`), not started |
| 4 | html renderer | specified (`spec/html.md`), not started |
| 5 | packaging and cutover | not started |

Everything here reads two trees and writes nothing to either:

| input | what it is | pinned at |
|---|---|---|
| `<luadox-fork>` | the Python fork | `origin/luals-all` + `oracle-patches/` |
| `<corpus>` | the production corpus, 580 Lua files | `6fa35c5ee93c912fb7062f976e33afb62151d416` |

## What the acceptance criterion is

A modernisation, not a bug-compatible clone, so the criterion is split in two:

1. **Rendered output must be exact.** html, LuaLS definitions, json. They are the safety
   net proving no content was lost in translation. An unexplained byte difference is a
   failure.
2. **Everything around it is expected to improve** and is not held to parity:
   diagnostics (which fire, their wording, their category), exit codes, error handling,
   and the three known parser defects. Those are **fixed**, not reproduced, and reported
   as a delta.

`harness/differ.py` sorts every difference into exactly three buckets:

```
output diff, unexplained             -> FAILURE, investigate
output diff, named in improvements   -> recorded as an improvement
diagnostics / exit-code difference   -> DELTA, reported, never a failure
```

`harness/improvements.toml` is the improvements list. It has **no wildcard**: an L2 entry
names every file it explains, an L1 entry names the JSON path fragment every difference
it explains must contain. An entry that stops explaining anything is a failure too — a
stale entry hides a regression as well as a missing one does.

## Rebuilding the oracle

The plan says "port `luals-all`". `luals-all` turns out to be **stale relative to the PR
lineage on the same fork** — see *Findings* below — so the oracle is `origin/luals-all`
plus five commits, exported to `oracle-patches/`:

```sh
git clone --no-hardlinks <luadox-fork> oracle
cd oracle && git checkout -b oracle origin/luals-all && git am ../oracle-patches/*.patch
```

The result is `a35e6095e2b8`, recorded in `golden/provenance.json`.

Patch 0006 ports luadox **PR #13** (branch `luals-section-members`), which landed after
Phase 1: a name assigned inside an explicit `@section` is a member of it even with no doc
comment. It changes `doc.json`, so the Rust cannot be graded without it — the 35 members
it restores would otherwise arrive as unexplained L1 differences, and they are not an
improvement of the Rust over the Python, they are the Python's own behaviour moving.
Re-pinning moved 14 of 594 output files and took the diagnostics baseline from 112 to
147 (35 new `undocumented-section-members`, none gone).

## Running it

```sh
cargo build --release                         # the tool
cargo test --release                          # 34 unit tests
cargo clippy --release --all-targets -- -D warnings
cargo fmt --all

python harness/oracle_run.py --levels 0,1,2 --record   # golden run, ~7 s
python harness/candidate_run.py                        # the Rust over the same corpus
python harness/differ.py                               # grade it
python spec/run.py                                     # the 16 fixtures

python harness/dump_decls.py                           # declaration dump
python harness/compare_decls.py                        # parser spike vs oracle
```

`--record` copies the digest manifest, the diagnostics and the provenance into `golden/`.
The 33 MB of rendered output is not checked in: a manifest says *that* something moved,
the run that produced it says what.

## Measurements

All on this machine, rustc 1.83.0 (msvc), Python 3.10.11, corpus `6fa35c5e`.

### Oracle (Phase 0)

| | |
|---|---|
| diagnostics baseline | 71 `snippets` + 41 `undocumented-enum-members` + 35 `undocumented-section-members`, everything else zero, exit 1 |
| declarations found | 4447 — 504 class, 72 module, 237 section, 257 table, 764 function, 2613 field |
| output | 592 files (504 class pages, 72 module pages, 9 top-level, 6 svg, 1 `luadox.lua`), 34 MB |
| run time | json 3.4 s, html 6.3 s, luals 3.0 s; parse alone 0.25 s |
| reproducible | yes — two runs agree on all 592 files, on `doc.json`, and on all 112 diagnostics |

### Parser spike (Phase 1)

| | tree-sitter 0.26.13 + tree-sitter-lua 0.5.0 | full_moon 2.2.0 (fallback) |
|---|---|---|
| builds on the 1.83 pin | yes, after pinning 3 transitive deps down | yes, after pinning 1 |
| cold release build | 16.7 s (deps cached), 469 KB binary | 15.0 s |
| parse 580 files / 31 217 lines | 0.11 s | 0.037 s |
| vs the Python scanner (0.25 s) | 2.3x faster | 6.8x faster |
| C toolchain in the workspace | **yes** | no |
| licence | MIT | MPL-2.0 |
| doc comment attachment | comments are nodes; matched to the next declaration by line | leading trivia on the declaration's own token |
| the 40 `#ifdef ENGINE_DEBUG` / `#endif` lines | 40 localised ERROR nodes | 140 errors, 0 after preprocessing |
| declarations lost to those lines | none | **none** |

Declaration agreement with the oracle, keyed on `(file, line, kind, local symbol)` —
**identical for both parsers**, against oracle `6b6ccf11046a`:

```
python 4340 declarations, rust 4299
  agree            4299 (99.06% of python)
  python only      41      value-only @enum members the oracle synthesizes with no
                           doc comment -- a resolver decision, not a parse
  rust only        0
of 4299 shared declarations: 205 value differences, 0 argument-list differences
```

All 205 value differences are one thing: a value whose expression spans more than one
line, which the line scanner drops and the parser keeps. Named in `improvements.toml`;
it moves `doc.json` and no html page.

## Phase 2: the IR and the json renderer

`luadox -r json` over the pinned corpus, graded by `harness/differ.py`:

```
L1  structured parity (doc.json)
  205 differing JSON paths: 205 explained, 0 not
IMPROVEMENTS (output diffs justified by a named fix)
  * A field value whose expression spans more than one line is no longer dropped.
      doc.json (.value)
DIAGNOSTICS DELTA (never a failure)
  python 147, candidate 387
  now reported: 240 (240 compact-block-content)
OK: rendered output matches, or every difference is a named fix
```

Exit code 1, as the oracle's. **0.20 s** against the oracle's 3.4 s for the same render.

And `python spec/run.py`, the second corpus:

```
16/16 fixtures match
```

### What is covered

Tags, scopes, `@within`, `@order`, name resolution, hierarchy, content assembly,
diagnostics and config -- for both corpora, so all 27 tags including the eleven the the production project
corpus never uses. Config is the Python `ConfigParser` dialect written by hand
(`#`-after-whitespace inline comments, indented-continuation multi-line values). The json
output is written by a hand-rolled ordered-object encoder, because Python dicts are
insertion-ordered and a renderer that sorts its keys produces a document that is equal
but not identical.

### What is not, and is named rather than implied

* The **LuaLS and html renderers** (Phases 3 and 4). `-r luals` and `-r html` report that
  they are not there rather than rendering something else.
* **`require()` crawling.** `follow = true` is an error, not a silent no-op.
* **Encodings other than utf-8**, likewise an error rather than a wrong read.
* **The yaml renderer**, dropped at Tier 2 as the plan says.

### The three design decisions that cost the most to get right

1. **The scan stays line-driven; the facts do not.** luadox semantics *are* line-shaped,
   so `parse.rs` keeps that shape. What changed is where it gets its answers: "is there a
   declaration on this line, and what does it say" is a lookup into a parse tree, not a
   regular expression over raw text. That is what fixes the three lexical defects without
   moving anything else.

2. **Derived names are lazy-once, not eager.** Computing a name when an element is
   registered lost 70 of 72 module pages: an `@enum` table asks its enclosing module for
   a name long before that module is itself registered. `ensure_name` / `ensure_topsym`
   reproduce the Python cached properties, which is the one piece of its laziness that is
   load-bearing rather than incidental.

3. **Cross references resolve in the renderer traversal order**, because that is where
   the Python resolves them -- `Markdown.get()` runs the first time a renderer asks,
   against whatever element it last focused. A field `@{Foo}` therefore resolves relative
   to the collection it is rendered under, not to the field. This was the difference
   between 359 unexplained refid differences and none.

### One bug class made unrepresentable

A `@compact` collection has no detail box, so the html renderer puts a member entire
documentation into the one-line synopsis cell. When that is a code block, a heading or a
list it is emitted but unreadable -- 240 elements over 50 files on this corpus, shipped
and unnoticed for a year because it reads correctly everywhere except in a browser.

The Python can only find it by matching a regular expression against rendered html. Here
it is a type error:

```
markdown::Inline        content a one-line context can lay out; no public constructor
markdown::Block         anything a documentation block can produce
Block::into_inline()    the only route between them, fallible, and its TooBigForARow
                        names the offending block kinds
markdown::RowContent    what every one-line renderer takes; only that conversion builds it
render::row             the one-line renderers, whose parameter is RowContent
```

There is no route by which a code block reaches a table cell. The single conversion site
is `prerender::fit_to_row`, and its `Err` is the `compact-block-content` report. Two
independent implementations agree on what it finds: 240 elements, `h5` 166, `pre` 166,
`ul` 85.

## The parser decision

**full_moon 2.2.0**, on this evidence.

Phase 1's table left one claim unproven: tree-sitter keeps "every declaration around and
inside" the `#ifdef` lines, while full_moon was only observed to keep every *top-level*
declaration. That observation came from `fallback-probe/src/bin/recover.rs`, which walks
`ast.nodes().stmts()` — top level by construction, so it could not have seen anything
else. A lost declaration is a member that silently does not render, so the claim was
measured rather than argued.

`fallback-probe/src/bin/decls.rs` mirrors `spike/src/lua.rs` line for line on full_moon
and emits the same JSON, so `harness/compare_decls.py` grades both against the same
oracle dump. It produces the numbers in the block above, exactly — `agree 4299`,
`rust only 0`, `205 value differences`, `0 argument-list differences`.

It produces them **with and without** the preprocessing step: the two declaration lists
compare equal element for element. full_moon's recovery reaches inside function bodies,
not just to the next top-level statement.

Why full_moon wins on the rest:

* **Doc-comment attachment is structural.** Leading trivia on the declaration's own
  token, rather than a comment node matched to the next declaration by line. The reason
  for this rewrite is to stop deciding semantics with line and text heuristics;
  reintroducing one at a doc generator's single most important attachment point would
  defeat it.
* **Pure Rust.** No C toolchain enters `scripts/rust`, whose header advertises that
  `cargo test` needs only a Rust toolchain. Phase 1 had that cost written down as a thing
  to argue for in review; it no longer has to be argued.
* **6.8x faster than the Python scanner**, against tree-sitter's 2.3x, and one transitive
  pin instead of three.

The cost that stands: **MPL-2.0** rather than MIT, which adds an entry to
`docs/licenses/third-party-licenses.rst`. File-level copyleft on
modified MPL files only, and the dependency is unmodified.

### The preprocessing step

The production Lua sources are C-preprocessed before they reach the interpreter, so 40 lines
across 8 files are `#ifdef ENGINE_DEBUG` / `#endif`. Those are not Lua, and no Lua parser
should be asked to make sense of them — the Python line scanner only survives them
because it never parses anything.

`blank_preprocessor_directives` replaces each directive line with an empty line,
preserving line numbers exactly, because a line number is the identity a declaration and
a diagnostic carry. It takes the corpus from **140 parse errors in 6 files to 0 in 0**.

It buys no declarations. It buys a parse error meaning something: without it, "the parser
reported an error" is 140 lines of noise, and a real syntax error in a hand-written file
would be indistinguishable from it.

### Modules excluded from the comparison, and why

`module` refs are excluded because the corpus contains no `@module` tag at all — all 72
are invented by the resolver when a file turns out to have documented elements. A parser
cannot decide that and should not be graded on it.

## Layout

```
harness/corpus.py         pinned inputs; refuses to record from an unpinned tree
harness/oracle_run.py     runs the oracle at L0/L1/L2, produces the manifest
harness/dump_decls.py     the parser-level property check, Python side
harness/normalize.py      what both sides are allowed to differ on, each rule named
harness/differ.py         the three-bucket classifier
harness/compare_decls.py  grades the Rust spike against the Python declaration dump
harness/candidate_run.py  runs the Rust and normalises it the same way
harness/improvements.toml the improvements list
golden/                   manifest.sha256 (594 lines), diagnostics.json, decls.json
oracle-patches/           the six commits that make the oracle out of origin/luals-all
spec/luals.md             what the LuaLS renderer must emit (Phase 3)
spec/html.md              what the html renderer must emit (Phase 4)
spec/fixtures/            16 fixtures, one source file each, rendered by the oracle
spec/run.py               grades the Rust against them
spike/                    the Phase 1 parser spike (tree-sitter-lua, rustc 1.83)
fallback-probe/           the same corpus through full_moon: decls.rs is the graded
                          comparison that decided the parser

crates/luadox/            the library
  config.rs               the ConfigParser dialect, by hand
  lua.rs                  full_moon: what a parser knows about one file
  tags.rs                 the 27 tags, as an enum
  ir.rs                   the arena: elements, flags, content
  parse.rs                the scan, the registry, resolution and ordering
  content.rs              blocks into content; cross references into links
  markdown.rs             Block / Inline, and the conversion between them
  prerender.rs            what parsing could not decide
  json.rs                 an ordered JSON value, written as Python writes one
  render/json.rs          the json renderer
  render/row.rs           the one-line renderers
crates/luadox-cli/        [[bin]] name = "luadox"
```
