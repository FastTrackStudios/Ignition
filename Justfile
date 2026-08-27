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

# One 320×180 still per profile look (baked + authored), house view,
# for the Looks pane's thumbnails. GPU; a minute or so for four looks.
look-previews:
    mkdir -p data/looks/previews
    python3 -c 'import json,glob; \
      names=set(json.load(open("data/profiles/ignition.ig-profile")).get("looks",{})); \
      [names.update(json.load(open(f)).get("looks",{})) for f in glob.glob("data/profiles/*.looks.json")]; \
      print("\n".join(sorted(names)))' | while IFS= read -r name; do \
        cargo run -q -p ignition-viz --bin viz -- --venue data/venues/norco --view house \
          --width 320 --height 180 --look-name "$name" --snapshot "data/looks/previews/$name.png"; \
      done

test:
    cargo test --workspace

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets
