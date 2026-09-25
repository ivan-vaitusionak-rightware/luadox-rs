"""
How a fixture is configured and compared: the config file each fixture is rendered with,
and the JSON difference spec/run.py reports.
"""

from __future__ import annotations

from pathlib import Path

FIXTURES = Path(__file__).resolve().parent / 'fixtures'
SRC = FIXTURES / 'src'
SNIPPETS = FIXTURES / 'snippets'
SIDEBAR = FIXTURES / 'sidebar.tmpl.html'

# Fixtures that also register manual pages: fixture name -> [(page id, markdown file in
# src/)].  A page other than `index` links with bare section fragments, which only a
# second page can show.
MANUALS = {
    'manual': [('index', 'manual.md')],
    'manual_pages': [('index', 'manual.md'), ('guide', 'manual_pages_guide.md')],
    'naming_edge': [('index', 'naming_edge_manual.md')],
}

# Fixtures that need a second source file, which lives under src/extra/ so that it is
# not itself globbed as a fixture.  A conflict between two pages can only be written
# across two files.
EXTRA_FILES = {'conflicts': ['extra/conflicts_twin.lua']}

# A diagnostic message can quote the absolute path of the file it is about, which names
# this machine.  Only the tail below the fixture tree is behaviour, so the expectation
# carries a token instead, and the actual output gets the same rule before comparing.
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


def diff_json(a, b, path: str = '$', out: list[str] | None = None) -> list[str]:
    """Every JSON path at which two documents disagree, deepest first."""
    out = [] if out is None else out
    if type(a) is not type(b):
        out.append(f'{path}: {type(a).__name__} vs {type(b).__name__}')
        return out
    if isinstance(a, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a:
                out.append(f'{path}.{k}: missing on the left')
            elif k not in b:
                out.append(f'{path}.{k}: missing on the right')
            else:
                diff_json(a[k], b[k], f'{path}.{k}', out)
    elif isinstance(a, list):
        if len(a) != len(b):
            out.append(f'{path}: {len(a)} items vs {len(b)}')
        for i, (x, y) in enumerate(zip(a, b)):
            diff_json(x, y, f'{path}[{i}]', out)
    elif a != b:
        out.append(f'{path}: {a!r} vs {b!r}')
    return out
