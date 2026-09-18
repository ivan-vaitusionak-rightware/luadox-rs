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
RE_ASSETS_VERSION = re.compile(r'\?[0-9a-f]{7,64}(?=["\'])')
ASSETS_VERSION_TOKEN = '?ASSETS_VERSION'


def newlines(data: bytes) -> bytes:
    """
    Reduces CRLF to LF before a digest.

    The Python opens every output file in *text mode*, so its bytes are a function of the
    host: LF on Linux, CRLF on Windows, and `\\r\\r\\n` for a default template whose
    checkout is CRLF (see spec/html.md section 11).  The recorded manifest would otherwise
    bake this machine into the expectation.  A port writes LF unconditionally and is
    compared after this rule, which is what spec/fixtures/expected/ already stores.
    """
    # A CR CR LF run first, and as ONE line break.  It is what a default template
    # produces on Windows -- read as bytes out of a CRLF checkout, then written
    # through text mode, which turns the LF of each CRLF into CRLF again.  Reducing
    # it in two steps would make it two lines, inventing a difference rather than
    # removing one: six lines of search.tmpl.html are the only place it occurs in the
    # corpus, and they are why search.html was the last page to differ.
    crcrlf = bytes([13, 13, 10])
    crlf = bytes([13, 10])
    cr = bytes([13])
    lf = bytes([10])
    for run in (crcrlf, crlf, cr):
        data = data.replace(run, lf)
    return data


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
