# luadox-rs

Rewriting luadox in Rust.

| phase | | state |
|---|---|---|
| 0 | oracle and differential harness | done |
| 1 | parser spike, go/no-go | done -- **full_moon 2.2.0** |
| 2 | IR + json renderer | **L1 green on the corpus and on 17/17 fixtures** |
| 3 | LuaLS renderer | **L2 byte-identical on the corpus and on 17/17 fixtures** |
| 4 | html renderer | **591/592 L2 files identical; the one difference is named** |
| 5 | packaging and cutover | not started -- the owner's call |

Everything here reads two trees and writes nothing to either:

| input | what it is | pinned at |
|---|---|---|
| `<luadox-fork>` | the Python fork | `origin/luals-all` + `oracle-patches/` |
| `$LUADOX_CORPUS_REPO` (see `harness/corpus.py`) | the production corpus, 580 Lua files | the commit `golden/provenance.json` records |

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
| the 40 `#ifdef` / `#endif` preprocessor lines | 40 localised ERROR nodes | 140 errors, 0 after preprocessing |
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

L2  byte parity (html, luals)
  591/592 files identical

IMPROVEMENTS (output diffs justified by a named fix)
  * A field value whose expression spans more than one line is no longer dropped.
      doc.json (.value)
  * comrak has no CODE_INDENT equivalent, so one indented continuation line becomes
    a code block.
      html/class/DepthTargetPass.html
DIAGNOSTICS DELTA (never a failure)
  python 147, candidate 387
  now reported: 240 (240 compact-block-content)
OK: rendered output matches, or every difference is a named fix
```

Exit code 1, as the oracle's. **0.20 s** against the oracle's 3.4 s for the same render.

And `python spec/run.py`, the second corpus, grading all three renderers:

```
17/17 fixtures match
```

### What is covered

Tags, scopes, `@within`, `@order`, name resolution, hierarchy, content assembly,
diagnostics and config -- for both corpora, so all 27 tags including the eleven the production
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

### Where each diagnostic category comes from

Nine categories, and which run can raise them. A json run cannot show `types`, because
both of its sites are in the LuaLS renderer -- so an "it never fires" reading of a json
run is a reading of the wrong run.

| category | raised by | exercised by |
|---|---|---|
| `snippets` | parse | corpus (71), `code_snippets` |
| `structure` | parse | corpus (0), `within_order`, `code_snippets` |
| `references` | parse, prerender | `xrefs`, `within_order` |
| `untyped` | prerender | `module_globals` |
| `undocumented-enum-members` | `validate_enums` | corpus (41), `enum` |
| `undocumented-section-members` | parse | corpus (35) |
| `conflicts` | parse (4 sites, 2 reachable) | `conflicts` |
| `types` | **the LuaLS renderer** (2 sites) | `types`, via the oracle's luals run |
| `compact-block-content` | prerender | corpus (240) -- Rust only, see below |

Two of the four `conflicts` sites cannot be reached from any source, and are defensive
rather than dead:

* *`reference "X" with the same name already exists`* needs `_add_reference` called twice
  on one object. The only repeat call is the implicit module added through the
  `for scope in reversed(ref.scopes) ... else` fallback, and that fallback requires
  `modref.name not in self.topsyms` -- false once the module is added, and false too in
  the one branch that leaves a module added-but-unregistered. The conditions exclude each
  other.
* *`could not determine which class or module X belongs to`* needs a scope stack with no
  top-level element in it. `scopes[0]` is the file's implicit module on every path:
  `@class` and `@module` rebind to `[scopes[0], ref]`, `@table` appends, and a manual
  section is scoped `[topref]`.

Both are ported anyway. `Category::ALL` is the union of what the two Python branches
declare, and it is printed verbatim when `allow_incomplete` names something unknown, so
dropping a member would change output.

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

## Phase 3: the LuaLS renderer

`luadox -r luals`, implemented against `spec/luals.md` rather than against
`render/luals.py`: reading the Python while porting is how a port inherits a behaviour
without noticing it is one. The spec's provenance and this repository's oracle agree --
`a35e6095e2b882940a7d3987956d0f298160f45b`, not dirty -- and the spec anticipates the
sixth patch, so there is nothing to reconcile.

```
17/17 luals fixtures byte-identical

corpus: luals/luadox.lua, 24532 lines
  oracle    1042134 bytes on disk, sha cd7b62605fc6490f
  candidate 1017602 bytes on disk, sha cd7b62605fc6490f
  identical after newline normalisation: True
```

The 24 532-byte difference on disk is one byte per line. The Python opens its output in
text mode, so on Windows it writes CRLF and its bytes are a property of the host; this
writes LF unconditionally, as `spec/html.md` §11 recommends, and `normalize.newlines` is
the rule both sides are digested through.

**Byte parity is against the pinned oracle, not against the newer renderer on
`origin/pr4-review`.** So: no `---@enum`, no `---@deprecated`, nothing for `@since`, and
every field `= nil` whatever the source assigned. Adopting the newer one is a change to
the *oracle*, and belongs there first.

### What this renderer taught the IR

One Phase 2 defect, which only this renderer could have surfaced. `parse_raw_content`
walks a block's lines followed by a sentinel row `(-1, '', None)`, and that sentinel
appends an **empty line** to the block's last markdown fragment. I had skipped it. The
line is invisible everywhere content is trimmed -- a doc comment, a json value, an
inlined description -- which is why L1 was green without it, and visible in exactly one
place:

```
oracle                                  rust (before)
  ---Declared second, ordered first…      ---Declared second, ordered first…
  ---                                     ---*read-only*
  ---*read-only*
```

It is what separates a field's description from the `*meta*` line this renderer appends
after it.

### Two things about it that are easy to assume wrongly

* **It maps type names; the html renderer does not.** `@treturn void` reads `void` on an
  html page and `nil` in `luadox.lua`. A shared "format a type" helper between the two
  renderers is already wrong.
* **It resolves a cross reference against the element being emitted**, where the json
  renderer resolves against the collection that element is in. The Python's laziness
  makes the difference invisible until two renderers disagree about a bare `@{Foo}`.

### The one deliberate divergence

A diagnostic, recorded in `improvements.toml` because `spec/luals.md` §8.2 asks for it:
the duplicate `untyped` report is emitted once rather than twice. The prerender stage
already reports every parameter with no `@tparam`, and the oracle's renderer reports the
same parameter again under the same category at the same line with a different message.
Across all 17 fixtures it is the only luals-run diagnostic difference in either
direction:

```
rust only  : none
oracle only: {'untyped': 1}
  [untyped] types: parameter "a" of Shape.undocumented has no documented type
```

## Phase 4: the html renderer

`luadox -r html`, against `spec/html.md`. **0.53 s** for the 591-file site, against the
oracle's 3.4 s.

```
591/592 files identical
  * comrak has no CODE_INDENT equivalent, so one indented continuation line becomes
    a code block.
      html/class/DepthTargetPass.html

17/17 fixtures match     (78/78 recorded html files identical)
```

### The markdown library

**comrak `=0.49.0`, with default features off.** 0.50 and newer are edition 2024, which
needs Cargo 1.85 against this workspace's 1.83 pin — the plan's "comrak 0.55" is not
available here. The default features pull `syntect`, and with it `yaml-rust`, `zlib-rs`
and `xdg`, for syntax highlighting the foot template already does in the browser.

### The one difference, measured rather than assumed

`spec/html.md` §10.2 names two divergences from `commonmark.blocks.CODE_INDENT = 1000` and
says the second was never measured, because it "shows up as a *missing* code block, not as
an extra one". `harness/markdown_parity.py` measures both: it wraps the oracle's own
`_markdown_to_html`, records every string it is called with, renders each through both
libraries and compares. 7451 renders, 4962 distinct strings, **one difference**:

```
on DepthTargetPass
markdown:  '     yourself.'
oracle:    <p>yourself.</p>
comrak:    <pre><code> yourself.
           </code></pre>
```

A `@see` continuation indented five spaces, at
`lua/src/autogen/DepthTargetPass.lua:21`. The fix belongs in the
source — the line is mis-indented whatever renders it — and the source is a tree this
project only reads, so it is recorded in `improvements.toml` with the page named.

The **second** half of §10.2 is unexercised, and that is now a measured statement rather
than a hope: the corpus has 42 lines indented four or more spaces starting with `-` and 6
following a blank line, and every one is a paragraph continuation where `indented` decides
nothing. The same harness on constructed input shows all four classes the section names,
plus the leading-pipe table dialect comrak cannot reproduce at all — so a zero here is a
real zero:

```
indented list after blank:    DIFFERS   <p>- one</p>        vs  <pre><code>- one
indented text after blank:    DIFFERS   <p>just prose</p>   vs  <pre><code>just prose
indented heading after blank: DIFFERS   <h1>not really</h1> vs  <pre><code># not really
indented fence after blank:   DIFFERS   language-lua        vs  <pre><code>```lua
leading pipe table:           DIFFERS   <table>             vs  <p>| a | b |
plain paragraph / raw html / fenced code: SAME
```

It is the guard §10.2 asks for, and an exact one rather than the proposed heuristic: it
fails on any difference on a page `improvements.toml` does not name.

### Three defects this renderer surfaced

Only the html path could have found any of them, which is the argument for building a
renderer rather than trusting a green L1.

1. **The search index lost a space.** `_markdown_to_text` ends with
   `re.sub(r'\s+', ' ', text)`, which collapses but does **not** strip, and every content
   block ends with the sentinel's empty line. So a fragment flattens with a trailing
   space, the fragments are joined with a newline, and the index turns that into a second
   space:

   ```
   oracle  text:"Reads a value.  Careful with this The note body continues..."
   rust    text:"Reads a value. Careful with this The note body continues..."
   ```

2. **The order files are read in is a property of the machine.** `glob.glob` does not
   sort; it returns `os.scandir` order, which on NTFS is case-insensitive alphabetical and
   on ext4 is hash order. That order decides the module list in every sidebar, the order
   of the search index and the previous/next chain — so the shipped docs already depend on
   which machine built them. Sorting case-insensitively makes it a property of the input,
   and agrees with the recorded oracle:

   ```
   oracle  FontWeightEnums, gfxEnums, GPUResourceMemoryTypeEnums
   rust    FontWeightEnums, GPUResourceMemoryTypeEnums, … gfxEnums   (byte order)
   ```

   Invisible at L1, because the prerenderer sorts toprefs before the json renderer sees
   them.

3. **`mimetypes.guess_type` is the host's**, seeded from the Windows registry or
   `/etc/mime.types` — §12.9 calls it a real portability hazard, because the same favicon
   can produce a different `type=` on another machine. Replaced with a fixed table.

### The asset bundle

Compiled in: the fourteen files of the Python's `data/`, plus a `sidebar.tmpl.html` that
**no branch of the fork ships**. A run without `project.sidebar_template` dies in the
Python's constructor with `FileNotFoundError`; the production config sets one, so nobody has
noticed. This ships a default, and records it as a deviation.

The `?<version>` cache-buster is a sha256 over the bundle in sorted-path order. The
implementation reproduces the oracle's `658dac8` exactly when run over the oracle's own
files, which is how it was checked; over this bundle it differs, because these files are
stored with LF and carry one more, so the harness normalises it to a token on both sides.

### Two harness bugs it also found

* `normalize.newlines` made **two** lines out of one. A default template on Windows is
  read as bytes from a CRLF checkout and written through text mode, so each of its lines
  ends `\r\r\n` — six lines of `search.tmpl.html`, the only place in the corpus.
  Reducing CRLF and then CR turned that into two newlines and invented six blank lines
  that were in neither output.
* The cache-buster regex wanted **eight** hex digits and the version is seven, so the
  manifest digests were never normalised at all. Harmless while both sides shipped the
  same bundle; the moment they did not, all 579 pages differed on that one token.

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
the consuming project's third-party licence list. File-level copyleft on
modified MPL files only, and the dependency is unmodified.

### The preprocessing step

The corpus's Lua sources are C-preprocessed before they reach the interpreter, so 40 lines
across 8 files are `#ifdef` / `#endif` directives. Those are not Lua, and no Lua parser
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
harness/markdown_parity.py the markdown guard: both libraries over every string the
                          renderer renders
golden/                   manifest.sha256 (594 lines), diagnostics.json, decls.json
oracle-patches/           the six commits that make the oracle out of origin/luals-all
spec/luals.md             what the LuaLS renderer must emit (Phase 3)
spec/html.md              what the html renderer must emit (Phase 4)
spec/fixtures/            17 fixtures, one source file each, rendered by the oracle
spec/run.py               grades the Rust against them, json and luals
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
  assets.rs               the bundle, compiled in, and the cache-buster
  render/json.rs          the json renderer
  render/html.rs          the html renderer
  render/luals.rs         the LuaLS renderer
  render/row.rs           the one-line renderers
crates/luadox-cli/        [[bin]] name = "luadox"
crates/md-probe/          a measurement tool: comrak over a list of strings
```
