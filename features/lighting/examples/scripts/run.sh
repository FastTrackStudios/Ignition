#!/usr/bin/env bash
# Runs the example scripts through an Architect app's SCRIPT_RUN, headless
# (no window): the same door a script in the app, the CLI and MCP use.
#
#   examples/scripts/run.sh <app> <root> [script.luau ...]
#
# <app> is a build of Studio with Lighting linked in (studio-architect);
# <root> the lighting data root to work on (a COPY of features/lighting:
# the scripts write). Each script's answer is printed.
set -euo pipefail
app=$1 root=$2; shift 2
here=$(cd "$(dirname "$0")" && pwd)
[ $# -gt 0 ] || set -- "$here"/0*.luau
export IGNITION_ROOT=$root
for script in "$@"; do
  echo "== $(basename "$script")"
  python3 -I -c 'import json,sys; print(json.dumps({"source": open(sys.argv[1]).read(), "name": sys.argv[1].rsplit("/",1)[-1]}))' "$script" |
    "$app" actions --headless run SCRIPT_RUN --json - |
    python3 -I -c '
import json, sys
d = json.load(sys.stdin)
r = d.get("result", d)
r = json.loads(r) if isinstance(r, str) else r
print(json.dumps(r, indent=1, ensure_ascii=False))
for line in d.get("log") or []: print("  log:", line)'
done
