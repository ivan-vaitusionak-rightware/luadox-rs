"""
Normalisations applied to both sides before a differential compare.

Every rule here exists because the two implementations are allowed to differ on it.
A rule that is not needed is a rule that hides a real difference, so each one names
what it hides.
"""

from __future__ import annotations

import re
from pathlib import Path

# Diagnostic messages quote the path luadox tried to open, which is absolute and
# therefore names the machine.  Only the tail below the corpus root is behaviour.
CORPUS_TOKEN = '<corpus>'

# The asset bundle's cache-buster is a sha256 over the assets.  Two implementations
# shipping identical assets still differ here if either re-encodes a byte, so it is
# compared as "present and well-formed", not by value.
RE_ASSETS_VERSION = re.compile(r'\?[0-9a-f]{8,64}(?=["\'])')
ASSETS_VERSION_TOKEN = '?ASSETS_VERSION'


def message(text: str, corpus_root: Path, token: str = CORPUS_TOKEN) -> str:
    """Strips the machine out of a diagnostic message.

    `token` names the tree the path is under, so one rule serves the production corpus and the
    spec fixtures without either pretending to be the other.  Python prints a filename
    through `repr()` inside an OSError, which doubles a Windows backslash, so that
    spelling is stripped as well as the plain one.
    """
    root = str(corpus_root)
    for variant in (root.replace('\\', '\\\\'), root, root.replace('\\', '/')):
        if variant and variant in text:
            text = text.replace(variant, token)
    # Whatever survived below the token still carries the host's separator.
    def slashes(m: re.Match[str]) -> str:
        return m.group(0).replace('\\\\', '/').replace('\\', '/')
    return re.sub(re.escape(token) + r'[^\s\'"]*', slashes, text)


def diagnostics(payload: dict, corpus_root: Path, token: str = CORPUS_TOKEN) -> dict:
    payload = dict(payload)
    payload['diagnostics'] = [dict(e, message=message(e['message'], corpus_root, token))
                              for e in payload['diagnostics']]
    return payload
