"""
Renders every markdown string the html renderer hands to its markdown library, through
both libraries, and reports where they disagree.

spec/html.md section 10.2 names two divergences from `commonmark.blocks.CODE_INDENT =
1000` and says the second was never measured, because it "shows up as a *missing* code
block, not as an extra one" -- a diff of rendered pages shows a changed page without
saying why. This measures it directly, string by string.

    python harness/markdown_parity.py
    python harness/markdown_parity.py --show 20

**The strings come from the renderer, not from `doc.json`.** An earlier version of this
script read the markdown values out of `doc.json` and reported zero differences, which was
wrong: the json renderer writes `md.get().strip()`, so every fragment had already lost the
leading whitespace that the whole question is about. The one real divergence in the corpus
is a `@see` continuation indented five spaces, and stripping made it invisible. Here the
oracle's own `_markdown_to_html` is wrapped and its argument recorded, so what is compared
is exactly what it was given.

This is also the guard section 10.2 asks for, and an exact one rather than the heuristic it
proposed: it fails when the corpus grows markdown the two renderers disagree on, whatever
the construct -- including the leading-pipe table dialect, which comrak cannot reproduce at
all. A difference on a page harness/improvements.toml already names is reported and does
not fail.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import corpus  # noqa: E402

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.10 ships tomli instead
    import tomli as tomllib

PROBE = corpus.ROOT / 'target' / 'release' / (
    'md-probe.exe' if sys.platform == 'win32' else 'md-probe')


def collect_from_oracle() -> list[tuple[str, str]]:
    """
    Runs the oracle's html renderer over the pinned corpus with `_markdown_to_html`
    wrapped, and returns every (page, markdown) it was called with.
    """
    corpus.ensure_oracle_importable()
    from luadox.render.html import HTMLRenderer  # noqa: E402

    seen: list[tuple[str, str]] = []
    original = HTMLRenderer._markdown_to_html

    def spy(self, md):
        ref = self.ctx.ref
        seen.append((getattr(ref, 'name', '?') if ref else '?', md))
        return original(self, md)

    HTMLRenderer._markdown_to_html = spy
    here = os.getcwd()
    saved = sys.argv
    try:
        with tempfile.TemporaryDirectory() as tmpdir:
            os.chdir(corpus.CONFIG_CWD)
            from luadox.main import main  # noqa: E402
            sys.argv = ['luadox', '-c', str(corpus.CONFIG), '-r', 'html', '-o', tmpdir]
            try:
                main()
            except SystemExit:
                pass
    finally:
        sys.argv = saved
        os.chdir(here)
        HTMLRenderer._markdown_to_html = original
    return seen


def render_python(texts: list[str]) -> list[str]:
    """The oracle's own markdown renderer, imported rather than reimplemented."""
    corpus.ensure_oracle_importable()
    import commonmark  # noqa: E402
    import commonmark_extensions.tables  # noqa: E402

    # The one global the oracle sets at import time, and half of what section 10.2 is
    # about.
    commonmark.blocks.CODE_INDENT = 1000
    parser = commonmark_extensions.tables.ParserWithTables()
    renderer = commonmark_extensions.tables.RendererWithTables()
    return [renderer.render(parser.parse(text)) for text in texts]


def render_rust(texts: list[str]) -> list[str]:
    if not PROBE.exists():
        raise SystemExit(f'no probe at {PROBE}; cargo build --release -p md-probe')
    with tempfile.TemporaryDirectory() as tmpdir:
        src = Path(tmpdir) / 'in.json'
        dst = Path(tmpdir) / 'out.json'
        src.write_text(json.dumps(texts), encoding='utf-8')
        proc = subprocess.run([str(PROBE), str(src), str(dst)], capture_output=True)
        if not dst.exists():
            sys.stderr.write(proc.stderr.decode('utf-8', 'replace'))
            raise SystemExit('the probe produced nothing')
        return json.loads(dst.read_text(encoding='utf-8'))


def classify(a: str, b: str) -> str:
    """What kind of divergence this is, named rather than counted."""
    if '<table' in a or '<table' in b:
        return 'the leading-pipe table dialect, which comrak cannot reproduce'
    if '<pre>' in b and '<pre>' not in a:
        return 'a block indented four spaces became an indented code block'
    if '<pre>' in a and '<pre>' not in b:
        return 'an indented code block stopped being one'
    return 'other'


def allowed_pages() -> set[str]:
    """The pages an L2 improvements entry already names."""
    path = Path(__file__).resolve().parent / 'improvements.toml'
    if not path.exists():
        return set()
    entries = tomllib.loads(path.read_text(encoding='utf-8')).get('improvement', [])
    return {Path(name).stem
            for entry in entries if entry.get('level') == 'L2'
            for name in entry.get('paths') or []}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--show', type=int, default=4)
    args = ap.parse_args()

    found = collect_from_oracle()
    texts = [text for _, text in found]
    # The same string is rendered on many pages; render each distinct one once.
    distinct = list(dict.fromkeys(texts))
    print(f'{len(found)} markdown renders, {len(distinct)} distinct strings')

    by_text = dict(zip(distinct, zip(render_python(distinct), render_rust(distinct))))
    known = allowed_pages()

    kinds: collections.Counter[str] = collections.Counter()
    examples: dict[str, list[tuple[str, str, str, str]]] = collections.defaultdict(list)
    unexplained = 0
    explained = 0
    for page, text in found:
        a, b = by_text[text]
        if a == b:
            continue
        if page in known:
            explained += 1
            continue
        unexplained += 1
        kind = classify(a, b)
        kinds[kind] += 1
        if len(examples[kind]) < args.show:
            examples[kind].append((page, text, a, b))

    print(f'{explained} differ on a page improvements.toml names')
    print(f'{unexplained} differ and are not named')
    for kind, n in kinds.most_common():
        print(f'   {n:5}  {kind}')
    for kind, rows in examples.items():
        print()
        print(f'=== {kind} ===')
        for page, text, a, b in rows[:args.show]:
            print(f'  on {page}')
            print('  markdown:')
            for line in text.split('\n')[:6]:
                print(f'    {line!r}')
            print('  oracle:')
            for line in a.strip().split('\n')[:6]:
                print(f'    {line}')
            print('  comrak:')
            for line in b.strip().split('\n')[:6]:
                print(f'    {line}')

    print()
    if unexplained:
        print('FAILED: markdown the two renderers disagree on, on a page '
              'improvements.toml does not name')
        return 1
    print('OK: every markdown difference is on a page improvements.toml names')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
