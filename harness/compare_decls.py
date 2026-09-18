"""
Grades the Rust parser spike against the Python oracle's declaration dump.

The comparison key is (file, line, kind, local symbol). Names, scopes and `@within`
are a resolver's job, not a parser's, so they are deliberately out of scope here:
this answers "does the parser see the same declarations in the same places", and
nothing else.

Two kinds of record are excluded from the key set, each for a stated reason:
  * `module` -- every module ref in this corpus is implicit (there are no `@module`
    tags), so it is invented by the resolver, not found by the parser;
  * fields with no doc block -- luadox synthesizes an `@enum` member from a bare
    assignment, which again is a resolver decision about an enclosing collection.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import corpus  # noqa: E402

EXCLUDED_KINDS = {'module', 'manual'}


def local(symbol: str | None) -> str:
    """The last component of a dotted/colon-qualified symbol."""
    if not symbol:
        return ''
    return symbol.replace(':', '.').rsplit('.', 1)[-1]


def key(rec: dict) -> tuple:
    return (rec['file'], rec['line'], rec['kind'], local(rec['symbol']))


def load_python(path: Path) -> dict[tuple, dict]:
    records = json.loads(path.read_text(encoding='utf-8'))['declarations']
    return {key(r): r for r in records if r['kind'] not in EXCLUDED_KINDS}


def load_rust(path: Path) -> tuple[dict[tuple, dict], dict]:
    payload = json.loads(path.read_text(encoding='utf-8'))
    # The Rust reports a corpus-root-relative path; the Python reports one relative to
    # the config's working directory. One prefix reconciles them.
    prefix = corpus.relpath(corpus.CORPUS_REPO) + '/'
    out = {}
    for r in payload['declarations']:
        r = dict(r, file=prefix + r['file'])
        out[key(r)] = r
    return out, payload


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--python', type=Path,
                    default=corpus.BUILD_DIR / 'oracle' / 'decls.json')
    ap.add_argument('--rust', type=Path, default=corpus.BUILD_DIR / 'spike' / 'decls.json')
    ap.add_argument('--show', type=int, default=12)
    args = ap.parse_args()

    py = load_python(args.python)
    rs, payload = load_rust(args.rust)

    only_py = sorted(set(py) - set(rs))
    only_rs = sorted(set(rs) - set(py))
    both = sorted(set(py) & set(rs))

    print(f'python {len(py)} declarations, rust {len(rs)}')
    print(f'  agree            {len(both)} ({100 * len(both) / max(len(py), 1):.2f}% of python)')
    print(f'  python only      {len(only_py)}')
    print(f'  rust only        {len(only_rs)}')

    for label, keys in (('python only', only_py), ('rust only', only_rs)):
        if not keys:
            continue
        kinds = Counter(k[2] for k in keys)
        print(f'\n{label} by kind: ' + ', '.join(f'{v} {k}' for k, v in kinds.most_common()))
        for k in keys[:args.show]:
            src = py.get(k) or rs.get(k)
            print(f'   {k[0]}:{k[1]} {k[2]} {k[3]}  {src.get("value") or src.get("args") or ""}')
        if len(keys) > args.show:
            print(f'   ... {len(keys) - args.show} more')

    # Where both found the declaration, do they agree on what it says?
    value_diffs = [(k, py[k].get('value'), rs[k].get('value'))
                   for k in both
                   if (py[k].get('value') or None) != (rs[k].get('value') or None)]
    arg_diffs = [(k, py[k].get('args'), rs[k].get('args'))
                 for k in both
                 if py[k].get('args') is not None and rs[k].get('args') is not None
                 and py[k]['args'] != rs[k]['args']]
    print(f'\nof {len(both)} shared declarations: {len(value_diffs)} value differences, '
          f'{len(arg_diffs)} argument-list differences')
    for k, a, b in value_diffs[:args.show]:
        print(f'   {k[0]}:{k[1]} {k[3]}\n      python {a!r}\n      rust   {b!r}')
    if len(value_diffs) > args.show:
        print(f'   ... {len(value_diffs) - args.show} more')
    for k, a, b in arg_diffs[:args.show]:
        print(f'   {k[0]}:{k[1]} {k[3]}\n      python {a!r}\n      rust   {b!r}')

    errs = payload.get('files_with_errors') or []
    if errs:
        total = sum(len(e['errors']) for e in errs)
        print(f'\ntree-sitter could not fit {total} constructs, in {len(errs)} files:')
        for entry in errs:
            kinds = Counter((snippet.split() or [''])[0]
                            for _, _, snippet in entry['errors'])
            print(f'   {entry["file"]}: {len(entry["errors"])} '
                  + ', '.join(f'{v}x {k}' for k, v in kinds.most_common()))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
