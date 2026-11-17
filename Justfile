# Ignition — `just <recipe>`, from the checkout root.
#
# The lighting recipes are the port's (`features/lighting/lighting.just`
# in Studio at 8bfa77ea), written for this repo: the data and the tools
# are in `features/lighting`, and the Architect app is another repo's
# build (see `architect_app`). The desk's and the site's recipes went
# with them to `legacy/Justfile`.

set positional-arguments

# Where the data is (venues, shows, songs, GDTF) and what the tools run from.
lighting := justfile_directory() / "features/lighting"

# The recital's Live set. Not in the repo; override with IGNITION_LIVE_SET.
live_set := env_var_or_default("IGNITION_LIVE_SET", "/run/media/Development/fts-dev/recital/show/Sr. Recital Showcase Project/Sr. Recital Showcase.als")

# An Architect app that has Lighting linked in or installed (Studio
# Architect's `studio-architect`). It is not built here: name the binary.
architect_app := env_var_or_default("IGNITION_ARCHITECT", "studio-architect")

# The dev loop's build (kache keys): desktop-dev for the host triple.
_build := "--profile desktop-dev --target $(rustc -vV | sed -n 's/^host: //p')"

# List the recipes.
default:
    @just --list

# ── the show ─────────────────────────────────────────────────────────

# Regenerate JT's recital show from the Live set (data/songs/jt-recital, data/shows/jt-recital.ig-show).
author-recital:
    cd "{{lighting}}" && python3 -I tools/author_recital.py "{{live_set}}"

# Load the night and print its running order; `just check-recital --at 3122` reports a beat.
check-recital *ARGS:
    cargo run {{_build}} -p ignition -- --set "{{live_set}}" --check "$@"

# The recital in the LAB (Room 138): --clock studio follows the Studio app, --clock midi:<port> Ableton; --output drives the rig.
recital *ARGS:
    cargo run {{_build}} -p ignition -- --set "{{live_set}}" "$@"

# The visualizer on its own, e.g. `just viz --venue data/venues/room138-cbu`.
viz *ARGS:
    cd "{{lighting}}" && cargo run {{_build}} -p ignition-viz --bin viz -- "$@"

# ── the extension (features/lighting/pane, id `ignition`) ────────────

# Build the renderer and the extension's process copy.
build:
    cargo build {{_build}} -p ignition -p ignition-pane

# The Architect app with this checkout's renderer and data (Linux): open the pane with the action "Lighting: show the room".
architect *ARGS: build
    #!/usr/bin/env bash
    set -euo pipefail
    host="$(rustc -vV | sed -n 's/^host: //p')"
    target="$(cargo metadata --no-deps --format-version 1 | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
    export IGNITION_ROOT="{{lighting}}"
    export IGNITION_BIN="$target/$host/desktop-dev/ignition"
    export IGNITION_LIVE_SET="{{live_set}}"
    exec "{{architect_app}}" "$@"

# EXT_CLONE Lighting from this checkout into the running Architect app: it builds the extension's process copy from source and overrides the built-in under the same id `ignition` (rebuilt on save).
pane-dev:
    #!/usr/bin/env bash
    set -euo pipefail
    folder="$("{{architect_app}}" dev-source ignition)"
    echo "Lighting's source: $folder (edit it there; each save rebuilds)"
    "{{architect_app}}" actions run EXT_CLONE --arg id=ignition

# Back to the built-in Lighting (the development copy's folder stays).
pane-undev:
    "{{architect_app}}" extensions undev "$("{{architect_app}}" dev-source ignition)"

# Check Lighting's manifest the way the app loads it (EXT_CHECK).
pane-check:
    "{{architect_app}}" extensions check features/lighting/pane

# ── tests and lint ───────────────────────────────────────────────────

# The lighting crates' tests (the domain; the visualizer's need a GPU).
test:
    cargo nextest run -p ignition-pane -p ignition-proto -p ignition-daw-proto -p ignition-colour -p ignition-rig -p ignition-show -p ignition-effects -p ignition-playback -p ignition-core -p ignition-dmx -p ignition-daw -p ignition-fixture

# The hygiene gate. The two forks in vendor/ are not workspace members, so `--workspace` is ours only.
# Clippy reports and does not fail for now: the port left 149 findings on
# Rust 1.99.1 that need decisions (#13). `-D warnings` comes back with them fixed.
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets

# ── rendering the show to video ──────────────────────────────────────

# The recital's light show as videos, one song at a time (the lit songs, or the slugs given):
# <recital>/videos/<slug>.mp4, 1920x1080 30 fps, the Wide camera, with the song's audio
# (recordings/<slug>.ogg, else refs/). Slow and offline: an hour or more for the night.
render-recital *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    host="$(rustc -vV | sed -n 's/^host: //p')"
    cargo build --profile desktop-dev --target "$host" -p ignition
    target="$(cargo metadata --no-deps --format-version 1 | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
    cd "{{lighting}}"
    IGNITION_BIN="$target/$host/desktop-dev/ignition" \
    IGNITION_FFMPEG="${IGNITION_FFMPEG:-$(nix build --no-link --print-out-paths nixpkgs#ffmpeg-headless | head -1)/bin/ffmpeg}" \
    IGNITION_LIVE_SET="{{live_set}}" python3 -I tools/render_recital.py render "$@"

# Re-mux the recital videos whose audio changed (a newer recordings/<slug>.ogg): the picture is kept.
remux-recital *ARGS:
    cd "{{lighting}}" && \
    IGNITION_FFMPEG="${IGNITION_FFMPEG:-$(nix build --no-link --print-out-paths nixpkgs#ffmpeg-headless | head -1)/bin/ffmpeg}" \
    python3 -I tools/render_recital.py remux "$@"

# ── the visualizer's stills (from the old Justfile; run in features/lighting) ──

CAMERA := "Wide"
EFFECTS := "all"
FRAMES := "16"
QUALITY := "medium"
BEAM_OUT := "/tmp/ignition-beam-test.png"
BEAM_MATRIX_OUT := "/tmp/ignition-beam-matrix"
BENCH_FRAMES := "300"

# Four stills, one per shipped look, at a fixed exposure so a dark look is dark.
look-previews:
    cd "{{lighting}}" && cargo run -q -p ignition-viz --bin viz -- --venue data/venues/norco \
      --camera {{quote(CAMERA)}} --auto-exposure off --screens-off \
      --width 320 --height 180 \
      --previews data/looks/previews --preview-looks all

# One 16-frame loop per library effect; `just effect-previews EFFECTS="chase,circle"` for a few.
effect-previews:
    cd "{{lighting}}" && cargo run -q -p ignition-viz --bin viz -- --venue data/venues/norco \
      --camera {{quote(CAMERA)}} --auto-exposure off --exposure 2.0 --ambient 0 --haze 1.6 --screens-off \
      --width 320 --height 180 \
      --previews data/effects/previews --preview-frames {{FRAMES}} \
      --preview-effects {{quote(EFFECTS)}}

# A 16-frame loop per profile macro.
macro-previews:
    cd "{{lighting}}" && cargo run -q -p ignition-viz --bin viz -- --venue data/venues/norco \
      --camera {{quote(CAMERA)}} --auto-exposure off --exposure 2.0 --ambient 0 \
      --haze 1.6 --screens-off --width 320 --height 180 \
      --previews data/macros/previews --preview-frames {{FRAMES}} --preview-macros all

# Everything the library panes draw, in one go.
previews: look-previews effect-previews macro-previews

# The beam test: data/songs/beam-alias.json rendered headlessly, for judging the volumetric raymarch on a steep beam.
beam-test:
    cd "{{lighting}}" && IGNITION_QUALITY={{QUALITY}} cargo run -q --release -p ignition-viz --bin viz -- \
      --venue data/venues/norco --cuelist data/songs/beam-alias.json --cue 0 \
      --camera {{quote(CAMERA)}} --width 2560 --height 1440 --snapshot {{BEAM_OUT}}
    @echo "wrote {{BEAM_OUT}}"

# The visualizer's half of the frame, headless and repeatable.
bench *ARGS:
    cd "{{lighting}}" && cargo run --release -p ignition-viz --bin viz -- \
        --venue data/venues/norco --bench {{BENCH_FRAMES}} "$@"

# ── the apps that were kept (see legacy/README.md for the ones that were not) ──

# The phone app in a desktop window, at iPhone dimensions.
phone *ARGS:
    cargo run -p ignition-mobile --no-default-features --features preview \
        --bin ignition-preview "$@"

# The Live page's stylesheet (committed: `asset!()` needs it at compile time).
tailwind:
    tailwindcss -i apps/ignition-live-web/tailwind.css \
        -o apps/ignition-live-web/assets/tailwind.css

# The Live view for an iPad, built for the browser (apps/ignition-live-web/dist). Nothing in this workspace serves it now: the desk did.
live-web:
    NO_DOWNLOADS=1 dx build -p ignition-live-web --platform web --release
    rm -rf apps/ignition-live-web/dist
    mkdir -p apps/ignition-live-web/dist
    cp -r target/dx/ignition-live-web/release/web/public/. apps/ignition-live-web/dist/
