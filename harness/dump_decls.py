"""
The parser-level property check: every declaration the oracle finds, as a flat list of
(file, line, kind, symbol, name) records.

This is the level that separates "the parser sees the same things" from "the renderer
writes the same bytes".  It is also what the Rust parser spike is graded against, so it
deliberately records only what a parser can know -- no resolution, no content, no
rendering.

`value` and `args` come along because they are the two places the oracle's lexical
defects show up: a `--` inside a string truncates a value, and a multi-line signature
is reassembled by the same regex scan.
"""

from __future__ import annotations

import argparse
import io
import json
import sys
import time
from configparser import ConfigParser
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import corpus  # noqa: E402

corpus.ensure_oracle_importable()

from luadox.parse import Parser  # noqa: E402
from luadox.reference import (ClassRef, FieldRef, FunctionRef, ManualRef,  # noqa: E402
                              ModuleRef, SectionRef, TableRef)

KINDS = [ModuleRef, ClassRef, SectionRef, TableRef, FunctionRef, FieldRef, ManualRef]


def record(ref) -> dict:
    out = {
        'file': corpus.relpath(ref.file) if ref.file else None,
        'line': ref.line,
        'kind': ref.type,
        'symbol': ref.symbol,
        'name': ref.name,
    }
    value = getattr(ref, 'value', None)
    if value is not None:
        out['value'] = value
    extra = getattr(ref, 'extra', None)
    if isinstance(ref, FunctionRef) and extra is not None:
        out['args'] = list(extra)
    if ref.flags.get('enum'):
        out['enum'] = True
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--out', type=Path,
                    default=corpus.BUILD_DIR / 'oracle' / 'decls.json')
    args = ap.parse_args()

    config = ConfigParser(inline_comment_prefixes='#')
    config.add_section('project')
    config.add_section('manual')
    # Accept the corpus's own allowance so the dump is not noisier than a real run.
    config.set('project', 'allow_incomplete', 'undocumented-enum-members')

    parser = Parser(config)
    files = corpus.corpus_files()
    started = time.perf_counter()
    for path in files:
        with open(path, encoding='utf-8') as f:
            parser.parse_source(f)
    elapsed = time.perf_counter() - started

    records = []
    for kind in KINDS:
        records.extend(record(ref) for ref in parser.parsed[kind])
    records.sort(key=lambda r: (r['file'] or '', r['line'] or 0, r['kind'],
                                r['symbol'] or ''))

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps({'files': len(files), 'declarations': records},
                                   indent=1, sort_keys=True, ensure_ascii=True) + '\n',
                        encoding='utf-8', newline='\n')
    by_kind: dict[str, int] = {}
    for r in records:
        by_kind[r['kind']] = by_kind.get(r['kind'], 0) + 1
    print(f'{len(files)} files parsed in {elapsed:.2f}s -> {len(records)} declarations')
    print('  ' + ', '.join(f'{v} {k}' for k, v in sorted(by_kind.items())))
    print(f'  -> {args.out}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
