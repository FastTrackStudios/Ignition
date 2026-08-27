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

# Compile the stylesheet once, for a plain `cargo run` — which knows
# nothing about dx's Tailwind pipeline.
tailwind:
    tailwindcss -i apps/ignition-studio/tailwind.css \
        -o apps/ignition-studio/assets/tailwind.css

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
# pane's thumbnails. GPU; a minute or so for four looks.
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
    mkdir -p data/looks/previews
    python3 -c 'import json,glob; \
      names=set(json.load(open("data/profiles/ignition.ig-profile")).get("looks",{})); \
      [names.update(json.load(open(f)).get("looks",{})) for f in glob.glob("data/profiles/*.looks.json")]; \
      print("\n".join(sorted(names)))' | while IFS= read -r name; do \
        cargo run -q -p ignition-viz --bin viz -- --venue data/venues/norco \
          --camera {{quote(CAMERA)}} --auto-exposure off --screens-off \
          --width 320 --height 180 --look-name "$name" --snapshot "data/looks/previews/$name.png"; \
      done

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

# Everything the library panes draw, in one go.
previews: look-previews effect-previews macro-previews

test:
    cargo test --workspace

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets

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
