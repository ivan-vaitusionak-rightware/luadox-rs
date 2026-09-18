"""
Runs the Rust luadox over the pinned corpus and records what the differ compares.

The counterpart to oracle_run.py, and it applies the same normalisations to the same
places: a run that is normalised differently from the one it is compared against is not
a comparison.

    python harness/candidate_run.py              # build, run, normalise
    python harness/candidate_run.py --no-build   # use the binary that is already there
    python harness/differ.py _build/oracle _build/candidate

Outputs, under _build/candidate/ by default:

    doc.json           L1 -- the json renderer, i.e. the IR made inspectable
    diagnostics.json   L0 -- every (category, file, line, message), path-normalised
    provenance.json    what produced it

L2 (html, luals) is not produced: those renderers are Phase 3 and Phase 4, and the differ
skips the level when a candidate has no manifest rather than reporting 592 differences
against output that does not exist yet.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import corpus  # noqa: E402
import normalize  # noqa: E402

CRATE_ROOT = corpus.ROOT
BINARY = CRATE_ROOT / 'target' / 'release' / ('luadox.exe' if sys.platform == 'win32'
                                             else 'luadox')


def build() -> None:
    proc = subprocess.run(['cargo', 'build', '--release'], cwd=str(CRATE_ROOT),
                          capture_output=True)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr.decode('utf-8', 'replace'))
        raise SystemExit('cargo build failed')


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--out', type=Path, default=corpus.BUILD_DIR / 'candidate')
    ap.add_argument('--no-build', action='store_true')
    args = ap.parse_args()

    prov = corpus.provenance()
    if not args.no_build:
        build()
    if not BINARY.exists():
        raise SystemExit(f'no binary at {BINARY}; run without --no-build')

    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    doc = out / 'doc.json'
    diag = out / 'diagnostics.json'

    # The config's globs are relative to this directory, so the binary is only meaningful
    # when run from it -- which is where generate_api_docs.py runs luadox from.
    argv = [str(BINARY), '-c', str(corpus.CONFIG), '-r', 'json', '-o', str(doc),
            '--diagnostics-json', str(diag), '--diagnostics-root', str(corpus.CONFIG_CWD)]
    started = time.perf_counter()
    proc = subprocess.run(argv, cwd=str(corpus.CONFIG_CWD), capture_output=True)
    elapsed = time.perf_counter() - started
    log = proc.stderr.decode('utf-8', 'replace')
    if not doc.exists():
        sys.stderr.write(log)
        raise SystemExit(f'the json renderer produced nothing (exit {proc.returncode})')

    payload = normalize.diagnostics(json.loads(diag.read_text(encoding='utf-8')),
                                    corpus.CORPUS_REPO)
    diag.write_text(json.dumps(payload, indent=2, sort_keys=True) + '\n',
                    encoding='utf-8', newline='\n')

    by_cat: dict[str, int] = {}
    for entry in payload['diagnostics']:
        by_cat[entry['category']] = by_cat.get(entry['category'], 0) + 1
    print(f'  json: exit {proc.returncode} in {elapsed:.2f}s')
    print('  diagnostics: ' + (', '.join(f'{v} {k}' for k, v in sorted(by_cat.items()))
                               or 'none'))

    (out / 'provenance.json').write_text(json.dumps({
        'corpus_commit': prov.corpus_commit,
        'corpus_dirty': prov.corpus_dirty,
        'candidate_commit': subprocess.run(
            ['git', '-C', str(CRATE_ROOT), 'rev-parse', 'HEAD'],
            capture_output=True).stdout.decode('utf-8').strip(),
        'exit_code': proc.returncode,
        'seconds': {'json': round(elapsed, 2)},
    }, indent=2, sort_keys=True) + '\n', encoding='utf-8', newline='\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
