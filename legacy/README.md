# legacy — not ported

What Ignition had at 196e159 that the port to an Architect extension did
not cover and that does not build against the ported crates without real
work. Nothing here is a workspace member (`exclude = ["legacy", …]` in the
root `Cargo.toml`), nothing here is built or tested, and the manifests
still name the old paths and pins. It is kept so the code is not lost; it
is moved, not edited.

| Path | What it is | Why it is here |
|---|---|---|
| `apps/ignition-studio` | The operator desk: a multi-window Dioxus Native app with the visualizer embedded in its windows, the Live WebSocket server, MIDI/OSC remote. | It draws Bevy's texture inside a Blitz window, which needs Bevy and Blitz on one wgpu device. Bevy 0.19 is on wgpu 29; Architect v0.12.6's Blitz is on wgpu 30 (that is why the extension's pane is a process view). It is also written against Dioxus at f717a8e with `blitz-dom =0.3.0-beta.1`, `anyrender 0.11` and the three forks below; the workspace is on Dioxus 430cfb56 with the FTS Blitz fork. |
| `crates/dioxus-native-vendored` | `dioxus-native` at f717a8e with runtime multi-window (`open_window`). | Only the desk used it. The workspace patches `dioxus-native` to FastTrackStudios/vendor v0.7.1, as Architect does. |
| `crates/anyrender-vello-vendored` | `anyrender_vello` with one wgpu instance for every Blitz window. | Only the desk used it. |
| `crates/blitz-dom-vendored` | `blitz-dom` 0.3.0-beta.1 with two selection fixes. | Only the desk used it. |
| `apps/ignition-web` | The public site: landing page, the guide as a knowledge graph, the browser demo. | It takes `ssg` and `ssg-build` from `task`'s `main` (unpinned) and `view-knowledge-graph` from a `task` revision built on Architect v0.5.0 and Dioxus f717a8e; with the workspace's Dioxus that is two Dioxus copies and two `Element` types. Its guide is `docs/lighting/guides` now. Not tried against the new pins. |
| `Justfile` | The old root Justfile: the desk's, the site's and the profiling recipes. | The root `Justfile` has the recipes that still run. |

Also still pointing at these and not updated: `.github/workflows/deploy.yml`,
`nix/modules/packages/web-bundle.nix` and `deploy/chart` (the site's build
and deploy).

What was kept in the workspace, with small changes: `apps/ignition-engine`,
`apps/ignition-mobile`, `apps/ignition-live-web`, `crates/ignition-live-ui`.
`ignition-live-ui`'s `token_sheet` tests still read the desk's stylesheets
from `legacy/apps/ignition-studio/src/`.
