"""
Runs the Python oracle over the pinned corpus and records everything the three
differential levels need.

Each renderer runs in its own subprocess: luadox's main() is a module-level script
with global state and calls sys.exit(), so running two renderers in one interpreter
would compare a second run against a parser that had already seen the first.

Outputs, under _build/oracle/ by default:

    diagnostics.json   L0 -- every (category, file, line, message), path-normalised
    doc.json           L1 -- the json renderer, i.e. the IR made inspectable
    html/              L2 -- the 591-file rendered site
    luals/             L2 -- the LuaLS definition output
    manifest.sha256    a digest per output file, the thing that is checked in
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import corpus  # noqa: E402
import normalize  # noqa: E402

# The asset bundle's cache-buster is a sha256 over the assets, so it differs by
# construction between two implementations that ship the same bytes differently.
# Both sides normalise it to a fixed token before the L2 compare.
RE_ASSETS_VERSION = re.compile(rb'\?[0-9a-f]{7,64}(?=["\'])')
ASSETS_VERSION_TOKEN = b'?ASSETS_VERSION'


def run_renderer(renderer: str, out: Path, diagnostics: Path | None,
                 quiet: bool) -> tuple[int, float, str]:
    argv = [sys.executable, '-c',
            'import sys; from luadox.main import main; main()',
            '-c', str(corpus.CONFIG),
            '-r', renderer,
            '-o', str(out.resolve())]
    if diagnostics:
        argv += ['--diagnostics-json', str(diagnostics.resolve()),
                 '--diagnostics-root', str(corpus.CONFIG_CWD)]
    env = dict(os.environ, PYTHONPATH=str(corpus.ORACLE_DIR), PYTHONIOENCODING='utf-8')
    started = time.perf_counter()
    proc = subprocess.run(argv, cwd=str(corpus.CONFIG_CWD), env=env,
                          capture_output=True)
    elapsed = time.perf_counter() - started
    log = proc.stderr.decode('utf-8', 'replace')
    if not quiet:
        tail = [line for line in log.splitlines() if ' ERROR ' in line or ' WARN' in line]
        print(f'  {renderer}: exit {proc.returncode} in {elapsed:.1f}s, '
              f'{len(tail)} error/warning lines')
    return proc.returncode, elapsed, log


def digest_tree(root: Path, normalise_assets_version: bool) -> list[str]:
    lines = []
    for path in sorted(root.rglob('*')):
        if not path.is_file():
            continue
        # Line endings are normalised for every output file, not only text-looking ones:
        # everything this tool writes goes through Python's text mode, so the bytes on
        # disk are a property of the host rather than of the tool.
        data = normalize.newlines(path.read_bytes())
        if normalise_assets_version:
            data = RE_ASSETS_VERSION.sub(ASSETS_VERSION_TOKEN, data)
        rel = path.relative_to(root).as_posix()
        lines.append(f'{hashlib.sha256(data).hexdigest()}  {rel}')
    return lines


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--out', type=Path, default=corpus.BUILD_DIR / 'oracle')
    ap.add_argument('--levels', default='0,1,2',
                    help='which levels to produce (default all)')
    ap.add_argument('--allow-dirty', action='store_true',
                    help='record golden data from a tree with uncommitted changes')
    ap.add_argument('--record', action='store_true',
                    help='copy the digest manifest, the diagnostics and the provenance '
                         'into golden/, which is what the repository checks in')
    ap.add_argument('--quiet', action='store_true')
    args = ap.parse_args()

    levels = {int(x) for x in args.levels.split(',') if x.strip()}
    prov = corpus.provenance()
    if (prov.corpus_dirty or prov.oracle_dirty) and not args.allow_dirty:
        raise SystemExit('corpus or oracle has uncommitted changes; commit them or '
                         'pass --allow-dirty (the recording is then not reproducible)')

    # The oracle runs with its cwd inside the corpus, so every path handed to it must
    # be absolute or it lands somewhere in the corpus tree.
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    manifest: list[str] = []
    timings: dict[str, float] = {}

    print(f'oracle {prov.oracle_commit[:12]} over corpus {prov.corpus_commit[:12]}')

    if 0 in levels or 1 in levels:
        # L0 rides along with the json render: diagnostics come out of parsing, which
        # every renderer does identically, so the cheapest carrier wins.
        doc = out / 'doc.json'
        diag = out / 'diagnostics.json' if 0 in levels else None
        code, elapsed, _ = run_renderer('json', doc, diag, args.quiet)
        timings['json'] = elapsed
        if not doc.exists():
            raise SystemExit(f'json renderer produced nothing (exit {code})')
        manifest.append(f'{hashlib.sha256(doc.read_bytes()).hexdigest()}  doc.json')
        if diag:
            payload = normalize.diagnostics(
                json.loads(diag.read_text(encoding='utf-8')), corpus.CORPUS_REPO)
            diag.write_text(json.dumps(payload, indent=2, sort_keys=True) + '\n',
                            encoding='utf-8', newline='\n')
            manifest.append(
                f'{hashlib.sha256(diag.read_bytes()).hexdigest()}  diagnostics.json')
            entries = payload['diagnostics']
            by_cat: dict[str, int] = {}
            for e in entries:
                by_cat[e['category']] = by_cat.get(e['category'], 0) + 1
            print('  diagnostics: ' + (', '.join(f'{v} {k}' for k, v in
                                                 sorted(by_cat.items())) or 'none'))

    if 2 in levels:
        html = out / 'html'
        if html.exists():
            shutil.rmtree(html)
        code, elapsed, _ = run_renderer('html', html, None, args.quiet)
        timings['html'] = elapsed
        if not html.exists():
            raise SystemExit(f'html renderer produced nothing (exit {code})')
        manifest += [f'{h}  html/{p}' for h, _, p in
                     (line.partition('  ') for line in digest_tree(html, True))]

        luals = out / 'luals'
        if luals.exists():
            shutil.rmtree(luals)
        code, elapsed, _ = run_renderer('luals', luals, None, args.quiet)
        timings['luals'] = elapsed
        if luals.exists():
            manifest += [f'{h}  luals/{p}' for h, _, p in
                         (line.partition('  ') for line in digest_tree(luals, False))]

    manifest.sort(key=lambda line: line.split('  ', 1)[1])
    (out / 'manifest.sha256').write_text('\n'.join(manifest) + '\n', encoding='utf-8',
                                         newline='\n')
    (out / 'provenance.json').write_text(json.dumps({
        'corpus_commit': prov.corpus_commit,
        'corpus_dirty': prov.corpus_dirty,
        'oracle_commit': prov.oracle_commit,
        'oracle_dirty': prov.oracle_dirty,
        'levels': sorted(levels),
        'seconds': {k: round(v, 2) for k, v in timings.items()},
    }, indent=2, sort_keys=True) + '\n', encoding='utf-8', newline='\n')
    print(f'  manifest: {len(manifest)} files -> {out / "manifest.sha256"}')

    if args.record:
        # 33 MB of output is not checked in; one digest per file is. A manifest tells
        # you *that* something moved, and the run that produced it tells you what.
        corpus.GOLDEN_DIR.mkdir(parents=True, exist_ok=True)
        for name in ('manifest.sha256', 'diagnostics.json', 'provenance.json'):
            src = out / name
            if src.exists():
                shutil.copyfile(src, corpus.GOLDEN_DIR / name)
        print(f'  recorded -> {corpus.GOLDEN_DIR}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
