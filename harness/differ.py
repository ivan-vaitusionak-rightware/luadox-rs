"""
Compares a candidate luadox run against the Python oracle's, and sorts every difference
into one of three buckets.

The rewrite is a modernisation, not a bug-compatible clone, so the acceptance criterion
is split:

  1. RENDERED OUTPUT -- html, luals, json -- must be exact. It is the safety net that
     proves no content was lost in translation. An unexplained byte difference is a
     failure.

  2. EVERYTHING AROUND IT is expected to improve and is not held to parity: which
     diagnostics fire, how they are worded, which category they carry, the exit code,
     and the three known parser defects. Those are reported as a delta.

Hence the buckets:

    output diff, unexplained            -> FAILURE, investigate
    output diff, named in improvements  -> recorded as an improvement
    diagnostics / exit code difference  -> DELTA, reported, never a failure

An improvements entry whose difference has gone away is also a failure: a stale entry
hides a regression just as well as a missing one.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import corpus  # noqa: E402

try:
    import tomllib
except ModuleNotFoundError:  # Python 3.10 ships tomli instead
    import tomli as tomllib


def load_improvements(path: Path) -> list[dict]:
    if not path.exists():
        return []
    return tomllib.loads(path.read_text(encoding='utf-8')).get('improvement', [])


def read_manifest(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for line in path.read_text(encoding='utf-8').splitlines():
        if not line.strip():
            continue
        digest, _, name = line.partition('  ')
        out[name] = digest
    return out


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


def improvement_for_file(entries: list[dict], path: str) -> dict | None:
    """The L2 entry that names this output file. There is no wildcard by design."""
    for entry in entries:
        if entry.get('level') == 'L2' and path in (entry.get('paths') or []):
            return entry
    return None


def explain_json_paths(entries: list[dict], paths: list[str]) -> tuple[list[dict], list[str]]:
    """Splits differing JSON paths into (entries that explained some, paths left over)."""
    used: list[dict] = []
    remaining = list(paths)
    for entry in entries:
        if entry.get('level') != 'L1':
            continue
        fragment = entry.get('json_path_contains')
        if not fragment:
            continue
        matched = [p for p in remaining if fragment in p]
        if matched:
            used.append(entry)
            remaining = [p for p in remaining if fragment not in p]
    return used, remaining


def compare_diagnostics(oracle: Path, candidate: Path) -> int:
    """The diagnostics delta. Always informational: it is the point of the rewrite."""
    left = json.loads(oracle.read_text(encoding='utf-8'))
    right = json.loads(candidate.read_text(encoding='utf-8'))

    def keyed(payload):
        return {(e['category'], e['file'], e['line'], e['message']) for e in
                payload['diagnostics']}

    a, b = keyed(left), keyed(right)
    gained, lost = sorted(b - a), sorted(a - b)
    print('\nDIAGNOSTICS DELTA (never a failure)')
    print(f'  python {len(a)}, candidate {len(b)}')
    if not gained and not lost:
        print('  identical')
    for label, items in (('now reported', gained), ('no longer reported', lost)):
        if not items:
            continue
        kinds = Counter(k[0] for k in items)
        print(f'  {label}: {len(items)} '
              f'({", ".join(f"{v} {k}" for k, v in kinds.most_common())})')
        for cat, file, line, msg in items[:8]:
            print(f'     [{cat}] {file}:{line} {msg}')
        if len(items) > 8:
            print(f'     ... {len(items) - 8} more')
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('oracle', type=Path, nargs='?',
                    default=corpus.BUILD_DIR / 'oracle')
    ap.add_argument('candidate', type=Path, nargs='?',
                    default=corpus.BUILD_DIR / 'candidate')
    ap.add_argument('--improvements', type=Path,
                    default=Path(__file__).resolve().parent / 'improvements.toml')
    ap.add_argument('--determinism', action='store_true',
                    help='both sides are the oracle: check the run reproduces itself, '
                         'and do not expect any improvement to apply')
    args = ap.parse_args()

    improvements = [] if args.determinism else load_improvements(args.improvements)
    failures: list[str] = []
    recorded: list[tuple[str, str]] = []
    matched_entries: set[int] = set()

    # L1: the json renderer is the IR made visible. Compare it structurally so a
    # difference lands on a JSON path, not on a byte offset.
    left_json = args.oracle / 'doc.json'
    right_json = args.candidate / 'doc.json'
    print('L1  structured parity (doc.json)')
    if not right_json.exists():
        print('  candidate produced no doc.json -- skipped')
    else:
        paths = diff_json(json.loads(left_json.read_text(encoding='utf-8')),
                          json.loads(right_json.read_text(encoding='utf-8')))
        if not paths:
            print('  identical')
        else:
            used, unexplained = explain_json_paths(improvements, paths)
            for entry in used:
                matched_entries.add(id(entry))
                recorded.append((f'doc.json ({entry["json_path_contains"]})',
                                 entry['fix']))
            explained = len(paths) - len(unexplained)
            print(f'  {len(paths)} differing JSON paths: {explained} explained, '
                  f'{len(unexplained)} not')
            if unexplained:
                failures.append(
                    f'L1 doc.json: {len(unexplained)} unexplained differences')
                for p in unexplained[:10]:
                    print(f'  ! {p}')
                if len(unexplained) > 10:
                    print(f'  ! ... {len(unexplained) - 10} more')

    # L2: byte parity over the rendered site and the LuaLS definitions.
    print('\nL2  byte parity (html, luals)')
    left_manifest = read_manifest(args.oracle / 'manifest.sha256')
    candidate_manifest = args.candidate / 'manifest.sha256'
    if not candidate_manifest.exists():
        print('  candidate produced no manifest -- skipped')
    else:
        right_manifest = read_manifest(candidate_manifest)
        names = {n for n in set(left_manifest) | set(right_manifest)
                 if not n.endswith(('doc.json', 'diagnostics.json'))}
        differing = sorted(n for n in names
                           if left_manifest.get(n) != right_manifest.get(n))
        print(f'  {len(names) - len(differing)}/{len(names)} files identical')
        unexplained = []
        for name in differing:
            entry = improvement_for_file(improvements, name)
            if entry is None:
                unexplained.append(name)
            else:
                matched_entries.add(id(entry))
                recorded.append((name, entry['fix']))
        if unexplained:
            failures.append(f'L2: {len(unexplained)} unexplained file difference(s)')
            for name in unexplained[:15]:
                print(f'  ! {name}: {left_manifest.get(name, "absent")[:12]} vs '
                      f'{right_manifest.get(name, "absent")[:12]}')
            if len(unexplained) > 15:
                print(f'  ! ... {len(unexplained) - 15} more')

    # An entry that no longer explains anything is as wrong as a missing one -- except
    # where it says outright that the corpus does not exercise it yet.
    for entry in improvements:
        if id(entry) in matched_entries or entry.get('present_in_corpus') is False:
            continue
        failures.append(f'improvements.toml: "{entry["fix"]}" no longer differs; '
                        'remove the entry')

    if recorded:
        print('\nIMPROVEMENTS (output diffs justified by a named fix)')
        seen = set()
        for name, fix in recorded:
            if fix not in seen:
                seen.add(fix)
                print(f'  * {fix}')
            print(f'      {name}')

    left_diag = args.oracle / 'diagnostics.json'
    right_diag = args.candidate / 'diagnostics.json'
    if left_diag.exists() and right_diag.exists():
        compare_diagnostics(left_diag, right_diag)

    print()
    if failures:
        print(f'FAILED: {len(failures)} unexplained difference(s)')
        for f in failures:
            print(f'  {f}')
        return 1
    print('OK: rendered output matches, or every difference is a named fix')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
