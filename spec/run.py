"""
Runs the Rust luadox over every fixture and compares its json against the oracle's.

The production corpus does not use eleven of the tags the tool supports -- `@module`,
`@scope`, `@rename`, `@alias`, `@type`, `@meta`, `@order`, `@field`, `@fullnames`,
`@usage`, `@code` -- so nothing in the differential harness would notice if they broke.
These fixtures are the second corpus that does.

    python spec/run.py                # every fixture
    python spec/run.py naming xrefs   # just these
    python spec/run.py --show 40      # more of each difference

The expected output is produced by spec/fixtures/regenerate.py from the pinned oracle
and checked in, so this needs no oracle to run -- only a built binary.

Only the json level is graded here. The LuaLS and html outputs are checked in too, and
become gates when their renderers exist in Phase 3 and Phase 4.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
FIXTURES = HERE / 'fixtures'
SRC = FIXTURES / 'src'
EXPECTED = FIXTURES / 'expected'
BINARY = ROOT / 'target' / 'release' / ('luadox.exe' if sys.platform == 'win32'
                                        else 'luadox')

sys.path.insert(0, str(ROOT / 'harness'))
from differ import diff_json  # noqa: E402

# Kept in step with regenerate.py: a fixture is one source file, plus a manual page and
# extra config where it needs them.
sys.path.insert(0, str(FIXTURES))
import regenerate  # noqa: E402


def render(name: str, out: Path) -> tuple[int, dict | None, dict | None]:
    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = Path(tmpdir)
        config = tmp / 'fixture.conf'
        regenerate.write_config(config, name)
        doc = out / 'doc.json'
        diag = out / 'diagnostics.json'
        argv = [str(BINARY), '-c', str(config), '-r', 'json', '-o', str(doc),
                '--diagnostics-json', str(diag), '--diagnostics-root', str(SRC)]
        proc = subprocess.run(argv, cwd=str(tmp), capture_output=True)
        if not doc.exists():
            sys.stderr.write(proc.stderr.decode('utf-8', 'replace'))
            return proc.returncode, None, None
        return (proc.returncode,
                json.loads(doc.read_text(encoding='utf-8')),
                json.loads(diag.read_text(encoding='utf-8')) if diag.exists() else None)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('names', nargs='*')
    ap.add_argument('--show', type=int, default=8)
    args = ap.parse_args()

    if not BINARY.exists():
        raise SystemExit(f'no binary at {BINARY}; cargo build --release')
    names = args.names or regenerate.fixtures()
    unknown = sorted(set(names) - set(regenerate.fixtures()))
    if unknown:
        raise SystemExit('no such fixture: {}'.format(', '.join(unknown)))

    failed = 0
    with tempfile.TemporaryDirectory() as tmpdir:
        for name in names:
            out = Path(tmpdir) / name
            out.mkdir(parents=True, exist_ok=True)
            code, doc, diag = render(name, out)
            expected_path = EXPECTED / name / 'doc.json'
            if doc is None:
                print(f'{name}: FAILED -- produced no doc.json (exit {code})')
                failed += 1
                continue
            expected = json.loads(expected_path.read_text(encoding='utf-8'))
            paths = diff_json(expected, doc)
            want = json.loads((EXPECTED / name / 'exit.json').read_text())['json']
            delta = diagnostics_delta(EXPECTED / name / 'diagnostics-json.json', diag)
            status = 'ok' if not paths else 'FAILED'
            if paths:
                failed += 1
            note = f', exit {code} vs {want}' if code != want else ''
            print(f'{name}: {status} ({len(paths)} json differences{note}{delta})')
            for p in paths[:args.show]:
                print(f'   ! {p}')
            if len(paths) > args.show:
                print(f'   ! ... {len(paths) - args.show} more')

    print(f'\n{len(names) - failed}/{len(names)} fixtures match')
    return 1 if failed else 0


def slashes(message: str) -> str:
    """A path's separator is the host's, not behaviour, and Python's OSError doubles a
    Windows backslash because it prints the filename through repr()."""
    return message.replace('\\\\', '/').replace('\\', '/')


def diagnostics_delta(expected_path: Path, actual: dict | None) -> str:
    """Diagnostics never fail a fixture; they are reported the way the differ reports
    them, as a delta."""
    if actual is None or not expected_path.exists():
        return ''
    expected = json.loads(expected_path.read_text(encoding='utf-8'))

    def keyed(payload):
        return {(e['category'], e['file'], e['line'], slashes(e['message']))
                for e in payload['diagnostics']}

    a, b = keyed(expected), keyed(actual)
    gained, lost = len(b - a), len(a - b)
    if not gained and not lost:
        return ''
    return f', diagnostics +{gained}/-{lost}'


if __name__ == '__main__':
    raise SystemExit(main())
