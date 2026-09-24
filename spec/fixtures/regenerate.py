"""
Regenerates every file under spec/fixtures/expected/ by running the Python oracle.

Nothing in expected/ is written by hand.  Each fixture is one input file under src/,
rendered twice -- once with the luals renderer, once with the html renderer -- into a
scratch directory; the interesting outputs are then copied into
expected/<fixture>/ together with the diagnostics of both runs.

    python spec/fixtures/regenerate.py            # all fixtures
    python spec/fixtures/regenerate.py enum xrefs # just these two
    python spec/fixtures/regenerate.py --check    # regenerate into a temp dir and
                                                  # fail if anything differs

The oracle is the clone at <repo>/oracle, i.e. origin/luals-all plus oracle-patches/
(see the repository README).  Its commit is recorded in expected/provenance.json, and
a regeneration from a different commit will say so there.

What is deliberately *not* copied into expected/:

  * the eleven static asset files (luadox.css, prism.*, js-search.min.js, search.js
    and the six SVGs).  They are byte-identical for every fixture and are listed by
    name and digest in provenance.json instead.

The '?<assets_version>' cache-buster in the html *is* kept verbatim.  It is a sha256
over the asset bundle, so a candidate implementation that ships the same assets
encoded differently will differ on it: normalise '?<hex>' to '?ASSETS_VERSION' on
both sides before comparing, exactly as harness/normalize.py does for the corpus.

One normalisation *is* applied to the copied files: line endings are reduced to LF.
The oracle opens its output in text mode, so every '\\n' it writes becomes os.linesep
-- CRLF on Windows -- while the three default templates are read as *bytes* and keep
whatever the git checkout gave them, which with core.autocrlf=true is CRLF.  The two
compose: template lines come out as '\\r\\r\\n' on Windows and as '\\r\\n' on Linux, and
generated lines as '\\r\\n' and '\\n'.  The oracle's raw bytes are therefore a property
of the host, not of the renderer, and cannot be a fixture.  See spec/html.md
("Line endings") for what a port should do instead.
"""

from __future__ import annotations

import argparse
import filecmp
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# '\r\r\n' (Windows) and '\r\n' (Linux, template lines) both become '\n'.
RE_NEWLINES = re.compile(rb'\r*\n')

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
ORACLE = ROOT / 'oracle'

sys.path.insert(0, str(ROOT / 'harness'))
import normalize  # noqa: E402
SRC = HERE / 'src'
SNIPPETS = HERE / 'snippets'
SIDEBAR = HERE / 'sidebar.tmpl.html'
EXPECTED = HERE / 'expected'

# Copied out of the rendered html tree, in this order.  Everything else the html
# renderer writes is a static asset (see the module docstring).
KEEP_GLOBS = ('class/*.html', 'module/*.html', 'index.html', 'search.html', 'index.js')

# Fixtures that also register manual pages: fixture name -> [(page id, markdown file in
# src/)].  A page other than `index` links with bare section fragments, which only a
# second page can show.
MANUALS = {
    'manual': [('index', 'manual.md')],
    'manual_pages': [('index', 'manual.md'), ('guide', 'manual_pages_guide.md')],
}

# Fixtures that need a second source file, which lives under src/extra/ so that it is
# not itself globbed as a fixture.  A conflict between two pages can only be written
# across two files.
EXTRA_FILES = {'conflicts': ['extra/conflicts_twin.lua']}

# A diagnostic message can quote the absolute path of the file it is about, which names
# this machine.  Only the tail below the fixture tree is behaviour, so the recorded
# expectation carries a token instead -- and spec/run.py applies the same rule to the
# candidate before comparing.
FIXTURES_TOKEN = '<fixtures>'

# Extra config sections a fixture needs, appended to the generated .conf verbatim.
EXTRA_CONFIG = {
    'luals_config': [
        '[luals]',
        'globals = app:Application env',
        'mixin_suffix = Metadata',
        'mixin_doc_phrase = Inherits properties from',
    ],
}


def fixtures() -> list[str]:
    return sorted(p.stem for p in SRC.glob('*.lua'))


def copy_lf(src: Path, dst: Path) -> None:
    """Copies a rendered file with its line endings reduced to LF."""
    dst.write_bytes(RE_NEWLINES.sub(b'\n', src.read_bytes()))


def write_config(path: Path, name: str) -> None:
    lines = [
        '[project]',
        'name = LuaDox Fixture',
        'title = Fixture',
        'files = {}'.format(' '.join(
            [(SRC / (name + '.lua')).as_posix()]
            + [(SRC / extra).as_posix() for extra in EXTRA_FILES.get(name, [])])),
        'follow = false',
        'encoding = utf8',
        'snippet_path = {}'.format(SNIPPETS.as_posix()),
        'sidebar_template = {}'.format(SIDEBAR.as_posix()),
    ]
    if name in MANUALS:
        lines += ['', '[manual]']
        lines += ['{} = {}'.format(page, (SRC / md).as_posix()) for page, md in MANUALS[name]]
    if name in EXTRA_CONFIG:
        lines += [''] + EXTRA_CONFIG[name]
    path.write_text('\n'.join(lines) + '\n', encoding='utf-8', newline='\n')


def run(renderer: str, config: Path, out: Path, diagnostics: Path, cwd: Path) -> int:
    argv = [sys.executable, '-c', 'from luadox.main import main; main()',
            '-c', str(config), '-r', renderer, '-o', str(out),
            '--diagnostics-json', str(diagnostics), '--diagnostics-root', str(SRC)]
    env = dict(os.environ, PYTHONPATH=str(ORACLE), PYTHONIOENCODING='utf-8')
    proc = subprocess.run(argv, cwd=str(cwd), env=env, capture_output=True)
    if not diagnostics.exists():
        sys.stderr.write(proc.stderr.decode('utf-8', 'replace'))
        raise SystemExit('{} renderer produced no diagnostics for {}'.format(
            renderer, config))
    return proc.returncode


def generate(name: str, dest: Path) -> dict:
    """Renders one fixture into dest/, returning what goes into provenance."""
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = Path(tmpdir)
        config = tmp / 'fixture.conf'
        write_config(config, name)

        # The json renderer first: it is the document made inspectable, so a fixture
        # whose json matches has its parsing, resolution and content assembly matched,
        # and any later difference is a rendering bug rather than a resolution one.
        json_out = tmp / 'doc.json'
        json_diag = tmp / 'diagnostics-json.json'
        json_exit = run('json', config, json_out, json_diag, tmp)
        copy_lf(json_out, dest / 'doc.json')

        luals_out = tmp / 'luals'
        luals_diag = tmp / 'diagnostics-luals.json'
        luals_exit = run('luals', config, luals_out, luals_diag, tmp)
        copy_lf(luals_out / 'luadox.lua', dest / 'luadox.lua')

        html_out = tmp / 'html'
        html_diag = tmp / 'diagnostics-html.json'
        html_exit = run('html', config, html_out, html_diag, tmp)
        kept = set()
        for pattern in KEEP_GLOBS:
            for path in sorted(html_out.glob(pattern)):
                rel = path.relative_to(html_out)
                kept.add(rel.as_posix())
                target = dest / 'html' / rel
                target.parent.mkdir(parents=True, exist_ok=True)
                copy_lf(path, target)

        for src, dst in ((json_diag, 'diagnostics-json.json'),
                         (luals_diag, 'diagnostics-luals.json'),
                         (html_diag, 'diagnostics-html.json')):
            payload = normalize.diagnostics(
                json.loads(src.read_text(encoding='utf-8')), HERE, FIXTURES_TOKEN)
            (dest / dst).write_text(
                json.dumps(payload, indent=2, sort_keys=True) + '\n',
                encoding='utf-8', newline='\n')
        (dest / 'exit.json').write_text(
            json.dumps({'json': json_exit, 'luals': luals_exit, 'html': html_exit},
                       indent=2, sort_keys=True) + '\n',
            encoding='utf-8', newline='\n')

        assets = {}
        for path in sorted(html_out.rglob('*')):
            rel = path.relative_to(html_out).as_posix()
            if path.is_file() and rel not in kept:
                assets[rel] = hashlib.sha256(path.read_bytes()).hexdigest()
    return {'assets': assets}


def git(*args: str) -> str:
    out = subprocess.run(['git', '-C', str(ORACLE), *args], capture_output=True)
    return out.stdout.decode('utf-8').strip()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('names', nargs='*', help='fixtures to regenerate (default: all)')
    ap.add_argument('--check', action='store_true',
                    help='regenerate into a scratch tree and fail on any difference')
    args = ap.parse_args()

    if not ORACLE.exists():
        raise SystemExit('no oracle at {}; build it as the repository README says'
                         .format(ORACLE))
    names = args.names or fixtures()
    unknown = sorted(set(names) - set(fixtures()))
    if unknown:
        raise SystemExit('no such fixture: {}'.format(', '.join(unknown)))

    if args.check:
        with tempfile.TemporaryDirectory() as tmpdir:
            bad = []
            for name in names:
                generate(name, Path(tmpdir) / name)
                cmp = filecmp.dircmp(str(EXPECTED / name), str(Path(tmpdir) / name))
                bad += [(name, d) for d in walk_diff(cmp)]
            for name, detail in bad:
                print('{}: {}'.format(name, detail))
            print('{} fixture(s) checked, {} difference(s)'.format(len(names), len(bad)))
            return 1 if bad else 0

    assets: dict[str, str] = {}
    for name in names:
        info = generate(name, EXPECTED / name)
        assets.update(info['assets'])
        print('{}: regenerated'.format(name))

    (EXPECTED / 'provenance.json').write_text(json.dumps({
        'oracle_commit': git('rev-parse', 'HEAD'),
        'oracle_dirty': bool(git('status', '--porcelain')),
        'fixtures': fixtures(),
        'assets_not_checked_in': assets,
        'normalisations': {
            'newlines': 'every rendered file is copied with its line endings reduced '
                        'to LF; the oracle writes CRLF on Windows and its byte-read '
                        'default templates add a second CR, so its raw bytes are a '
                        'property of the host (see spec/html.md, "Line endings")',
            'assets_version': 'kept verbatim; normalise ?<hex> to ?ASSETS_VERSION on '
                              'both sides before comparing a candidate implementation',
        },
    }, indent=2, sort_keys=True) + '\n', encoding='utf-8', newline='\n')
    print('provenance -> {}'.format(EXPECTED / 'provenance.json'))
    return 0


def walk_diff(cmp: filecmp.dircmp) -> list[str]:
    out = ['only in expected: ' + f for f in cmp.left_only]
    out += ['only in regenerated: ' + f for f in cmp.right_only]
    out += ['differs: ' + f for f in cmp.diff_files]
    for sub in cmp.subdirs.values():
        out += walk_diff(sub)
    return out


if __name__ == '__main__':
    raise SystemExit(main())
