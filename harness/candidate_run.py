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

    manifest.sha256    a digest per output file, for the L2 compare
    luals/luadox.lua   L2 -- the LuaLS definition file
    html/              L2 -- the 591-file rendered site

The differ reports a whole output tree the candidate did not produce as skipped rather
than as 591 failures, and any file *inside* a tree it did produce as a difference like any
other.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

LF = chr(10)

# The same pair oracle_run.py normalises with, on bytes.
RE_ASSETS_VERSION = re.compile(rb'\?[0-9a-f]{7,64}(?=["\'])')
ASSETS_VERSION_TOKEN = b'?ASSETS_VERSION'

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
    manifest: list[str] = []
    timings: dict[str, float] = {}

    # Diagnostics ride along with the json render: they come out of parsing, which every
    # renderer does identically, so the cheapest carrier wins.
    code, elapsed = run('json', doc, diag)
    timings['json'] = elapsed
    if not doc.exists():
        raise SystemExit('the json renderer produced nothing (exit {})'.format(code))
    manifest.append('{}  doc.json'.format(digest(doc)))

    payload = normalize.diagnostics(json.loads(diag.read_text(encoding='utf-8')),
                                    corpus.CORPUS_REPO)
    diag.write_text(json.dumps(payload, indent=2, sort_keys=True) + LF,
                    encoding='utf-8', newline=LF)
    manifest.append('{}  diagnostics.json'.format(digest(diag)))

    by_cat: dict[str, int] = {}
    for entry in payload['diagnostics']:
        by_cat[entry['category']] = by_cat.get(entry['category'], 0) + 1
    print('  json: exit {} in {:.2f}s'.format(code, elapsed))
    print('  diagnostics: ' + (', '.join('{} {}'.format(v, k)
                                         for k, v in sorted(by_cat.items())) or 'none'))

    for renderer in ('luals', 'html'):
        target = out / renderer
        if target.exists():
            shutil.rmtree(target)
        code, elapsed = run(renderer, target, None)
        timings[renderer] = elapsed
        files = 0
        for path in sorted(target.rglob('*')):
            if path.is_file():
                rel = path.relative_to(target).as_posix()
                manifest.append('{}  {}/{}'.format(digest(path), renderer, rel))
                files += 1
        print('  {}: exit {} in {:.2f}s, {} files'.format(renderer, code, elapsed, files))

    manifest.sort(key=lambda line: line.split('  ', 1)[1])
    (out / 'manifest.sha256').write_text(LF.join(manifest) + LF,
                                         encoding='utf-8', newline=LF)
    print('  manifest: {} files'.format(len(manifest)))

    (out / 'provenance.json').write_text(json.dumps({
        'corpus_commit': prov.corpus_commit,
        'corpus_dirty': prov.corpus_dirty,
        'candidate_commit': subprocess.run(
            ['git', '-C', str(CRATE_ROOT), 'rev-parse', 'HEAD'],
            capture_output=True).stdout.decode('utf-8').strip(),
        'exit_code': code,
        'renderers': sorted(timings),
        'seconds': {k: round(v, 2) for k, v in timings.items()},
    }, indent=2, sort_keys=True) + LF, encoding='utf-8', newline=LF)
    return 0


def digest(path: Path) -> str:
    """
    The same rule oracle_run.py digests with: line endings normalised, because the
    Python's are a property of the host and not of the tool, and the asset bundle's
    cache-buster reduced to a token, because it is a sha256 over bytes the two
    implementations store differently.
    """
    data = normalize.newlines(path.read_bytes())
    data = RE_ASSETS_VERSION.sub(ASSETS_VERSION_TOKEN, data)
    return hashlib.sha256(data).hexdigest()


def run(renderer: str, out: Path, diagnostics: Path | None) -> tuple[int, float]:
    # The config's globs are relative to this directory, so the binary is only meaningful
    # when run from it -- which is where generate_api_docs.py runs luadox from.
    argv = [str(BINARY), '-c', str(corpus.CONFIG), '-r', renderer, '-o', str(out)]
    if diagnostics:
        argv += ['--diagnostics-json', str(diagnostics),
                 '--diagnostics-root', str(corpus.CONFIG_CWD)]
    started = time.perf_counter()
    proc = subprocess.run(argv, cwd=str(corpus.CONFIG_CWD), capture_output=True)
    elapsed = time.perf_counter() - started
    if not out.exists():
        sys.stderr.write(proc.stderr.decode('utf-8', 'replace'))
    return proc.returncode, elapsed


if __name__ == '__main__':
    raise SystemExit(main())
