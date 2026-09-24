"""
The third corpus: public projects that document themselves with luadox.

The production corpus and the fixtures are both written by the people who wrote this
tool, so they share its habits. These projects were found on GitHub by searching for
`luadox.conf` and for the generated html's footer, and they use the tool in ways
neither corpus does: a directory as input, `@module` and `@section` in one block, a
module assigning its own table, pipe tables, manual pages other than the landing page,
Lua 5.4 syntax. Every one of those was a divergence when this script first ran.

    python harness/foreign_corpora.py            # clone what is missing, run everything
    python harness/foreign_corpora.py rtk OFS    # just these

Clones go to $LUADOX_FOREIGN_CORPORA (default <repo>/_foreign, ignored by git), shallow,
at whatever their default branch is today: this is a smoke test against live projects,
not a pinned golden run. The oracle and the release binary each render html, json and
luals; the outputs are normalised the way spec/run.py normalises (LF, the assets
cache-buster) and compared file by file.

What is expected to remain, as of the run recorded in the README:

  * json/luals: a `value` the oracle drops because the literal spans lines
    (improvements.toml, "A field value whose expression spans more than one line").
  * rtk: four fields the oracle invents from `if x == nil then` lines ("A comparison
    on the line after a block is not a field").
  * meshchat index.html: one indented continuation line the oracle keeps as a
    paragraph and comrak makes a code block ("comrak has no CODE_INDENT equivalent").
  * guns4d: the oracle dies (KeyError in `Reference.topref`) and writes nothing.

Both tools get `--nofollow`: the Rust does not follow `require()`, and a directory
input, which the oracle turns into `init.lua` plus everything it requires, is replaced
by a glob over the directory. The oracle gets `--sidebar-template` because the fork's
data directory has no default sidebar, and `encoding = utf8` because it otherwise reads
the templates in the locale's codepage.
"""

from __future__ import annotations

import filecmp
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
ORACLE = ROOT / 'oracle'
BINARY = ROOT / 'target' / 'release' / ('luadox.exe' if sys.platform == 'win32' else 'luadox')
SIDEBAR = ROOT / 'crates' / 'luadox' / 'assets' / 'sidebar.tmpl.html'
CORPORA = Path(os.environ.get('LUADOX_FOREIGN_CORPORA', ROOT / '_foreign'))
OUT = CORPORA / '_out'
ASSETS_VERSION = re.compile(rb'\?[0-9a-f]{7,64}(?=["\'])')
NEWLINES = re.compile(rb'\r*\n')

# name -> (github repo, directory the config's globs resolve against, config file,
# extra positional files). A config named `_luadox.conf` is written by this script.
PROJECTS = {
    'rtk': ('jtackaberry/rtk', 'doc', '_luadox.conf', []),
    'meshchat': ('hickey/meshchat', '.', 'luadox.conf', []),
    'guns4d': ('FatalistError/guns4d', 'generate_docs', 'luadox.conf', []),
    'OFS': ('OpenFunscripter/OFS', 'LuaDox', 'luadox.conf', ['stubs.lua']),
    'ReaCAT': ('MathieuCGit/Reaper_Tools', 'Chords tool/ReaCAT', '_luadox.conf', []),
    'mta-collectibles': ('Fernando-A-Rocha/mta-collectibles', '.', '_luadox.conf', []),
}


def clone(name: str, repo: str) -> Path:
    target = CORPORA / name
    if not target.exists():
        CORPORA.mkdir(parents=True, exist_ok=True)
        subprocess.run(['git', 'clone', '-q', '--depth', '1', f'https://github.com/{repo}.git',
                        str(target)], check=True)
    return target


def write_configs() -> None:
    # rtk documents a directory; list its files instead, and do not follow.
    doc = CORPORA / 'rtk' / 'doc'
    conf = (doc / 'luadox.conf').read_text(encoding='utf-8')
    conf = re.sub(r'^files = .*$', 'files = rtk=../rtk/*.lua', conf, flags=re.M)
    conf = re.sub(r'^follow = .*$', 'follow = false', conf, flags=re.M)
    (doc / '_luadox.conf').write_text(conf, encoding='utf-8')

    # ReaCAT and mta-collectibles ship rendered docs but no config.
    (CORPORA / 'ReaCAT' / 'Chords tool' / 'ReaCAT' / '_luadox.conf').write_text(
        '[project]\nname = ReaCAT\nfiles = *.lua\nfollow = false\nencoding = utf8\n',
        encoding='utf-8')
    mta = CORPORA / 'mta-collectibles'
    files = sorted(p.relative_to(mta).as_posix() for p in mta.rglob('*.lua'))
    (mta / '_luadox.conf').write_text(
        '[project]\nname = collectibles\nfiles = ' + '\n    '.join(files)
        + '\nfollow = false\nencoding = utf8\n', encoding='utf-8')


def run(argv: list[str], cwd: Path, env: dict | None = None) -> tuple[int, str, float]:
    """Exit code, stderr and wall time of one render."""
    started = time.perf_counter()
    proc = subprocess.run(argv, cwd=str(cwd), env=env, capture_output=True)
    return proc.returncode, proc.stderr.decode('utf-8', 'replace'), time.perf_counter() - started


def normalise(root: Path) -> None:
    for path in root.rglob('*'):
        if path.is_file() and path.suffix in ('.html', '.js', '.json', '.lua', '.css'):
            data = NEWLINES.sub(b'\n', path.read_bytes())
            path.write_bytes(ASSETS_VERSION.sub(b'?ASSETS_VERSION', data))


def tree_diff(a: Path, b: Path) -> list[str]:
    bad: list[str] = []

    def walk(d: filecmp.dircmp, prefix: str = '') -> None:
        bad.extend(prefix + f for f in d.diff_files)
        bad.extend('oracle only: ' + prefix + f for f in d.left_only)
        bad.extend('rust only: ' + prefix + f for f in d.right_only)
        for name, sub in d.subdirs.items():
            walk(sub, prefix + name + '/')

    walk(filecmp.dircmp(a, b))
    return bad


def compare(name: str) -> bool:
    repo, cwd, conf, extra = PROJECTS[name]
    cwd = clone(name, repo) / cwd
    env = dict(os.environ, PYTHONPATH=str(ORACLE), PYTHONIOENCODING='utf-8')
    print(f'=== {name}')
    clean = True
    for renderer in ('html', 'json', 'luals'):
        oracle_out = OUT / name / f'oracle-{renderer}'
        rust_out = OUT / name / f'rust-{renderer}'
        for out in (oracle_out, rust_out):
            shutil.rmtree(out, ignore_errors=True)
            if renderer != 'html':
                out.mkdir(parents=True)
        target = (lambda out: out) if renderer == 'html' else (lambda out: out / f'doc.{renderer}')
        common = ['-c', conf, '-r', renderer, '--nofollow', *extra]
        rc_oracle, log, t_oracle = run(
            [sys.executable, '-c', 'from luadox.main import main; main()', *common,
             '--sidebar-template', str(SIDEBAR), '-o', str(target(oracle_out))], cwd, env)
        rc_rust, _, t_rust = run([str(BINARY), *common, '-o', str(target(rust_out))], cwd)
        # Wall time of one cold run each, oracle then Rust; the oracle's includes the
        # interpreter start-up, which is the same for every project.
        timing = f'{t_oracle:.2f} s vs {t_rust:.3f} s'
        if not oracle_out.exists() or not any(oracle_out.rglob('*')):
            last = log.strip().splitlines()[-1] if log.strip() else ''
            print(f'  {renderer:<5} oracle wrote nothing (exit {rc_oracle}): {last[:110]}')
            clean = False
            continue
        normalise(oracle_out)
        normalise(rust_out)
        bad = tree_diff(oracle_out, rust_out)
        files = sum(1 for p in rust_out.rglob('*') if p.is_file())
        if bad:
            clean = False
            shown = ', '.join(bad[:5]) + (' ...' if len(bad) > 5 else '')
            print(f'  {renderer:<5} exit {rc_oracle}/{rc_rust}, {files} files, {timing}, '
                  f'{len(bad)} differ: {shown}')
        else:
            print(f'  {renderer:<5} exit {rc_oracle}/{rc_rust}, {files} files, {timing}, identical')
    return clean


def main() -> int:
    names = sys.argv[1:] or list(PROJECTS)
    for name in names:
        clone(name, PROJECTS[name][0])
    write_configs()
    identical = [name for name in names if compare(name)]
    print(f'\n{len(identical)}/{len(names)} projects identical: {", ".join(identical) or "-"}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
