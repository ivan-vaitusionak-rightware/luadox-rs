"""
Where the differential harness finds its inputs.

Every path here is a pinned input, not a default: the corpus is a specific checkout at a
specific commit and the oracle is a specific commit of the Python fork.  A run that
cannot confirm both refuses to produce golden data, because golden data recorded
against an unknown tree is worse than none.

The corpus is not this repository's to name, so its location comes from the environment:

  LUADOX_CORPUS_REPO        the checkout luadox documents (required)
  LUADOX_CORPUS_CONFIG      the luadox config file, relative to that checkout (required)
  LUADOX_CORPUS_CONFIG_CWD  the directory the config's globs resolve against, relative to
                            the checkout (default: the config file's own directory)
  LUADOX_CORPUS_LUA_ROOT    the root of the Lua sources, relative to the checkout
                            (default: lua/src)

The pinned corpus commit is the one golden/provenance.json records; the first --record
run pins it.  CORPUS_REPO, CONFIG, CONFIG_CWD, LUA_ROOT and CORPUS_COMMIT resolve on
first access, so importing this module needs no environment.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent

ORACLE_DIR = ROOT / 'oracle'
GOLDEN_DIR = ROOT / 'golden'
BUILD_DIR = ROOT / '_build'


def _env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise SystemExit(f'{name} is not set; harness/corpus.py lists the variables the '
                         'harness reads')
    return value


def _corpus_repo() -> Path:
    return Path(_env('LUADOX_CORPUS_REPO'))


def _config() -> Path:
    return _corpus_repo() / _env('LUADOX_CORPUS_CONFIG')


def _config_cwd() -> Path:
    # luadox resolves the globs in its config against the current directory, not against
    # the config file, so the config is only meaningful together with this working
    # directory.
    relative = os.environ.get('LUADOX_CORPUS_CONFIG_CWD')
    return _corpus_repo() / relative if relative else _config().parent


def _lua_root() -> Path:
    return _corpus_repo() / os.environ.get('LUADOX_CORPUS_LUA_ROOT', 'lua/src')


def _pinned_commit() -> str | None:
    provenance_file = GOLDEN_DIR / 'provenance.json'
    if not provenance_file.exists():
        return None
    return json.loads(provenance_file.read_text(encoding='utf-8')).get('corpus_commit')


_LAZY = {
    'CORPUS_REPO': _corpus_repo,
    'CONFIG': _config,
    'CONFIG_CWD': _config_cwd,
    'LUA_ROOT': _lua_root,
    'CORPUS_COMMIT': _pinned_commit,
}


def __getattr__(name: str):
    try:
        return _LAZY[name]()
    except KeyError:
        raise AttributeError(name) from None


@dataclass(frozen=True)
class Provenance:
    corpus_commit: str
    corpus_dirty: bool
    oracle_commit: str
    oracle_dirty: bool


def _git(repo: Path, *args: str) -> str:
    out = subprocess.run(['git', '-C', str(repo), *args], capture_output=True)
    if out.returncode != 0:
        raise RuntimeError(f'git {" ".join(args)} in {repo}: '
                           f'{out.stderr.decode("utf-8", "replace").strip()}')
    return out.stdout.decode('utf-8').strip()


def provenance(strict: bool = True) -> Provenance:
    repo = _corpus_repo()
    corpus_commit = _git(repo, 'rev-parse', 'HEAD')
    corpus_dirty = bool(_git(repo, 'status', '--porcelain', '--',
                             str(_lua_root().relative_to(repo)),
                             str(_config().parent.relative_to(repo))))
    oracle_commit = _git(ORACLE_DIR, 'rev-parse', 'HEAD')
    oracle_dirty = bool(_git(ORACLE_DIR, 'status', '--porcelain'))
    pinned = _pinned_commit()
    if strict and pinned and corpus_commit != pinned:
        raise SystemExit(f'corpus is at {corpus_commit}, pinned at {pinned} by '
                         f'{GOLDEN_DIR / "provenance.json"}; re-record deliberately or '
                         'check the pinned commit out')
    return Provenance(corpus_commit, corpus_dirty, oracle_commit, oracle_dirty)


def corpus_files() -> list[Path]:
    """
    The Lua files luadox is pointed at, in the order its config lists them.
    Kept here rather than re-globbing from the config so the parser-level check can
    run without a config parser.
    """
    src = _lua_root()
    globs = ['*.lua', 'autogen/*.lua', 'autogen/metadata/*.lua', 'autogen/enums/*.lua',
             'math-docs/*.lua']
    files: list[Path] = []
    for pattern in globs:
        files.extend(sorted(src.glob(pattern)))
    return files


def relpath(path: Path | str) -> str:
    """Corpus-relative, forward slashes: the identity a diagnostic or declaration
    carries across implementations."""
    return os.path.relpath(os.path.abspath(str(path)),
                           str(_config_cwd())).replace(os.sep, '/')


def ensure_oracle_importable() -> None:
    if str(ORACLE_DIR) not in sys.path:
        sys.path.insert(0, str(ORACLE_DIR))
