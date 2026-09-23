# The HTML renderer — executable specification

What `luadox -r html` writes, rule by rule, so Phase 4 can be implemented against this
document instead of against `render/html.py`.

**Authority.** The oracle is the clone at `<repo>/oracle` — `origin/luals-all` plus
`oracle-patches/`. Every rule below was read out of `luadox/render/html.py` (843 lines)
at that commit **and** confirmed against a fixture under `spec/fixtures/` or against the
recorded corpus run in `_build/oracle/html`, except where marked *unverified*. Where this
document and the oracle disagree, the oracle wins.

**Scope.** The renderer only: the IR it is handed is the Phase 2 contract. §12 lists what
could not be pinned down.

---

## 1. What is written

`render(toprefs, outdir)` writes, in this order:

1. **Copied config files** — `project.css`, `project.js`, `project.favicon`, each split by
   `files_str_to_list` (shlex per line) and copied flat into `outdir` with `shutil.copy`.
   A path that does not exist is **skipped with `log.critical('%s file "%s" does not
   exist, skipping', …)` and the run continues** — but the `<link>`/`<script>` tag for it
   is still emitted on every page. The shipped production docs have a dangling
   `custom-styles-lua.css` for exactly this reason.
2. **One page per topref**, in the order the prerenderer sorted them — `(type, symbol)`,
   so all `class` pages, then `manual`, then `module`. A topref with
   `userdata['empty'] and implicit` is skipped (§12.2). The path is
   `<outdir>/<ref.type>/<ref.name>.html`, except a `ManualRef` named `index`, which goes
   to `<outdir>/index.html`. Directories are created as needed.
3. **`index.js`** — the search index (§9).
4. **`search.html`** — the search page (§8).
5. **`index.html`** — the generated landing page, **only when no manual page is named
   `index`** (§8).
6. **The eleven asset files**, byte-for-byte from the bundle (§4).

`outdir` empty or `None` becomes `out` after `log.warn('"out" is not defined in config
file, assuming ./out/')`.

### The corpus taxonomy

```
504  class/<Name>.html          one per @class
 72  module/<name>.html         one per module that reached topsyms
  9  top level                  index.html, search.html, index.js,
                                luadox.css, prism.css, prism.js,
                                js-search.min.js, search.js, favicon96x96.png
  6  img/i-{left,right,download,github,gitlab,bitbucket}.svg
```

591 files. `index.html` is the corpus's manual page (`[manual] index = …`), not the
generated landing page, and `favicon96x96.png` is the one copied config file that exists.

---

## 2. The page frame

Every page — class, module, manual, search, landing — is built by the same context
manager, `_render_html(topref, lines)`. The frame is:

```
<head template>
<div class="topbar">…</div>
<div class="sidebar">…</div>
<div class="body">
    … page-specific content …
</div>
<foot template>
```

Each `out(...)` call appends one element of a list that is finally joined with `'\n'`, so
**an empty string yields an empty line**. Several rules below exist only because of that:
`out(self._since(colref))` emits a blank line whenever the element has no `@since`.

### HT-2.1 Titles and the body class

```python
project_title = config['project'].get('title', config['project'].get('name', 'Lua Project'))
page_title    = topref.collections[0].heading   # ManualRef
              = topref.display                  # everything else
html_title    = f'{page_title} - {project_title}'
bodyclass     = f"{topref.type or 'other'}-{re.sub(r'\W+', '', topref.name).lower()}"
```

A `ManualRef` with no collections is fatal: `log.critical('manual "%s" has no sections
(empty doc or possible symbol collision)')` and `sys.exit(1)`.

> `class_basic`: `<title>Widget - Fixture</title>`, `<body class="class-widget">`.
> `manual`: `<title>Manual Title - Fixture</title>`, `<body class="manual-index">`.
> The search and landing pages: `<title>Search - Fixture</title>`,
> `<body class="other-search">` — the pseudo-ref's type is the empty string and its name
> is `--search`, which the `\W+` strip reduces to `search`.

### HT-2.2 The `{head}` fragment

Built in this order and joined with `'\n'`:

```html
<link href="{root}{css basename}?{version}" rel="stylesheet" />      per project.css entry
<script src="{root}{js basename}?{version}"></script>               per project.js entry
<link rel="shortcut icon" {type} href="{root}{favicon basename}?{version}"/>
```

Only the basename is used — the files are copied flat to the doc root. The favicon's
`{type}` is `' type="<mimetype>"'` from `mimetypes.guess_type`, and empty when the type is
unknown; because the format string already has a space before it, a known type produces
**two** spaces (`<link rel="shortcut icon"  type="image/png" …`).

### HT-2.3 The topbar

```html
<div class="topbar">
<div class="group one">
<div class="button description"><a href="{path}index.html"><span>{hometext}</span></a></div>
<!-- or, when there is no [manual] index: -->
<div class="description"><span>{hometext}</span></div>
</div>
<div class="group two">
{user links}
</div>
<div class="group three">
{prev}{next}
</div>
</div>
```

- The button form is chosen by `config.has_section('manual') and config.get('manual',
  'index', fallback=False)`. `path` is `''` when the current topref *is* the manual index
  and `'../'` otherwise.
- `hometext` is **`project.name`**, falling back to the project title. The `--hometext`
  command-line option writes `project.hometext`, which nothing reads: the option is dead.
- **User links** are every config section whose name starts with `link`, **sorted by
  section name**:
  ```html
  <div class="button{ iconleft}"><a href="{url}" title="{tooltip}">{img}<span>{text}</span></a></div>
  ```
  `icon` is either a path or one of the four shorthands `download`, `github`, `gitlab`,
  `bitbucket`, which expand to `{root}img/i-<name>.svg?<version>`; `{root}` is substituted
  in both `icon` and `url`. With an icon the `iconleft` class is added and `img` is an
  `<img src="…" alt=""/>`. `text` has **no fallback**, so a `[link…]` section without one
  raises `configparser.NoOptionError` and the run dies with a traceback. (*Unverified* —
  no fixture and no corpus use.)
- **Prev/next** walk `manual + classes + modules` where `manual` is
  `parser.topsyms` filtered to `ManualRef` in insertion order, `classes` is the `ClassRef`
  toprefs **sorted by name**, and `modules` is the `ModuleRef` toprefs in insertion order.
  The loop stops one past the current topref. On the search and landing pages the very
  first entry counts as the match, so they get a *Next* to the second entry and no
  *Previous*.
  ```html
  <div class="button iconleft"><a href="{href}" title="{name}"><img src="{root}img/i-left.svg?{version}" alt=""/><span>Previous</span></a></div>
  <div class="button iconright"><a href="{href}" title="{name}"><span>Next</span><img src="{root}img/i-right.svg?{version}" alt=""/></a></div>
  ```

### HT-2.4 The sidebar

```html
<div class="sidebar">
{sidebar template}
<form action="{root}search.html">
<input class="search" name="q" type="search" placeholder="Search" />
</form>
{Contents}{Manual}{Classes}{Modules}
</div>
```

Each of the four lists is emitted only when it has entries, in this order, and each has
the shape `<div class="{cls}"><div class="heading">{Title}</div><ul>…</ul></div>`:

| block | entries | link text |
|---|---|---|
| `sections` / *Contents* | `topref.collections`, skipping any `ManualRef` | `Class <code>{heading}</code>` / `Module <code>{heading}</code>` for a class or module collection, `colref.heading` otherwise — **raw, not markdown-rendered** |
| `manual` / *Manual* | `parser.parsed[ManualRef]`, skipping any with a scope | `ref.heading` |
| `classes` / *Classes* | `ClassRef` toprefs sorted by name | `ref.display` |
| `modules` / *Modules* | `ModuleRef` toprefs in insertion order, skipping empty implicit ones | `ref.name` |

Contents entries link to `#{colref.symbol}`; the other three use `_get_ref_href`. An entry
whose `ref.name` equals the current topref's gets `class="selected"`.

---

## 3. Templates

Four, all substituted with `str.format`, so **a literal `{` or `}` in a custom template
raises**. They are read from the config path when one is given and from the asset bundle
otherwise:

| key | config option | asset | placeholders |
|---|---|---|---|
| head | `project.head_template` | `head.tmpl.html` | `{version} {title} {head} {root} {bodyclass}` |
| foot | `project.foot_template` | `foot.tmpl.html` | `{root} {version}` |
| search | `project.search_template` | `search.tmpl.html` | `{root} {version}` |
| sidebar | `project.sidebar_template` | `sidebar.tmpl.html` | `{root} {version}` |

**`sidebar.tmpl.html` does not exist** in `luadox/data/` on any branch of the fork. A run
without `project.sidebar_template` therefore dies in the renderer's constructor with
`FileNotFoundError`. The production config sets it, so nobody has noticed; the fixtures set it too. Port
the fix, not the bug: ship a default sidebar template.

The three that do exist are, verbatim:

```html
<!-- head.tmpl.html -->
<!DOCTYPE html>
<html lang="en">
<!-- Documentation generated by LuaDox: https://github.com/jtackaberry/luadox -->
<head>
    <meta http-equiv="Content-Type" content="text/html; charset=UTF-8"/>
    <title>{title}</title>
    <link href="{root}prism.css?{version}" rel="stylesheet" />
    <link rel="stylesheet" href="{root}luadox.css?{version}" type="text/css">
    {head}
</head>
<body class="{bodyclass}">
```
```html
<!-- foot.tmpl.html (no trailing newline) -->
<script src="{root}prism.js?{version}"></script>
</body>
</html>
```
```html
<!-- search.tmpl.html (no trailing newline, and no closing </div> for #results) -->
<div id="template" class="result"><div class="title"><span></span><a href=""></a></div><div class="text"></div></div>
<div id="results">
<noscript>⛔ Javascript is required for search functionality.</noscript>
<div class="summary"></div>
<script src="{root}js-search.min.js?{version}"></script>
<script src="{root}index.js?{version}"></script>
<script src="{root}search.js?{version}"></script>
```

**Trailing newlines matter.** `head.tmpl.html` ends with one and `foot.tmpl.html` and
`search.tmpl.html` do not, so the join in §2 puts a blank line after `<body class="…">`
and none before `</body>`. A custom template that differs here changes every page.

A configured template is read with `open(path, 'r', encoding=<project.encoding or
locale.getpreferredencoding()>)` — **text mode**, so its line endings are normalised to
`\n`. A default template is read as **bytes** and decoded, so its line endings survive as
they are on disk. §11.

---

## 4. The asset bundle

Eleven files are copied verbatim into the output, from `luadox/data/`:

| output path | source | bytes |
|---|---|---|
| `luadox.css` | `data/luadox.css` | 13530 |
| `prism.css` | `data/prism.css` | 1983 |
| `prism.js` | `data/prism.js` | 8007 |
| `js-search.min.js` | `data/js-search.min.js` | 7760 |
| `search.js` | `data/search.js` | 1907 |
| `img/i-left.svg` | `data/img/i-left.svg` | 365 |
| `img/i-right.svg` | `data/img/i-right.svg` | 366 |
| `img/i-download.svg` | `data/img/i-download.svg` | 336 |
| `img/i-github.svg` | `data/img/i-github.svg` | 1042 |
| `img/i-gitlab.svg` | `data/img/i-gitlab.svg` | 1020 |
| `img/i-bitbucket.svg` | `data/img/i-bitbucket.svg` | 543 |

They are written with `open(outfile, 'wb')` — binary, so no newline translation. Their
sha256 digests at the pinned oracle are in `spec/fixtures/expected/provenance.json`.

`prism.css` and `prism.js` are a Prism build (syntax highlighting, applied by the foot
template to `<pre><code class="language-…">`); `js-search.min.js` is the js-search
library; `search.js` is luadox's own search page driver; the six SVGs are the topbar
icons. `luadox.css` is luadox's own stylesheet and is the one the oracle patches touched.

### HT-4.1 The cache-buster

`self._assets_version = assets.hash()[:7]`, where `hash()` is

```python
h = sha256()
for f in sorted(self.files):     # every non-directory under data/, path-relative
    h.update(self.get(f))
h.hexdigest()
```

so it covers **all fourteen** files in `data/` — the eleven copied assets *and* the three
templates — concatenated in sorted-path order, with no separators and no names. The paths
it sorts carry the host separator (`img\i-left.svg` on Windows), which happens not to
change the order for this bundle. The value is `658dac8` at the pinned oracle, and it
appears as `?<version>` on every asset URL in the templates and in the generated head,
topbar and search markup.

Two implementations that ship the same assets will still differ here if either re-encodes
a byte, so the harness normalises `?<hex>` to `?ASSETS_VERSION` on both sides. A port
should compute the value the same way over the same bundle; parity on the *value* is not
required, parity on *where it appears* is.

---

## 5. Paths, anchors and ids

### HT-5.1 The root prefix

```python
def _get_root_path():
    viatopref = self.ctx.ref.topref
    return '' if (isinstance(viatopref, ManualRef) and viatopref.name == 'index') \
                 or viatopref.symbol == '--search' else '../'
```

It depends on the **current context ref**, which the renderer mutates as it walks, not on
the ref being linked. Every page except the manual `index`, the search page and the
landing page therefore sits one directory down and prefixes `../`.

### HT-5.2 A link to a reference

```python
topsym   = ref.userdata.get('within_topsym') or ref.topsym
topref   = parser.refs[topsym]                     # KeyError → re-raised, run dies
prefix   = _get_root_path()
if not isinstance(ref.topref, ManualRef) or ref.topref.name != 'index':
    prefix += topref.type + '/'
if isinstance(ref.topref, ManualRef) and ref.symbol:
    fragment = '#' + ref.symbol if ref.scopes else ''
else:
    fragment = '#' + ref.name if ref.name != ref.topsym else ''
href = prefix + topsym + '.html' + fragment
```

Note the asymmetry, which is easy to lose in a port: the *directory* comes from the
`@within`-redirected `topref.type`, the *manual/index test* from the ref's own
`ref.topref`, and the *file name* from the redirected `topsym`.

A link to a topref itself has no fragment. A link from a page to its own content still
carries the full relative path (`../class/Link.html#Link.target`), never a bare `#…`.

> `xrefs`: every link on `class/Link.html` starts `../class/Link.html`.
> `manual`: links on `index.html` are `class/Doc.html#Doc.read` — no `../`.

### HT-5.3 Anchors

| element | id | where |
|---|---|---|
| a collection (class, module, section, table) | `colref.symbol` | `<h2 … id=…>` |
| a field or function | `ref.name` | `<dt id=…>`, and `<var id=…>` in a compact row |
| a manual section | `secref.symbol` | `<h{level} id=…>` |

A manual section's symbol is derived from its heading: lower-cased, every character
outside `[a-zA-Z0-9- ]` removed, runs of spaces replaced by `_`, then `_-_` replaced by
`-`. A repeated symbol gets the running count appended (`a_sub-section`, then
`a_sub-section2`).

Every anchor is followed by

```html
<a class="permalink" href="#{id}" title="Permalink to this definition">¶</a>
```

---

## 6. A class or module page

For each collection of the topref, in `topref.collections` order:

```html
<div class="section">
<h2 class="{colref.type}" id="{colref.symbol}">{heading}
{since}
{permalink}
</h2>
<div class="inner">
    {hierarchy}{inherits}{content}
    {synopsis}
    {fields}
    {functions}
</div>
</div>
```

`{since}` is `<span class="tag since">since {version}</span>` or the empty string, which
still costs a line. `{heading}` is:

- `'{Type} <code>{colref.heading}</code>'` when the collection is the class or module
  itself — the heading being its own symbol;
- `_markdown_to_html(colref.heading)` with every `<p>` and `</p>` removed otherwise. The
  markdown renderer's trailing newline survives the strip, so a section or table heading
  produces **one extra blank line** before the `since`/permalink lines.

> `compact`: `<h2 class="section" id="Flags">Flags` followed by two blank lines;
> `class_basic`: `<h2 class="class" id="Widget">Class <code>Widget</code>` followed by
> one.

### HT-6.1 Hierarchy and parents (classes only)

When `len(colref.hierarchy) > 1` — the chain built by following the **first** `@inherits`
parent upward, stopping at an unresolved parent or a cycle:

```html
<div class="hierarchy">
<div class="heading">Class Hierarchy</div>
<ul>
<li class="class">{prefix}<span>{html}</span></li>
…
</ul>
</div>
```

The list runs root-first. For entry `n` (0-based), `prefix` is `''` when `n == 0` and
`'&nbsp;' * ((n - 1) * 6) + '&nbsp;└─ '` otherwise. The entry for the class being rendered
carries the extra class `self` and is its plain name; every other entry is
`_types_to_html([cls.name])`, i.e. `<em><a href="…">Name</a></em>`.

When the class has **more than one** resolvable direct parent, an additional block lists
them all — the hierarchy above shows only the first:

```html
<div class="inherits">
<div class="heading">Inherits</div>
<div>{comma-separated _types_to_html of each parent}</div>
</div>
```

`parents` resolves each `@inherits` name and **skips the ones that do not resolve**, so
this block can disagree with the LuaLS renderer, which emits unresolved parents verbatim.

> `inherits_multi`: the hierarchy is `Base`, `Middle`, `Derived`; *Inherits* lists
> `Middle, Mixin`; `luadox.lua` says `Derived : Middle, Mixin, Missing`.

### HT-6.2 Column bookkeeping

Computed once per collection, before anything is emitted:

```python
fields_title         = 'Attributes' if any(isinstance(f.scope, ClassRef)) else 'Fields'
fields_meta_columns  = max(1 if f.meta else 0)          # 0 or 1
fields_has_type_col  = any(f.types)
functions_title      = 'Methods' if any(isinstance(f.scope, ClassRef) and ':' in f.symbol) else 'Functions'
functions_meta_cols  = max(1 if f.flags.get('meta') else 0)
fields_compact       = 'fields' in colref.compact
functions_compact    = 'functions' in colref.compact
```

`@compact` with no arguments sets both. Note `functions_title` needs *both* a class scope
and a colon in the symbol, so `Config.reload` inside a class is still a *Function*.

### HT-6.3 The synopsis

Emitted when the collection has any field or function.

```html
<div class="synopsis">
<h3>Synopsis</h3>                                    <!-- only when not fields_compact -->
<div class="heading">{fields_title}</div>            <!-- only when functions exist or not fields_compact -->
<table class="fields {compact|}">…</table>           <!-- only when fields exist -->
<div class="heading">{functions_title}</div>         <!-- only when fields exist or not functions_compact -->
<table class="functions {compact|}">…</table>        <!-- only when functions exist -->
</div>
```

The class attribute is `'fields {}'.format('compact' if fields_compact else '')`, so a
non-compact table is `class="fields "` **with a trailing space**.

The `<h3>Synopsis</h3>` test looks only at `fields_compact`, even when the collection has
no fields at all: `@compact functions` alone still prints *Synopsis*. (*Unverified* — the
fixtures cover `@compact fields` and bare `@compact`.)

A field row:

```html
<tr>
<!-- not compact -->
<td class="name"><a href="#{name}"><var>{title}</var></a>{enum value}</td>
<!-- compact -->
<td class="name"><var id="{name}">{title}</var>{enum value}{deprecated}{permalink}</td>

<td class="meta types">{types}</td>     <!-- when the field has @type -->
<td class="meta"></td>                  <!-- else, when any field in the collection has one -->
<td class="meta">{markdown(meta)}</td>  <!-- when the field has @meta -->
<td class="meta"></td>                  <!-- padding to fields_meta_columns -->
<td class="doc">{doc}</td>              <!-- omitted entirely when empty -->
</tr>
```

A function row is the same shape with `<td class="doc">` **always** emitted, the name cell

```html
<td class="name"><a href="#{name}"><var>{display}</var></a>()</td>
<td class="name"><var id="{name}">{display}</var>({params}){deprecated}{permalink}</td>
```

where `display` is `ref.display_compact` when the function's scope is a class (the topsym
prefix stripped, `@display` winning if set) and `ref.title` otherwise, and `params` is
`', '.join('<em>{}</em>'.format(p))`. The function meta cell prints `ref.meta` **raw**,
without markdown rendering — unlike the field one.

`{enum value}` is `' = <span class="value">{ref.value}</span>'` when the collection has
the `enum` flag and the field captured a literal, and empty otherwise. `{deprecated}` is
`<span class="tag deprecated">deprecated</span>` and appears **only in compact rows**.

The doc cell is:

- not compact — `_markdown_to_html(ref.content.get_first_sentence(skip_leading=True))`.
  The first sentence stops *before* its final period, so a synopsis line ends without one;
  `skip_leading` steps over leading admonitions so a `@deprecated` box cannot become the
  summary.
- compact — the **whole** content, minus a leading `Admonition` when the ref is
  deprecated (the marker carries that signal).

> `enum`: `<var>Trap</var> = <span class="value">0</span>`, and the undocumented member's
> row has no `<td class="doc">` at all.
> `compact`: the trace row carries the marker and the permalink inside the name cell.
> `within_order`: `<td class="meta"><p>read-only</p>` in the table, `<span class="tag
> meta">read-only</span>` in the detail list.

### HT-6.4 The field detail list

Emitted when the collection has fields and they are not compact:

```html
<h3 class="fields">{fields_title}</h3>        <!-- only when the collection also has functions -->
<dl class="fields">
<dt id="{name}">
<span class="icon"></span><var>{ref.display}</var>{enum value}
<span class="tag type">{types}</span>         <!-- when @type -->
<span class="tag meta">{ref.meta}</span>      <!-- when @meta, raw -->
{since}
{permalink}
</dt>
<dd>
{content}
</dd>
…
</dl>
```

### HT-6.5 The function detail list

```html
<h3 class="functions">{functions_title}</h3>  <!-- only when the collection also has fields -->
<dl class="functions">
<dt id="{name}">
<span class="icon"></span><var>{ref.display}</var>({params})
<span class="tag meta">{ref.meta}</span>
{since}
{permalink}
</dt>
<dd>
{content}
<div class="heading">Parameters</div>         <!-- only when any parameter has a type or a description -->
<table class="parameters">
<tr>
<td class="name"><var>{param}</var></td>
<td class="types">({types})</td>
<td class="doc">{doc}</td>
</tr>
</table>
<div class="heading">Return Values</div>      <!-- only when @treturn exists -->
<table class="returns">
<tr>
<td class="name">{n}.</td>                    <!-- only when there is more than one return -->
<td class="types">({types})</td>
<td class="doc">{doc}</td>
</tr>
</table>
</dd>
…
</dl>
```

`{ref.display}` is the full symbol here (`Widget:move`), not the compact form. A
parameter with no type renders as `<td class="types">()</td>` (*unverified*: no fixture
mixes a documented and an undocumented parameter in one function).

### HT-6.6 Types

`_types_to_html(types)` resolves each name with `parser.resolve_ref` and wraps it:

- resolved → `<em><a href="{href}">{name}</a></em>`, the name printed **as written**, not
  as the ref's name;
- not resolved → `<em>{name}</em>`;
- one entry → that entry; two or more → `', '.join(all but last) + ' or ' + last`;
- no entries → the empty string.

**No `TYPE_MAP` is applied.** `@treturn void` prints `void` here and `nil` in
`luadox.lua`; `bool` stays `bool`. A shared "format a type" helper between the two
renderers is a bug.

---

## 7. A manual page

```html
<div class="manual">
{content}                                     <!-- the preamble, when the page has one -->
<h{level} id="{symbol}">{heading}
{permalink}
</h{level}>
{content}
…
</div>
```

`level` is the markdown heading level, 1–3; `h4` and deeper never become sections and are
left inside the markdown. There is no `since` line here, so a manual heading is followed
by exactly one line before the permalink, where a class heading is followed by two.

`heading` is emitted **raw** — it has been through `refs_to_markdown`, so a `@{ref}` in a
heading becomes a literal `[text](luadox:…)` in the HTML rather than a link. (*Unverified*
— no fixture puts a reference in a heading, and the corpus's one manual page has none.)

> `manual`: `<h2 id="a_sub-section">A Sub-Section` + permalink + `</h2>`; the duplicate
> heading becomes `a_sub-section2`; the `#` inside a fence stays in the code block.

---

## 8. The search page and the landing page

Both are the frame of §2 around a pseudo-reference the renderer registers in its
constructor:

```python
ref = TopRef(parser.refs, file='search.html', symbol='--search')
ref.flags['display'] = 'Search'
parser.refs['--search'] = ref
```

It is **not** in `topsyms`, so it never appears in a sidebar list; its `type` is the empty
string, which is what makes the body class `other-search`.

- **`search.html`** — the frame with the search template as its body.
- **`index.html`** — the same frame with an **empty** body, written only when no manual
  page is named `index`. Its title and body class are the search page's, because it reuses
  the same pseudo-ref to get the root path right.

> `class_basic/html/index.html` is the frame around a bare
> `<div class="body">` / `</div>` pair, and `manual/html/index.html` is the manual page
> instead.

---

## 9. The search index

`index.js` is written with no trailing newline:

```js
var docs = [
{path:"…", type:"…", title:"…", text:"…"},
…
];
```

One entry per reference, iterating `ClassRef, ModuleRef, FieldRef, FunctionRef,
SectionRef` in that order and, within each, `parser.parsed[typ]` in **declaration order** —
which is not the page order. `TableRef` and `ManualRef` are not indexed.

Per entry:

- `path` = `_get_ref_href(ref)` with the context set to the search pseudo-ref, so every
  path is relative to the doc root.
- `type` = the class's `type` attribute: `class`, `module`, `field`, `function`,
  `section`.
- `title` = `ref.display`, except:
  - a **`SectionRef` whose topref is not a manual**: leading admonitions are peeled off,
    the first sentence of the remaining text is taken, and if it is **shorter than 80
    characters** it becomes the title while the body text becomes the peeled admonitions
    plus the rest. Otherwise the display name stands.
  - a **`ModuleRef`**: `title.split('.', 1)[-1]` — everything up to the first dot is
    dropped.
- `text` = `_content_to_text(ref.content)`.
- Both `title` and `text` then have `"` replaced by `\"` and newlines by spaces. Nothing
  else is escaped — a backslash in the source passes through unescaped and would corrupt
  the file (*unverified*: the corpus has none).

`_content_to_text` joins with `'\n'` and strips: a `Markdown` fragment contributes
`_markdown_to_text(md)`, an `Admonition` contributes its title *and* its body as two
entries, and a `SeeAlso` contributes nothing. Because an empty admonition body still
contributes an empty entry, a bare `@deprecated` yields **two** spaces in the flattened
text.

`_markdown_to_text` applies these substitutions in order:

| pattern | replacement |
|---|---|
| ```` ```.*?``` ```` (dotall) | removed |
| `` `([^`]+)` `` | `\1` |
| `#+` | removed |
| `\*([^*]+)\*` | `\1` |
| `!?\[([^]]*)\]\([^)]+\)` | `\1` |
| `@{[^\|]+\|([^}]+)\}` | `\1` |
| `@{([^}]+)\}` | `\1` |
| `\s+` | a single space |

> `manual`: a manual section's title is its **symbol** (`a_sub-section`), because the
> `SectionRef` heuristic is skipped for manual toprefs and `display` is the symbol.
> `within_order`: a non-manual section's title is its first sentence.

---

## 10. Markdown

### HT-10.1 What the oracle runs

`commonmark` 0.9.1 (CommonMark 0.29, per the project's own README — the installed
metadata does not record a spec version) plus `commonmarkextensions` 0.0.6, both
unpinned-lower-bound in `requirements.txt` and both the last release of their project.
Rendering is

```python
parser = commonmark_extensions.tables.ParserWithTables()
ast    = parser.parse(md)
html   = CustomRendererWithTables(self).render(ast)
```

with no options: raw HTML passes through unescaped, no smart punctuation, and a fenced
block renders as `<pre><code class="language-lua">`.

Three local modifications:

1. **`commonmark.blocks.CODE_INDENT = 1000`**, at import time, globally.
2. **`TableWaitingForBug3`** replaces `Table.continue_` to work around
   GovReady/CommonMark-py-Extensions#3.
3. **`CustomRendererWithTables`** overrides `make_table_node` to emit
   `<table class="user">` and `link` to rewrite a `luadox:<id>` destination into a real
   href via `_get_ref_href` just before the link is written.

### HT-10.2 `CODE_INDENT = 1000` is broader than "no indented code"

`CODE_INDENT` also drives `parser.indented = self.indent >= CODE_INDENT`, which is
consulted by **eight** block starts and one continuation. With the constant at 1000,
`indented` is false for every realistic line, so:

- indented code blocks never start — the intended effect;
- and a **block quote, ATX heading, fenced code block, HTML block, setext heading,
  thematic break or list item starts at any indentation**, where stock CommonMark would
  have made it indented code. So does a block quote's continuation.

comrak has no equivalent knob — `comrak::options::Parse` has nothing about indentation —
so both halves are divergences, not one. The measured cost of the first half on the production
corpus is **one page of 578** (`class/DepthTargetPass.html`, a `@see`
continuation indented five spaces after a blank line). The second half was never measured
and is not visible in a diff of the same kind: it shows up as a *missing* code block, not
as an extra one.

What to do: fix the source for the first, and put a harness guard on the second — a
corpus line indented four or more spaces whose first non-space character is one of
`> # \` ~ - _ * + <` or a digit is a candidate divergence. Neither is worth a markdown
pre-pass unless the count grows.

### HT-10.3 Constructs where comrak 0.55 and the oracle could differ

Each of these is a place to check deliberately, not a known break:

| construct | the oracle | comrak |
|---|---|---|
| indented code, and blocks starting indented | §10.2 | stock CommonMark |
| leading-pipe tables | the `commonmark_extensions` dialect: a block *starts* at a line whose first non-space character is `\|`, continues while lines start with `\|` or are non-empty and do not start with `>` or a backtick, `=` separators switch on multi-line mode, and the output is `<table class="user">` | GFM tables: a header row plus a `---\|---` delimiter row, `<table>` with no class. **Not reproducible.** Zero uses in the corpus; guard for a leading-pipe line and refuse |
| raw HTML | passed through | needs `render.unsafe_ = true` |
| fence info string | `<pre><code class="language-lua">` | the same with `github_pre_lang = false` (the default) |
| autolinks, strikethrough, task lists, footnotes | not implemented | GFM extensions, **off by default** — leave them off |
| smart quotes/dashes | off | `parse.smart` off by default — leave it off |
| CommonMark version | 0.29 | 0.31; the deltas are in link reference definitions, HTML block termination and a handful of emphasis edge cases |
| lazy continuation | commonmark.py's own implementation, and `parser.indented` is always false (§10.2), so a continuation line can be indented arbitrarily and still continue the paragraph | stock: a continuation indented four or more spaces after a blank line starts an indented code block |
| a `\n`-terminated block | every block renderer ends with `'\n'`, which the caller sometimes strips (`_content_to_html` strips an admonition body, `_markdown_to_html(colref.heading)` does not) | comrak also ends with `\n`; the strips are the renderer's, not the library's |

### HT-10.4 Content fragments to HTML

`_content_to_html(content)` joins the fragments with `'\n'`:

| fragment | HTML |
|---|---|
| `Markdown` | `_markdown_to_html(md.get())` |
| `Admonition` | one line: `<div class="admonition {type}"><div class="title">{title}</div>{body}</div>`, where `body` is `<div class="body">{inner}\n</div>` with `inner` the recursively rendered content **stripped**, or the empty string when that is empty |
| `SeeAlso` | `<div class="see">See also {links}</div>`, where `links` is the refs rendered as markdown links, run through the markdown renderer, `.strip()`ed and then sliced `[3:-4]` to remove the `<p>`/`</p>` |
| anything else | `ValueError` |

The `SeeAlso` slice is unconditional, so an **empty** `@see` — every reference of it
unresolvable — produces `<div class="see">See also </div>` rather than nothing. The LuaLS
renderer drops it instead.

`Admonition.type` is `note`, `warning` or `deprecated`; the title is
`refs_to_markdown(tag.title or tag.type.title())` from the parser, so it is markdown that
is **not** rendered here — it is interpolated raw into the `<div class="title">`.

Every content block ends with an empty line: `parse_raw_content` walks the block's lines
followed by a sentinel row, which appends an empty line to the block's last `Markdown`
fragment, or adds an empty `Markdown` fragment when the block ended with a tag. So almost
every `<dd>` and `<div class="inner">` ends with a blank line — including one whose
element has no doc comment at all, whose `Content` is a single empty `Markdown` and is
therefore *truthy*. It is not noise to be tidied away; it is in the bytes, and it is why
§12.2 is dead code.

> `admonitions`: the note's two paragraphs and the warning are three single lines of HTML.
> `deprecated_since`: a bare `@deprecated` is
> `<div class="admonition deprecated"><div class="title">Deprecated</div></div>` with no
> body div.

---

## 11. Line endings and encodings

- Pages, `index.js` and the LuaLS file are written with `open(…, 'w', encoding='utf8')` —
  **text mode**, so every `\n` becomes `os.linesep`: LF on Linux, CRLF on Windows.
- A **configured** template is read in text mode, so its own endings are normalised to
  `\n` first and come out as one `os.linesep`.
- A **default** template is read as bytes and decoded, so its endings survive as they are
  in the checkout. With `core.autocrlf=true` — the setting on this machine — they are
  CRLF, and the write then turns the `\n` of each `\r\n` into `\r\n`, producing `\r\r\n`.

So the oracle's exact bytes are a function of the host *and* the git checkout, and the
corpus L2 manifest bakes that in. A port should write `\n` unconditionally and compare
after normalising, which is what `spec/fixtures/expected/` stores. Input files are read
with `project.encoding`, defaulting to `locale.getpreferredencoding()`; output is always
UTF-8.

> Windows, default templates: `<!DOCTYPE html>\r\r\n`.
> Windows, the production config's templates: `<!DOCTYPE html>\r\n`.

---

## 12. Unspecified, or specified only by reading the code

1. **`[link*]` sections.** No fixture, no corpus use. The `text` option has no fallback
   and will raise.
2. **The empty-implicit-module skip.** `userdata['empty']` is `not has_content`, and
   `has_content` is true when any collection has content — but the prerender sentinel
   gives every collection a non-empty `Content`, and a module that reached `topsyms` always
   has itself as a collection. The corpus renders 72 module pages for 72 module toprefs, so
   the branch never fired there either. Keep it; do not rely on it.
3. **A reference in a manual heading** (§7) is emitted as raw markdown. Unverified.
4. **A parameter with no type inside an otherwise-documented function** (§6.5) renders
   `()`. Unverified.
5. **`@compact functions` alone** and its effect on the *Synopsis* heading (§6.3).
   Unverified.
6. **Backslashes and non-ASCII in the search index** (§9). The corpus is pure ASCII;
   `"` is the only character escaped.
7. **`@order` with an anchor** (`@order before X` / `@order after X`) changes the order
   both renderers see. It is an IR rule, not a renderer rule, and no fixture exercises it —
   only `@order last` is covered.
8. **What `search.js` and `js-search.min.js` expect of `index.js`.** The index format is
   reproduced byte-for-byte here without understanding; if a port changes it, nothing in
   this spec will catch it.
9. **`mimetypes.guess_type`** is the host's, seeded from the Windows registry or
   `/etc/mime.types`. A favicon with an unusual extension can produce a different `type=`
   attribute on a different machine. Unverified, and a real portability hazard.
