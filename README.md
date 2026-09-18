# luadox-rs

Rewriting luadox in Rust. Phase 0 (oracle and differential harness) and Phase 1 (parser
spike, go/no-go) are done; Phase 2 (IR + json renderer) is in progress.

**The parser is full_moon 2.2.0.** See *The parser decision* below.

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
python harness/oracle_run.py --levels 0,1,2 --record   # golden run, ~13 s
python harness/dump_decls.py                           # declaration dump
python harness/differ.py _build/oracle _build/candidate
python harness/compare_decls.py                        # parser spike vs oracle

cd spike && cargo test --release && cargo clippy --release --all-targets -- -D warnings
./target/release/luadox-spike.exe <corpus>/lua/src ../_build/spike/decls.json
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
harness/improvements.toml the improvements list
golden/                   manifest.sha256 (594 lines), diagnostics.json, decls.json
oracle-patches/           the six commits that make the oracle out of origin/luals-all
spike/                    the Phase 1 parser spike (tree-sitter-lua, rustc 1.83)
fallback-probe/           the same corpus through full_moon: decls.rs is the graded
                          comparison that decided the parser
```
