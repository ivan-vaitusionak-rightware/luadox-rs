# Renderer fixtures

Sixteen minimal Lua (and one Markdown) inputs, each written to exercise one rule from
[`../luals.md`](../luals.md) or [`../html.md`](../html.md), with the expected output for
it checked in beside it. `python spec/run.py` renders every fixture and compares.

`expected/` was recorded from the Python luadox. A change that alters it is a behaviour
change, and the updated files go in with the change that causes it.

## Layout

```
src/<name>.lua            the input
src/manual.md             the one manual-page input (used by the `manual` fixture)
snippets/hello.lua        the snippet @example lua hello.lua resolves to
sidebar.tmpl.html         supplies the sidebar template every fixture needs (see below)
expected/<name>/
    luadox.lua                the luals renderer's whole output
    html/class/*.html         the class pages
    html/module/*.html        the module pages
    html/index.html           the landing page, or the manual index where there is one
    html/search.html          the search page
    html/index.js             the search index
    diagnostics-luals.json    every diagnostic of the luals run
    diagnostics-html.json     every diagnostic of the html run
    exit.json                 each run's exit code
expected/provenance.json  the recording commit, the fixture list, the asset digests
```

## Configuration

Each fixture is rendered twice, into a scratch directory, from a config that
`../fixture_setup.py` generates:

```ini
[project]
name = LuaDox Fixture
title = Fixture
files = <abs>/src/<name>.lua
follow = false
encoding = utf8
snippet_path = <abs>/snippets
sidebar_template = <abs>/sidebar.tmpl.html
```

`sidebar_template` is not optional. `render/html.py` asks the asset bundle for
`sidebar.tmpl.html` when the config does not name one, and **no such file exists in
`luadox/data/`** on any branch of the fork — the html renderer only runs with a
configured sidebar template. That is the latent break the plan names in §0; the fixture
config works around it the same way the production config does.

Two fixtures need more than that, and `../fixture_setup.py` holds the extra lines:

- `manual` adds `[manual] index = <abs>/src/manual.md`.
- `luals_config` adds a `[luals]` section with `globals`, `mixin_suffix` and
  `mixin_doc_phrase`.

## What is and is not checked in

- The **eleven static asset files** (`luadox.css`, `prism.css`, `prism.js`,
  `js-search.min.js`, `search.js`, and the six SVGs under `img/`) are byte-identical for
  every fixture, so they are recorded as name + sha256 in `provenance.json` instead of
  being copied sixteen times.
- The `?<assets_version>` cache-buster **is** kept verbatim. Its value for the pinned
  oracle is `658dac8`. A candidate implementation that ships the same assets encoded
  differently will differ on it, so normalise `?<hex>` to `?ASSETS_VERSION` on both sides
  before comparing, as `../normalize.py` does.
- **Line endings are reduced to LF on copy.** The oracle opens its output files in text
  mode, so every `\n` it writes becomes `os.linesep`; the three default templates are read
  as *bytes* and keep whatever the git checkout gave them, which with `core.autocrlf=true`
  is CRLF. The two compose: on Windows a template line lands as `\r\r\n` and a generated
  line as `\r\n`. The raw bytes are therefore a property of the host and the checkout, not
  of the renderer, and cannot be a fixture. See *Line endings* in `../html.md`.

## The fixtures

| fixture | what it pins down |
|---|---|
| `class_basic` | the shape of a class page and of a `---@class` block: `@tparam`/`@treturn` ordering, multiple returns, the `# ` before a return description, `---@return any` for an undocumented return, first-sentence-vs-body splitting, and that the synopsis drops the sentence's final period |
| `nested_table` | `@table` inside a class and inside another table: tables are emitted **flat and unqualified** (`Colors`, not `Palette.Colors`), nested tables follow their parent's members rather than nesting, and `table_level` tracking closes a table at its `}` |
| `compact` | `@compact fields` and bare `@compact`: which headings the html drops, the permalink inside the name cell, the `deprecated` marker, the full (not first-sentence) cell text — and that the luals renderer ignores `@compact` entirely |
| `inherits_multi` | several `@inherits` on one class, comma-separated and repeated: `---@class Derived : Middle, Mixin, Missing` emits an unresolvable parent verbatim while html's *Inherits* block drops it, and the linear hierarchy follows only the first parent |
| `admonitions` | `@note` with a title and a multi-paragraph body, `@warning` with none: indentation-based nesting, `**Title**` in luals, the single-line `<div class="admonition …">` in html |
| `code_snippets` | a hand-written fence, an inline `@example` block, a resolvable `@example lua hello.lua`, and a missing one: the `##### Example` heading, the blank line the fence open and close leave behind, `MISSING SNIPPET:` inside the fence, and the `-----` that a snippet's own `--` comment becomes under the `---` prefix |
| `xrefs` | `@{ref}`, `@{ref\|text}`, backtick refs, forward references, and the unresolvable form of each: what luals strips them to, what html links them to, which ones raise a `references` diagnostic (only `@{}`), and that an unresolvable `@see` is dropped silently but still emits an empty `<div class="see">` |
| `enum` | `@enum` with documented, undocumented, hex and non-integer members: html prints values, **luals emits `---@class` and `= nil`, not `---@enum`**, and the two `structure` / `undocumented-enum-members` diagnostics |
| `deprecated_since` | `@deprecated` with and without an explanation and `@since` on a class, a field and a method: the prerendered *Deprecated* admonition leads the content in both renderers, html adds `<span class="tag since">`, and **luals emits neither `---@deprecated` nor any since marker** |
| `module_globals` | an implicit module: members are emitted as globals, an unqualified field uses its bare symbol while `log.level` keeps its path, and the `-- Namespace tables …` stub block declares `log` |
| `explicit_module` | `@module gfx`: a backing table with the module's doc comment and **no `---@class`**, members qualified under it |
| `naming` | `@scope`, `@rename`, `@alias`, `@display` on a class — and that a declaration's owner is **lexical**: `function Shell.make()` written after `@class Aliased` is emitted inside `Aliased` |
| `types` | `TYPE_MAP` (`bool`, `int`, `float`, `double`, `void`), a resolved class name, an unresolved name, a C++ `::` name, a `\|` union, and a parameter with no `@tparam` — plus the two `types` diagnostics that only the luals run raises and the `untyped` one that it raises **twice** |
| `within_order` | `@section`, `@within`, `@order last`, `@fullnames`, `@display`, `@meta`: which section a member lands in, the resulting order, and the `<td class="meta">` padding |
| `luals_config` | the `[luals]` config: the injected-globals block and its position, `mixin_suffix`, and `mixin_doc_phrase` turning prose into extra `---@class` parents |
| `manual` | a manual page: `index` at the output root, `h1`–`h3` becoming sections and `h4` not, the symbol-from-heading rule and its duplicate suffix, a fence that contains a `#`, tags without a comment prefix, and that luals skips manuals entirely |

## Reading a fixture

The inputs are deliberately dull; the interesting part is always the output. When a rule
in either spec cites a fixture, it cites the file and the lines, so the quickest way to
check a claim is to open `expected/<name>/luadox.lua` or the html page next to it.
