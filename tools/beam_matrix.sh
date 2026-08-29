#!/usr/bin/env bash
# Renders the beam-test matrix through both renderers and contact-sheets
# the pair, so an artefact can be seen in the case that provokes it
# rather than in whichever picture happened to be open.
#
#   tools/beam_matrix.sh [out_dir] [width] [height]
#
# Every extra IGNITION_* variable in the environment is passed through,
# so this is also how a dial is judged: set one, run it, compare sheets.
set -euo pipefail

OUT=${1:-/tmp/beam-matrix}
W=${2:-1280}
H=${3:-720}
CUES=$(python3 -c "import json;print(len(json.load(open('data/songs/beam-tests.json'))['cues']))")
NAMES=$(python3 -c "import json;print(' '.join(c['name'].replace(' ','-') for c in json.load(open('data/songs/beam-tests.json'))['cues']))")

mkdir -p "$OUT"
read -r -a NAME_ARR <<< "$NAMES"

for path in froxel march; do
  for ((i = 0; i < CUES; i++)); do
    name=${NAME_ARR[$i]}
    env_froxel=1
    [ "$path" = march ] && env_froxel=0
    IGNITION_FROXEL=$env_froxel \
      cargo run -q --release -p ignition-viz --bin viz -- \
        --venue data/venues/norco --cuelist data/songs/beam-tests.json \
        --cue "$i" --camera Wide --width "$W" --height "$H" \
        --snapshot "$OUT/${path}-${i}-${name}.png" >/dev/null 2>&1
    printf '  %-8s %s\n' "$path" "$name"
  done
done

# One sheet per renderer, and one that stacks the two so the same cue
# sits above itself. Built with plain appends rather than `montage`,
# which insists on a font for its labels and there is not one here.
sheet() {
  local path=$1 row=() rows=()
  local i=0
  for file in "$OUT/${path}-"*.png; do
    row+=("$file")
    i=$((i + 1))
    if [ ${#row[@]} -eq 4 ]; then
      magick "${row[@]}" +append "$OUT/.row-${path}-${i}.png"
      rows+=("$OUT/.row-${path}-${i}.png")
      row=()
    fi
  done
  if [ ${#row[@]} -gt 0 ]; then
    magick "${row[@]}" +append "$OUT/.row-${path}-last.png"
    rows+=("$OUT/.row-${path}-last.png")
  fi
  magick "${rows[@]}" -background black -gravity west -append "$OUT/sheet-${path}.png"
  rm -f "$OUT/.row-${path}-"*.png
}

sheet froxel
sheet march
magick "$OUT/sheet-froxel.png" "$OUT/sheet-march.png" \
  -background black -gravity west -append "$OUT/sheet-both.png"

echo "wrote $OUT/sheet-both.png (froxel above, march below)"
echo "cues, in order: $NAMES"
