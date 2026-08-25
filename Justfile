# Ignition — common commands. Everything runs inside `nix develop`
# already if you use direnv; otherwise prefix with `nix develop -c`.

# The operator app, hot-reloading. Native renderer only: the visualizer
# is composited through Blitz's own wgpu device, which a webview does
# not have — `--renderer webview` builds and runs, but the viewport is
# empty. The first serve is a cold build into dx's own target dir.
studio *ARGS:
    dx serve -p ignition-studio --platform desktop --renderer native \
        --hot-patch false {{ARGS}}

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
