# dioxus-native — vendored copy

Upstream: https://github.com/DioxusLabs/dioxus, `packages/native`, at
revision `f717a8e184a522d078b70bb4b4d62a5f9a99ddfc` (version
`0.8.0-alpha.0`). Licensed MIT OR Apache-2.0 — see `LICENSE-MIT` and
`LICENSE-APACHE`, copied from that checkout.

The workspace `[patch."https://github.com/DioxusLabs/dioxus"]` table
points every `dioxus-native` dependency at this directory. The version
is unchanged so the patch applies; `dioxus-native-dom` and the other
dioxus crates still come from the pinned git revision.

## What changed (every hunk is marked `IGNITION PATCH`)

- `Cargo.toml`: `workspace = true` dependencies rewritten as explicit
  git/crates.io pins matching the upstream workspace at that revision.
- `dioxus_application.rs`: two new `DioxusNativeEvent` variants,
  `NewWindow { attributes, root, on_created, on_closed }` and
  `CloseWindow(WindowId)`; a `WindowFactory` (net provider, HTML parser,
  wgpu features/limits) so a runtime window is built like the launch
  window; the launch window's setup factored into `init_window` and
  reused; embedder events taken by value so `NewWindow` can carry a
  `FnOnce`; window close routed through one path that runs `on_closed`.
- `hooks.rs`: `open_window`, `open_window_with_props`, `open_window_via`,
  `close_window`, `close_window_via`, `use_shell_proxy`, `WindowOpen`.
- `lib.rs`: `BlitzShellProxy` provided as a root context on every
  window; the factory built in `launch_cfg_with_props`; re-exports.

Spec: `r[studio.windows.implementation]` in `docs/spec/studio.md`.
- `dioxus_renderer.rs`, `dioxus_application.rs`: frame-stage `tracing`
  spans on the target `ignition::profile` — `blitz.render` / `blitz.scene`
  around the window renderer's two halves, and `loop.window_event` /
  `loop.wake` / `loop.wait` / `loop.new_events` around the winit handler.
  Behind this crate's `tracing` feature, and disabled by the log filter
  unless `IGNITION_PROFILE` is set, so the cost when off is a branch on a
  static. Without them the studio's frame stages accounted for barely
  half of a frame — `loop.wake` turned out to be most of the rest. See
  `r[studio.profiling]`, `docs/ops/profiling.md` and
  `crates/ignition-profile`.
