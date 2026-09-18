# luadox-rs — Phase 0 and Phase 1

Phase 0 (oracle and differential harness) and Phase 1 (parser spike, go/no-go) of the
plan to rewrite luadox in Rust. Phases 2–5 are not started.

Everything here reads two trees and writes nothing to either:

| input | what it is | pinned at |
|---|---|---|
| `<luadox-fork>` | the Python fork | `origin/luals-all` + `oracle-patches/` |
| `<corpus>` | the production corpus, 580 Lua files | `6fa35c5ee93c912fb7062f976e33afb62151d416` |

## What the acceptance criterion is

A modernisation, not a bug-compatible clone, so the criterion is split in two:

1. **Rendered output must be exact.** html, LuaLS definitions, json. They are the safety
   net proving no content was lost in translation. An unexplained byte difference is a
   failure.
2. **Everything around it is expected to improve** and is not held to parity:
   diagnostics (which fire, their wording, their category), exit codes, error handling,
   and the three known parser defects. Those are **fixed**, not reproduced, and reported
   as a delta.

`harness/differ.py` sorts every difference into exactly three buckets:

```
output diff, unexplained             -> FAILURE, investigate
output diff, named in improvements   -> recorded as an improvement
diagnostics / exit-code difference   -> DELTA, reported, never a failure
```

`harness/improvements.toml` is the improvements list. It has **no wildcard**: an L2 entry
names every file it explains, an L1 entry names the JSON path fragment every difference
it explains must contain. An entry that stops explaining anything is a failure too — a
stale entry hides a regression as well as a missing one does.

## Rebuilding the oracle

The plan says "port `luals-all`". `luals-all` turns out to be **stale relative to the PR
lineage on the same fork** — see *Findings* below — so the oracle is `origin/luals-all`
plus five commits, exported to `oracle-patches/`:

```sh
git clone --no-hardlinks <luadox-fork> oracle
cd oracle && git checkout -b oracle origin/luals-all && git am ../oracle-patches/*.patch
```

The result is `6b6ccf11046a`, recorded in `golden/provenance.json`.

## Running it

```sh
python harness/oracle_run.py --levels 0,1,2 --record   # golden run, ~13 s
python harness/dump_decls.py                           # declaration dump
python harness/differ.py _build/oracle _build/candidate
python harness/compare_decls.py                        # parser spike vs oracle

cd spike && cargo test --release && cargo clippy --release --all-targets -- -D warnings
./target/release/luadox-spike.exe <corpus>/lua/src ../_build/spike/decls.json
```

`--record` copies the digest manifest, the diagnostics and the provenance into `golden/`.
The 33 MB of rendered output is not checked in: a manifest says *that* something moved,
the run that produced it says what.

## Layout

```
harness/corpus.py         pinned inputs; refuses to record from an unpinned tree
harness/oracle_run.py     runs the oracle at L0/L1/L2, produces the manifest
harness/dump_decls.py     the parser-level property check, Python side
harness/normalize.py      what both sides are allowed to differ on, each rule named
harness/differ.py         the three-bucket classifier
harness/compare_decls.py  grades the Rust spike against the Python declaration dump
harness/improvements.toml the improvements list
golden/                   manifest.sha256 (594 lines), diagnostics.json, decls.json
oracle-patches/           the five commits that make the oracle out of origin/luals-all
spike/                    the Phase 1 parser spike (tree-sitter-lua, rustc 1.83)
```
