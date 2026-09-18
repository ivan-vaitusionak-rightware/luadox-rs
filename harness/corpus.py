"""
Where the differential harness finds its inputs.

Every path here is a pinned input, not a default: the corpus is a specific the production project
worktree at a specific commit and the oracle is a specific commit of the Python fork.
A run that cannot confirm both refuses to produce golden data, because golden data
recorded against an unknown tree is worse than none.
"""

from __future__ import annotations

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

# The production tree the docs build consumes.  Read-only for this project.
CORPUS_REPO = Path(r'<corpus>')
CORPUS_COMMIT = '6fa35c5ee93c912fb7062f976e33afb62151d416'

# luadox resolves the globs in its config against the current directory, not against
# the config file, so the config is only meaningful together with this working
# directory.  generate_api_docs.py runs it from exactly here.
CONFIG = CORPUS_REPO / 'docs' / 'tools' / 'luadox' / 'config' / 'engine-lua-api.conf'
CONFIG_CWD = CORPUS_REPO / 'docs' / 'tools' / 'doxygen'


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
    corpus_commit = _git(CORPUS_REPO, 'rev-parse', 'HEAD')
    corpus_dirty = bool(_git(CORPUS_REPO, 'status', '--porcelain', '--',
                             'Engine/source/lua', 'docs/tools/luadox'))
    oracle_commit = _git(ORACLE_DIR, 'rev-parse', 'HEAD')
    oracle_dirty = bool(_git(ORACLE_DIR, 'status', '--porcelain'))
    if strict and corpus_commit != CORPUS_COMMIT:
        raise SystemExit(f'corpus is at {corpus_commit}, pinned at {CORPUS_COMMIT}; '
                         'update CORPUS_COMMIT deliberately or check the tree out')
    return Provenance(corpus_commit, corpus_dirty, oracle_commit, oracle_dirty)


def corpus_files() -> list[Path]:
    """
    The 580 Lua files luadox is pointed at, in the order its config lists them.
    Kept here rather than re-globbing from the config so the parser-level check can
    run without a config parser.
    """
    src = CORPUS_REPO / 'Engine' / 'source' / 'lua' / 'src'
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
                           str(CONFIG_CWD)).replace(os.sep, '/')


def ensure_oracle_importable() -> None:
    if str(ORACLE_DIR) not in sys.path:
        sys.path.insert(0, str(ORACLE_DIR))
