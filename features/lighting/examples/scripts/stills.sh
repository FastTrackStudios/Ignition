#!/usr/bin/env bash
# Stills of what the example scripts made, drawn by the visualizer (`viz`,
# headless). Run after run.sh, on the same data root.
#
#   examples/scripts/stills.sh <ignition-program> <viz> <root> <out dir>
#
# `viz` does not read the library, so each song is baked first
# (IGNITION_SONG_BAKE: slots and library entries written in).
set -euo pipefail
prog=$1 viz=$2 root=$3 out=$4
export IGNITION_ROOT=$root
tmp=$(mktemp -d)
mkdir -p "$out"
act() { "$prog" "$@" > /dev/null; }
shot() { # venue cue-list cue file [camera] [seconds into the cue's effects]
  (cd "$root" && "$viz" --venue "data/venues/$1" --cuelist "$2" --cue "$3" --camera "${5:-Wide}" \
    --width 1280 --height 720 ${6:+--time "$6"} --snapshot "$out/$4" > "$tmp/viz.log" 2>&1) ||
    { echo "viz failed for $4"; tail -3 "$tmp/viz.log"; exit 1; }
  echo "$out/$4"
}
# The recital's Drive To LA, as programmed (its own palette), before any script.
git_root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
IGNITION_ROOT=$git_root/features/lighting "$prog" IGNITION_SONG_BAKE "{\"song\":\"Drive To LA\",\"to\":\"$tmp/dtla-own.json\"}" > /dev/null
rm -rf "$git_root/features/lighting/data/.history"
shot room138-cbu "$tmp/dtla-own.json" 4 dtla-chorus-recital.png
# …and after 03-reskin-set.luau (every song on Ice).
act IGNITION_SONG_BAKE "{\"song\":\"Drive To LA\",\"to\":\"$tmp/dtla-ice.json\"}"
shot room138-cbu "$tmp/dtla-ice.json" 4 dtla-chorus-ice.png
# Fire as 02-program-song.luau programmed it, in each palette of 01-library.luau.
for palette in Sunset Ice Neon; do
  act IGNITION_PALETTE_USE "{\"song\":\"Fire\",\"palette\":\"$palette\"}"
  act IGNITION_SONG_BAKE "{\"song\":\"Fire\",\"to\":\"$tmp/fire-$palette.json\"}"
  shot room138-cbu "$tmp/fire-$palette.json" 4 "fire-chorus-$(echo "$palette" | tr 'A-Z' 'a-z').png"
done
# The environment of 04-environment.luau: a show on it, made here with the
# same actions, to stand the rig up.
s='"show":"club","song":"Demo"'
"$prog" IGNITION_SONG_CREATE '{"show":"club","title":"Demo","env":"club-stage","sections":[{"name":"Verse","bars":8},{"name":"Chorus","bars":8},{"name":"Bridge","bars":8},{"name":"Solo","bars":8},{"name":"Outro","bars":8}]}' > /dev/null 2>&1 || true # there already on a second run
act IGNITION_SECTION_APPLY "{$s,\"section\":\"*\",\"parts\":[{\"target\":\"Key\",\"dimmer\":0.7,\"color\":\"Warm White\"},{\"target\":\"Wash\",\"dimmer\":0.6,\"color\":\"Deep Blue\"},{\"target\":\"Movers\",\"dimmer\":1.0,\"color\":\"Cyan\",\"focus\":\"Stage\"}]}"
act IGNITION_FOCUS_ON "{$s,\"env\":\"club-stage\",\"section\":\"Verse\",\"target\":\"Movers\",\"person\":\"Singer\"}"
act IGNITION_FOCUS_ON "{$s,\"env\":\"club-stage\",\"section\":\"Outro\",\"target\":\"Movers\",\"person\":\"Drummer\"}"
# One effect of the profile (`tilt wave`), three trick settings (the presets
# of 01-library.luau are the recital's library; this show writes them in place).
wave() { # section, tricks as JSON
  act IGNITION_SECTION_APPLY "{$s,\"section\":\"$1\",\"parts\":[{\"target\":\"Movers\",\"focus\":{\"x\":0,\"y\":-2.5,\"z\":0.4}},{\"target\":\"Movers\",\"effect\":\"tilt wave\",\"tricks\":$2}]}"
}
wave Chorus '{"phase":{"from":0,"to":360}}'
wave Bridge '{"tricks":["Mirror"],"phase":{"from":0,"to":360}}'
wave Solo '{"tricks":[{"Block":2}],"phase":{"from":0,"to":360}}'
act IGNITION_SONG_BAKE "{$s,\"to\":\"$tmp/demo.json\"}"
shot club-stage "$tmp/demo.json" 0 club-focus-singer.png
shot club-stage "$tmp/demo.json" 4 club-focus-drummer.png
shot club-stage "$tmp/demo.json" 1 club-tricks-chase.png Wide 0.5
shot club-stage "$tmp/demo.json" 2 club-tricks-mirrored.png Wide 0.5
shot club-stage "$tmp/demo.json" 3 club-tricks-blocks.png Wide 0.5
shot club-stage "$tmp/demo.json" 0 club-top.png Top
