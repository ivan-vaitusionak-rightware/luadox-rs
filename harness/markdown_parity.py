"""
Renders every markdown fragment of the corpus through both markdown libraries and reports
where they disagree.

spec/html.md section 10.2 names two divergences from `commonmark.blocks.CODE_INDENT =
1000` and says the second was never measured, because it "shows up as a *missing* code
block, not as an extra one" -- a diff of rendered pages would show it as a changed page
without saying why.  This measures it directly, fragment by fragment, before any of the
html renderer is written against an assumption.

    python harness/markdown_parity.py                # every fragment in _build/oracle/doc.json
    python harness/markdown_parity.py --show 20

The fragments come from `doc.json`, which is the markdown the renderer is handed *after*
resolution -- the same strings `_markdown_to_html` receives.  Links keep their
`luadox:<id>` destinations on both sides, so a difference here is a markdown difference
and nothing else.
"""

from __future__ import annotations

import argparse
import collections
import json
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import corpus  # noqa: E402

PROBE = corpus.ROOT / 'target' / 'release' / (
    'md-probe.exe' if sys.platform == 'win32' else 'md-probe')


def fragments(doc: dict) -> list[tuple[str, str]]:
    """Every markdown value in the document, with the JSON path it sits at."""
    found: list[tuple[str, str]] = []

    def walk(node, path: str) -> None:
        if isinstance(node, dict):
            if node.get('type') == 'markdown' and node.get('value'):
                found.append((path, node['value']))
            for key, value in node.items():
                walk(value, f'{path}.{key}')
        elif isinstance(node, list):
            for i, value in enumerate(node):
                walk(value, f'{path}[{i}]')

    walk(doc, '$')
    return found


def render_python(texts: list[str]) -> list[str]:
    """The oracle's own renderer, imported rather than reimplemented."""
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


def classify(md: str, a: str, b: str) -> str:
    """What kind of divergence this is, named rather than counted."""
    if '<ul>' in a and '<ul>' not in b and '<pre>' in b:
        return 'indented list item became an indented code block'
    if '<pre>' in b and '<pre>' not in a:
        return 'an indented block became an indented code block'
    if '<pre>' in a and '<pre>' not in b:
        return 'an indented code block stopped being one'
    if '<table' in a or '<table' in b:
        return 'table dialect'
    return 'other'


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--doc', type=Path, default=corpus.BUILD_DIR / 'oracle' / 'doc.json')
    ap.add_argument('--show', type=int, default=4)
    args = ap.parse_args()

    doc = json.loads(args.doc.read_text(encoding='utf-8'))
    found = fragments(doc)
    texts = [text for _, text in found]
    # The same fragment appears on many pages; render each distinct one once.
    distinct = list(dict.fromkeys(texts))
    print(f'{len(found)} markdown fragments, {len(distinct)} distinct')

    left = render_python(distinct)
    right = render_rust(distinct)
    by_text = {text: (a, b) for text, a, b in zip(distinct, left, right)}

    kinds: collections.Counter[str] = collections.Counter()
    examples: dict[str, list[tuple[str, str, str, str]]] = collections.defaultdict(list)
    differing_paths = 0
    differing_texts = set()
    for path, text in found:
        a, b = by_text[text]
        if a == b:
            continue
        differing_paths += 1
        differing_texts.add(text)
        kind = classify(text, a, b)
        kinds[kind] += 1
        if len(examples[kind]) < args.show:
            examples[kind].append((path, text, a, b))

    print(f'{differing_paths} fragment occurrences differ, '
          f'{len(differing_texts)} distinct')
    for kind, n in kinds.most_common():
        print(f'   {n:5}  {kind}')
    for kind, rows in examples.items():
        print(f'\n=== {kind} ===')
        for path, text, a, b in rows[:args.show]:
            print(f'  {path}')
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

    # This is the guard section 10.2 proposes, and an exact one rather than the heuristic
    # it suggested: a divergence is a divergence between the two renderers, not a line
    # that looks like it might cause one.  It covers the leading-pipe table dialect too,
    # which is the other construct comrak cannot reproduce.
    if differing_paths:
        print()
        print('FAILED: the corpus now contains markdown the two renderers disagree on')
        return 1
    print()
    print('OK: both renderers agree on every markdown fragment in the corpus')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
