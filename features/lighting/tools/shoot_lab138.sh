#!/usr/bin/env bash
# The Lab 138 acceptance frames (docs/lighting/lab-138-room.md).
#
#   tools/shoot_lab138.sh <out dir> [photos dir]
#
# Renders the room from where five of its photographs were taken, with the
# house lights on and the stage lights that were in the room that day
# (`environment.json`'s `found` group). The rig is left out, because it was
# not up when the photographs were taken: the frames are of the room.
#
# With a photos dir (IMG_5782.jpg ...; not in the repository), also writes
# <out>/pairs/: each photograph beside its render. Those show people and
# stay out of git.
#
# Needs the model (tools/build_lab138_room.py) and a built `viz`:
#   IGNITION_VIZ=<path to viz>, or it is run through cargo.
set -euo pipefail
cd "$(dirname "$0")/.."
out=$(realpath -m "${1:?usage: tools/shoot_lab138.sh <out dir> [photos dir]}")
photos=${2:-}
mkdir -p "$out"

venue=$(mktemp -d -t lab138-room-XXXXXX)
trap 'rm -rf "$venue"' EXIT
src=data/venues/room138-cbu
cp "$src/environment.json" "$src/cameras.json" "$src/screens.json" "$venue/"
ln -s "$(realpath "$src/model")" "$venue/model"
echo '[]' > "$venue/fixtures.json"
python3 -I - "$src" "$venue" <<'PY'
import json, sys
src, out = sys.argv[1], sys.argv[2]
room = [r for r in json.load(open(f"{src}/room.json")) if not r["name"].startswith("Column")]
props = [p for p in json.load(open(f"{src}/props.json")) if p["name"] == "Drum Kit"]
json.dump(room, open(f"{out}/room.json", "w"))
json.dump(props, open(f"{out}/props.json", "w"))
PY

viz=(${IGNITION_VIZ:-cargo run -q -p ignition-viz --bin viz --})
# ImageMagick is not in the dev shell; it is only needed for the pairs.
if command -v magick >/dev/null; then magick=(magick); else magick=(nix shell nixpkgs#imagemagick -c magick); fi
shoot() { # <file> <camera> [args...]
  local file=$1 camera=$2
  shift 2
  "${viz[@]}" --venue "$venue" --camera "$camera" --width 1600 --height 1200 \
    --screens-off --haze 0 --settle-frames 60 --snapshot "$out/$file" "$@" >/dev/null 2>&1
  [ -s "$out/$file" ] || { echo "no frame for $camera" >&2; exit 1; }
  echo "$out/$file"
}

# photograph : camera
views=("5797:Curtain" "5790:Stage" "5782:Console" "5786:Stage Left" "5784:Lounge")
for view in "${views[@]}"; do
  photo=${view%%:*}; camera=${view#*:}; name=$(echo "$camera" | tr 'A-Z ' 'a-z-')
  shoot "$name.png" "Photo - $camera" --house on --env-light found=on
  if [ -n "$photos" ]; then
    mkdir -p "$out/pairs"
    "${magick[@]}" "$photos/IMG_$photo.jpg" -resize 800x600! \
      \( "$out/$name.png" -resize 800x600! \) +append -quality 88 "$out/pairs/$name.jpg"
  fi
done
# The same stage with the house lights off: what the curtain does with colour.
shoot "curtain-house-off.png" "Photo - Curtain" --house off --env-light found=on
