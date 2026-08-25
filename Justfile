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
studio *ARGS:
    IGNITION_MONITOR={{studio_monitor}} \
        dx serve -p ignition-studio --platform desktop --renderer native \
        --hot-patch false {{ARGS}}

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

test:
    cargo test --workspace

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets
