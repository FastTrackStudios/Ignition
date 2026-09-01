# Ignition — common commands. Everything runs inside `nix develop`
# already if you use direnv; otherwise prefix with `nix develop -c`.

# Which monitor the studio goes fullscreen on. `right` is the outermost
# by position, so it survives replugging a display — an output name
# (`DP-3`) or a corner (`6560,0`) also work. THEBATTLESHIP today:
# DP-5 1440x2560 at 0,0 · DP-4 5120x1440 at 1440,0 · DP-3 2560x1440 at 6560,0.
studio_monitor := env_var_or_default("IGNITION_MONITOR", "right")

# The operator app, hot-reloading. Native renderer only: the visualizer
# is composited through Blitz's own wgpu device, which a webview does
# not have — `--renderer webview` builds and runs, but the viewport is
# empty. The first serve is a cold build into dx's own target dir.
#
# `NO_DOWNLOADS=1` is load-bearing: dx's Tailwind step otherwise fetches
# a standalone binary into ~/.cache, which will not run on NixOS. With
# it set, dx uses `which tailwindcss` — supplied by the flake.
#
# `--release` because the visualizer has to hold 120 fps. A debug Bevy
# is not a slightly slower Bevy: the transform/visibility/render-extract
# passes are all generic-heavy code that optimises away to very little
# and, unoptimised, dominate the frame. The cost is rebuild time — rsx
# hot-reload still applies without a rebuild, but a Rust change is now a
# release compile. `just studio-dev` is the fast-iteration escape hatch.
studio *ARGS:
    IGNITION_MONITOR={{studio_monitor}} NO_DOWNLOADS=1 \
        dx serve -p ignition-studio --platform desktop --renderer native \
        --hot-patch false --release {{ARGS}}

# The studio unoptimised, for when the Rust edit loop matters more than
# the frame rate. Expect the viewport to be visibly slower.
studio-dev *ARGS:
    IGNITION_MONITOR={{studio_monitor}} NO_DOWNLOADS=1 \
        dx serve -p ignition-studio --platform desktop --renderer native \
        --hot-patch false {{ARGS}}

# Compile the stylesheets once, for a plain `cargo run` — which knows
# nothing about dx's Tailwind pipeline. Both apps, because both scan the
# shared `ignition-live-ui` sources: a utility used in a shared pane has
# to be emitted into each sheet that mounts it.
tailwind:
    tailwindcss -i apps/ignition-studio/tailwind.css \
        -o apps/ignition-studio/assets/tailwind.css
    tailwindcss -i apps/ignition-live-web/tailwind.css \
        -o apps/ignition-live-web/assets/tailwind.css

# The Live view for an iPad: the same components as the desk, built
# for the browser and copied to where the studio serves them from
# (`apps/ignition-live-web/dist`, at `/` on `IGNITION_LIVE_PORT`).
# Then `IGNITION_LIVE=1 just studio` and open the URL it prints.
# See docs/ops/ipad-live.md. r[impl studio.touch.ipad]
live-web:
    NO_DOWNLOADS=1 dx build -p ignition-live-web --platform web --release
    rm -rf apps/ignition-live-web/dist
    mkdir -p apps/ignition-live-web/dist
    cp -r target/dx/ignition-live-web/release/web/public/. apps/ignition-live-web/dist/

# Windowed, for when fullscreen is in the way.
studio-windowed *ARGS:
    IGNITION_FULLSCREEN=0 just studio {{ARGS}}

# One-shot window, no hot reload — a plain cargo build, so it reuses
# target/ and starts in seconds once warm.
studio-once *ARGS:
    cargo run -p ignition-studio {{ARGS}}

# A headless render of the venue. `--overlay` adds the cue sheet.
shot OUT="/tmp/ignition.png" *ARGS:
    cargo run -p ignition-viz --bin viz -- \
        --venue data/venues/norco --snapshot {{OUT}} {{ARGS}}

# One 320×180 still per profile look (baked + authored) for the Looks
# pane's thumbnails. GPU; every look in one process, so a few seconds
# rather than a few seconds each.
#
# Through `--previews` rather than one `--snapshot` per look, because
# that is the path that writes the file name the pane looks for: a slug.
# Naming them after the look wrote "chorus full.png", which the pane
# then referred to as a `file:` URL — and a URL percent-encodes a space,
# which Blitz reads back without decoding. The picture silently did not
# load. See `ignition_viz::preview::slug`.
#
# The wide camera: dead centre of the house, far enough out to hold the
# whole stage. A look is a thing that happens across the room, so the
# frame has to contain the room.
#
# `--auto-exposure off` is the point of the whole target. With the eye
# following each frame, every look is exposed to the same average
# brightness — so a blackout and a full chorus come back looking alike,
# which is exactly the comparison the thumbnails exist to make. Fixed at
# the stage exposure, a dark look *is* dark.
#
#   just look-previews CAMERA="Wide"   to frame them differently
CAMERA := "Wide"
look-previews:
    cargo run -q -p ignition-viz --bin viz -- --venue data/venues/norco \
      --camera {{quote(CAMERA)}} --auto-exposure off --screens-off \
      --width 320 --height 180 \
      --previews data/looks/previews --preview-looks all

# One 16-frame loop per library effect, for the Effects pane's hover
# previews. Every effect renders in a single process — the rig is built
# once and each is staged onto it in turn — so the whole library is a
# couple of minutes rather than a couple of minutes *each*.
#
# The base is a blackout when the effect drives intensity itself, so the
# picture is the effect and nothing else; an effect that only moves or
# only colours is given its own fixtures lit, or there would be nothing
# to see it happen to. The white is the emitters at full rather than the
# rig's warm "Open White", because a yellow cast is the one thing in
# frame that is not the effect. The exposure is fixed, so a dim effect
# looks dim beside a bright one — which is the comparison these exist
# to make. Screens off: a logo on the upstage TVs is a distraction in a
# thumbnail this size. And no ambient fill: the default 0.15 is there so
# a window is never pitch black, but in a preview it means the room is
# plainly visible before the effect has done anything — which reads as
# the effect doing something. At zero, unlit is black.
#
#   just effect-previews EFFECTS="chase,circle"   for just a few
EFFECTS := "all"
FRAMES := "16"
effect-previews:
    cargo run -q -p ignition-viz --bin viz -- --venue data/venues/norco \
      --camera {{quote(CAMERA)}} --auto-exposure off --exposure 2.0 --ambient 0 --haze 1.6 --screens-off \
      --width 320 --height 180 \
      --previews data/effects/previews --preview-frames {{FRAMES}} \
      --preview-effects {{quote(EFFECTS)}}

# A 16-frame loop per profile macro, for the Macros pane. Same
# treatment as the effects: a macro is a little programme that plays out
# over time, so a still of one says nothing.
macro-previews:
    cargo run -q -p ignition-viz --bin viz -- --venue data/venues/norco \
      --camera {{quote(CAMERA)}} --auto-exposure off --exposure 2.0 --ambient 0 \
      --haze 1.6 --screens-off --width 320 --height 180 \
      --previews data/macros/previews --preview-frames {{FRAMES}} --preview-macros all

# The beam test: `data/songs/beam-alias.json` rendered headlessly at
# whatever quality you name, for judging what the volumetric raymarch
# does to a *steep* beam. Every mover at full white into a hazed room,
# the two ranks crossed, nothing else lit.
#
# This is the picture that decides the step count, and it is not the
# benchmark cue: a diagonal beam runs along the camera ray and hides the
# artefact, while a near-vertical one is crossed by it and shows every
# step boundary as a bead or a bulge. A step count chosen against the
# benchmark cue will be too low.
#
#   just beam-test                          medium, into /tmp
#   just beam-test QUALITY=ultra            what it looks like when it is right
#   IGNITION_FOG_STEPS=256 just beam-test   what one dial buys
QUALITY := "medium"
BEAM_OUT := "/tmp/ignition-beam-test.png"
beam-test:
    IGNITION_QUALITY={{QUALITY}} cargo run -q --release -p ignition-viz --bin viz -- \
      --venue data/venues/norco --cuelist data/songs/beam-alias.json --cue 0 \
      --camera {{quote(CAMERA)}} --width 2560 --height 1440 --snapshot {{BEAM_OUT}}
    @echo "wrote {{BEAM_OUT}}"

# The whole beam matrix, both renderers, contact-sheeted.
#
# `beam-test` is one picture and it decides one dial. This is the set:
# a cue per case — vertical, crossed, aimed at the house, long and short
# throws, a par's mouth, and the whole rig at once — rendered through
# the froxel grid and the screen-space march alike, because the two fail
# differently and each was tuned against a case the other hides.
#
#   just beam-matrix                              both sheets into /tmp
#   IGNITION_FOG_STEPS=256 just beam-matrix       what one march dial buys
#   IGNITION_FROXEL_SAMPLES=4 just beam-matrix    what one froxel dial buys
BEAM_MATRIX_OUT := "/tmp/ignition-beam-matrix"
beam-matrix:
    tools/beam_matrix.sh {{BEAM_MATRIX_OUT}} 1280 720

# Everything the library panes draw, in one go.
previews: look-previews effect-previews macro-previews

test:
    cargo test --workspace

# Every workspace member that is ours — i.e. not one of the five
# `crates/*-vendored` forks. Those are upstream code carrying one
# deliberate hunk each; policing their style would mean editing them for
# reasons the NOTICE cannot justify, and every such edit makes the diff
# against upstream harder to read at the next version bump.
vendored := "--exclude anyrender_vello --exclude bevy_pbr --exclude blitz-dom --exclude dioxus-native --exclude gdtf"

# The hygiene gate, and what CI runs. `-D warnings` is the point: a
# clippy run that only prints is a gate nothing has to pass, and this
# tree went from 0 to 55 warnings without anyone noticing. Bevy's two
# unavoidable lints are allowed at the top of `ignition-viz/src/lib.rs`,
# with the reason, rather than by loosening this line.
lint:
    cargo fmt --all --check
    cargo clippy --workspace {{vendored}} --all-targets -- -D warnings

# --- performance ---------------------------------------------------

# The studio with the frame-stage profiler on: a table in the log every
# couple of seconds, sorted by self time, saying which stage of the
# frame spent it. Release, because that is the build whose numbers mean
# anything — see docs/ops/profiling.md.
#
#   just profile ALL=1        every span in the process (needs `just profile-build-trace`)
#   just profile TRACE=/tmp/ignition-trace.json   plus a Perfetto timeline
ALL := "0"
TRACE := ""
profile *ARGS:
    IGNITION_PROFILE={{ if ALL == "1" { "all" } else { "1" } }} \
    {{ if TRACE == "" { "" } else { "IGNITION_PROFILE_TRACE=" + TRACE } }} \
        just studio {{ARGS}}

# The profiler on the *benchmark* cue, with the rig lit and the effects
# running: `data/songs/benchmark.json` on every mover, par, bar, beam
# and hazer at once, chases and figures and colour cycles going, the
# screens playing. This is the worst case and the only number worth
# quoting — an idle transport flatters the studio by a third.
#
#   just profile-bench                          the stage table
#   just profile-bench ALL=1                    down to Bevy's schedules
#   IGNITION_FOG_STEPS=96 just profile-bench    what a quality dial buys
profile-bench *ARGS:
    IGNITION_BENCH=1 just profile {{ARGS}}

# Every quality preset in turn, on the benchmark cue, with the haze
# camera each one resolved to. The table in docs/ops/profiling.md is
# this recipe's output on an RTX 4080 — re-run it on the machine that
# has to hold the show. `r[viz.quality-presets]`
quality-ladder:
    #!/usr/bin/env bash
    set -euo pipefail
    log=$(mktemp -d)/q.log
    for q in potato low medium high ultra; do
      IGNITION_QUALITY=$q IGNITION_BENCH=1 IGNITION_MONITOR={{studio_monitor}}         IGNITION_PROFILE=1 IGNITION_PROFILE_INTERVAL=6 IGNITION_LOG_FILE=$log         timeout 28 cargo run -q --release -p ignition-studio >/dev/null 2>&1 || true
      printf "%-8s " "$q"
      grep "viz.haze: resized" $log | tail -1         | grep -oE "width=[0-9]+ height=[0-9]+ scale=[0-9]+ steps=[0-9]+" | tr "\n" " "
      grep "^profile:" $log | tail -2 | grep -oE "[0-9.]+ fps" | tr "\n" " "
      echo
    done

# `just profile ALL=1` only has Bevy's per-system spans to aggregate if
# the binary was built with them. This is that binary — a full release
# rebuild of the visualizer, so expect a few minutes.
profile-build-trace *ARGS:
    IGNITION_MONITOR={{studio_monitor}} NO_DOWNLOADS=1 \
        dx serve -p ignition-studio --platform desktop --renderer native \
        --hot-patch false --release --features trace {{ARGS}}

# Everything the spans cannot see: Vello's own encoding, wgpu, the image
# decoders behind the library thumbnails, malloc. A sampling profile of
# the whole process, written where `hotspot` or `perf report` can read
# it.
#
# Needs `kernel.perf_event_paranoid <= 1`; the recipe says so rather
# than failing with an empty file. Run the studio first, then this
# against it — sampling from launch would spend the whole profile in
# shader compilation, which is a real cost but not the one that repeats
# every frame.
PERF_SECONDS := "20"
perf-studio:
    #!/usr/bin/env bash
    set -euo pipefail
    paranoid=$(cat /proc/sys/kernel/perf_event_paranoid)
    if [ "$paranoid" -gt 1 ]; then
      echo "perf_event_paranoid is $paranoid; run:" >&2
      echo "  sudo sysctl kernel.perf_event_paranoid=1" >&2
      exit 1
    fi
    pid=$(pgrep -f 'target/.*ignition-studio' | head -1)
    if [ -z "$pid" ]; then echo "no studio running — start one with 'just studio'" >&2; exit 1; fi
    echo "sampling pid $pid for {{PERF_SECONDS}}s"
    perf record --call-graph dwarf -F 999 -p "$pid" -o /tmp/ignition-perf.data \
        -- sleep {{PERF_SECONDS}}
    echo "wrote /tmp/ignition-perf.data — 'perf report -i /tmp/ignition-perf.data' or 'hotspot /tmp/ignition-perf.data'"

# The visualizer's half of the frame, headless and repeatable, through
# the same embedded route the studio uses — `crates/ignition-viz/src/bench.rs`.
# This is what a change to the renderer is judged against; the profiler
# is what tells you which change to make.
BENCH_FRAMES := "300"
bench *ARGS:
    cargo run --release -p ignition-viz --bin viz -- \
        --venue data/venues/norco --bench {{BENCH_FRAMES}} {{ARGS}}
