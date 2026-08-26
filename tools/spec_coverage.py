#!/usr/bin/env python3
"""Requirement coverage, without the tracey daemon.

Lists every `r[<id>]` in docs/spec and whether code carries an
`r[impl <id>]` / `r[verify <id>]` for it. Flags tags naming ids no spec
defines. `tracey web` / `tracey mcp` do this properly; this is the
one-file version for a CI step or a quick look.

    python3 tools/spec_coverage.py            # summary per topic
    python3 tools/spec_coverage.py --uncovered
    python3 tools/spec_coverage.py --untested
"""
import glob, re, sys, collections

ROOT = __import__("os").path.dirname(__import__("os").path.dirname(__import__("os").path.abspath(__file__)))
spec = {}
for f in sorted(glob.glob(f"{ROOT}/docs/spec/*.md")):
    for m in re.finditer(r"^r\[([a-z0-9.-]+)\]", open(f).read(), re.M):
        spec[m.group(1)] = f.split("/")[-1]
impl, verify, unknown = collections.defaultdict(list), collections.defaultdict(list), []
files = [p for pat in ("crates/**/*.rs", "apps/**/*.rs") for p in glob.glob(f"{ROOT}/{pat}", recursive=True) if "/target/" not in p]
for f in files:
    for m in re.finditer(r"r\[(impl|verify) ([a-z0-9.-]+)\]", open(f).read()):
        rel = f[len(ROOT) + 1 :]
        (impl if m.group(1) == "impl" else verify)[m.group(2)].append(rel)
        if m.group(2) not in spec:
            unknown.append((rel, m.group(2)))

mode = sys.argv[1] if len(sys.argv) > 1 else "--summary"
if mode == "--uncovered":
    for i in sorted(spec):
        if i not in impl: print(i)
elif mode == "--untested":
    for i in sorted(spec):
        if i not in verify: print(i)
else:
    by = collections.defaultdict(lambda: [0, 0, 0])
    for i, f in spec.items():
        t = by[f]; t[0] += 1; t[1] += i in impl; t[2] += i in verify
    print(f"{'spec':22} {'reqs':>5} {'impl':>5} {'verify':>6}")
    for f, (n, a, b) in sorted(by.items()):
        print(f"{f:22} {n:5} {a:5} {b:6}")
    n = len(spec); print(f"{'total':22} {n:5} {sum(i in impl for i in spec):5} {sum(i in verify for i in spec):6}")
if unknown:
    print("\nUNKNOWN ids (no spec defines them):")
    for rel, i in unknown: print(f"  {rel}: {i}")
    sys.exit(1)
