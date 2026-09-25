"""
Runs luadox over every fixture and compares its output against the checked-in expected output.

The production corpus does not use eleven of the tags the tool supports -- `@module`,
`@scope`, `@rename`, `@alias`, `@type`, `@meta`, `@order`, `@field`, `@fullnames`,
`@usage`, `@code` -- so these fixtures are what exercises them.

    python spec/run.py                # every fixture
    python spec/run.py naming xrefs   # just these
    python spec/run.py --show 40      # more of each difference

It needs only a built binary.

All three renderers are graded: the json document structurally, the LuaLS definition
file and every recorded html page byte for byte. The asset bundle's `?<version>`
cache-buster is normalised on both sides: parity on *where it appears* is required, not on
its value.
"""

from __future__ import annotations

import argparse
import difflib
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

import fixture_setup  # noqa: E402
import normalize  # noqa: E402
from fixture_setup import diff_json  # noqa: E402


def render(name: str, renderer: str, out: Path) -> tuple[int, Path, dict | None]:
    """Runs one renderer over one fixture, returning its exit code, where it wrote, and
    the diagnostics of that run."""
    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = Path(tmpdir)
        config = tmp / 'fixture.conf'
        fixture_setup.write_config(config, name)
        target = out / renderer
        diag = out / ('diagnostics-' + renderer + '.json')
        argv = [str(BINARY), '-c', str(config), '-r', renderer, '-o', str(target),
                '--diagnostics-json', str(diag), '--diagnostics-root', str(SRC)]
        proc = subprocess.run(argv, cwd=str(tmp), capture_output=True)
        written = {
            'luals': target / 'luadox.lua',
            'json': target / 'luadox.json',
            # The html renderer writes a directory; the caller walks it.
            'html': target,
        }[renderer]
        if not written.exists():
            sys.stderr.write(proc.stderr.decode('utf-8', 'replace'))
        return (proc.returncode, written,
                json.loads(diag.read_text(encoding='utf-8')) if diag.exists() else None)


def compare_bytes(expected: Path, actual: Path, show: int) -> list[str]:
    """
    Byte comparison, reported as a unified diff so a failure names the lines rather than
    two digests. The cache-buster is normalised first, on both sides.
    """
    def text(path: Path) -> list[str]:
        if not path.exists():
            return []
        data = normalize.newlines(path.read_bytes()).decode('utf-8')
        return normalize.RE_ASSETS_VERSION.sub(
            normalize.ASSETS_VERSION_TOKEN, data).splitlines()

    want, got = text(expected), text(actual)
    if want == got:
        return []
    return list(difflib.unified_diff(
        want, got, 'expected', 'actual', lineterm='', n=1))[:show]


def compare_html(expected_dir: Path, actual_dir: Path, show: int) -> tuple[int, int, list[str]]:
    """Every recorded html file of one fixture. Returns (same, total, first diff)."""
    same = total = 0
    first: list[str] = []
    for want in sorted(expected_dir.rglob('*')):
        if not want.is_file():
            continue
        total += 1
        rel = want.relative_to(expected_dir)
        diff = compare_bytes(want, actual_dir / rel, show)
        if diff:
            if not first:
                first = [f'html/{rel.as_posix()}'] + diff
        else:
            same += 1
    return same, total, first


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('names', nargs='*')
    ap.add_argument('--show', type=int, default=8)
    args = ap.parse_args()

    if not BINARY.exists():
        raise SystemExit(f'no binary at {BINARY}; cargo build --release')
    names = args.names or fixture_setup.fixtures()
    unknown = sorted(set(names) - set(fixture_setup.fixtures()))
    if unknown:
        raise SystemExit('no such fixture: {}'.format(', '.join(unknown)))

    failed = 0
    with tempfile.TemporaryDirectory() as tmpdir:
        for name in names:
            out = Path(tmpdir) / name
            out.mkdir(parents=True, exist_ok=True)
            problems: list[str] = []
            notes: list[str] = []

            code, written, diag = render(name, 'json', out)
            if not written.exists():
                problems.append('produced no doc.json (exit {})'.format(code))
            else:
                expected = json.loads((EXPECTED / name / 'doc.json')
                                      .read_text(encoding='utf-8'))
                paths = diff_json(expected, json.loads(written.read_text(encoding='utf-8')))
                notes.append('{} json differences'.format(len(paths)))
                problems.extend('json {}'.format(p) for p in paths[:args.show])
                want = json.loads((EXPECTED / name / 'exit.json').read_text())['json']
                if code != want:
                    notes.append('exit {} vs {}'.format(code, want))
            delta = diagnostics_delta(EXPECTED / name / 'diagnostics-json.json', diag)
            if delta:
                notes.append(delta.lstrip(', '))

            code, written, diag = render(name, 'luals', out)
            diff = compare_bytes(EXPECTED / name / 'luadox.lua', written, args.show)
            notes.append('luals ' + ('identical' if not diff else 'DIFFERS'))
            problems.extend(diff)
            delta = diagnostics_delta(EXPECTED / name / 'diagnostics-luals.json', diag)
            if delta:
                notes.append('luals ' + delta.lstrip(', '))

            code, _, diag = render(name, 'html', out)
            same, total, diff = compare_html(EXPECTED / name / 'html', out / 'html',
                                             args.show)
            notes.append('html {}/{}'.format(same, total))
            problems.extend(diff)
            delta = diagnostics_delta(EXPECTED / name / 'diagnostics-html.json', diag)
            if delta:
                notes.append('html ' + delta.lstrip(', '))

            status = 'ok' if not problems else 'FAILED'
            if problems:
                failed += 1
            print('{}: {} ({})'.format(name, status, ', '.join(notes)))
            for line in problems[:args.show]:
                print('   ! {}'.format(line))

    print(f'\n{len(names) - failed}/{len(names)} fixtures match')
    return 1 if failed else 0


def diagnostics_delta(expected_path: Path, actual: dict | None) -> str:
    """Diagnostics never fail a fixture; they are reported as a delta."""
    if actual is None or not expected_path.exists():
        return ''
    expected = json.loads(expected_path.read_text(encoding='utf-8'))

    def keyed(payload):
        # A path inside a message names this machine; only the tail below the fixture tree
        # is behaviour.
        return {(e['category'], e['file'], e['line'],
                 normalize.message(e['message'], FIXTURES, fixture_setup.FIXTURES_TOKEN))
                for e in payload['diagnostics']}

    a, b = keyed(expected), keyed(actual)
    gained, lost = len(b - a), len(a - b)
    if not gained and not lost:
        return ''
    return f', diagnostics +{gained}/-{lost}'


if __name__ == '__main__':
    raise SystemExit(main())
