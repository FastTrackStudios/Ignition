#!/usr/bin/env bash
# Screenshot the studio, one image per monitor.
#
# The studio puts one window on each output (see an operator's layout in
# `data/operators/*.ig-user`), so "screenshot a window" and "screenshot
# an output" are the same job here. grim is no use under KWin — it wants
# wlr-screencopy, which KWin does not speak — so this goes through
# Spectacle for the capture and crops the outputs back out afterwards.
#
#   tools/shoot.sh                 capture whatever is on screen now
#   tools/shoot.sh --launch 20     launch the studio, wait 20s, capture, close
#   tools/shoot.sh --out DIR       where the pngs go (default: a temp dir)
#   tools/shoot.sh --width 1600    downscale each output to this wide
#   tools/shoot.sh --as cody       load that operator's saved layout
#
# Without --as the studio draws its built-in layout, not the one in
# `data/operators/<name>.ig-user` — which is a good way to spend an hour
# wondering why a layout edit changed nothing.
#
# Writes <out>/<OUTPUT>.png per monitor, plus desktop.png for the lot.
set -euo pipefail

out=""; launch=""; width=1600; operator="${IGNITION_OPERATOR:-}"
while [ $# -gt 0 ]; do
  case "$1" in
    --launch) launch="${2:-15}"; shift 2 ;;
    --as) operator="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --width) width="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -n "$out" ] || out=$(mktemp -d -t ignition-shoot-XXXXXX)
mkdir -p "$out"

pid=""
cleanup() { [ -n "$pid" ] && kill "$pid" 2>/dev/null || true; }
trap cleanup EXIT

if [ -n "$launch" ]; then
  [ -x ./target/debug/ignition-studio ] || cargo build -p ignition-studio
  IGNITION_OPERATOR="$operator" IGNITION_LOG_FILE="$out/studio.log" \
    ./target/debug/ignition-studio > "$out/studio.out" 2>&1 &
  pid=$!
  echo "studio pid $pid; giving it ${launch}s to draw"
  sleep "$launch"
  kill -0 "$pid" 2>/dev/null || { echo "studio exited early:"; tail -20 "$out/studio.out"; exit 1; }
fi

spectacle -b -n -f -o "$out/desktop.png" >/dev/null 2>&1 || true
# Spectacle returns before the file is flushed.
for _ in $(seq 20); do [ -s "$out/desktop.png" ] && break; sleep 0.5; done
[ -s "$out/desktop.png" ] || { echo "no capture came back" >&2; exit 1; }

# `Output: <n> <NAME> <uuid>` … `Geometry: <x>,<y> <w>x<h>`, in order.
kscreen-doctor -o 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' \
  | awk '/^Output:/ {name=$3} /Geometry:/ && name {print name, $2, $3; name=""}' \
  | while read -r name origin size; do
      x=${origin%,*}; y=${origin#*,}
      magick "$out/desktop.png" -crop "${size}+${x}+${y}" +repage \
        -resize "${width}x>" "$out/$name.png"
      echo "$out/$name.png  ($size at $x,$y)"
    done
