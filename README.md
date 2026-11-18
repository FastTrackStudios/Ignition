# Ignition

A lighting-and-visual control system: DMX/Art-Net/sACN console, GPU-accelerated
real-time 3D visualizer, and video/projection mapping, in one Rust/Dioxus
application — desktop and web, from the same codebase.

**Status**: pre-alpha; see `docs/lighting/`.

## Why

FastTrackStudio (the sibling repo) is a music-production and live-worship
platform that already owns the *musical* structure of a show: song sections,
tempo maps, chord detection, setlists, a guide/count-in engine. No lighting
console on the market has access to that. Ignition's long-term bet is cues
that write themselves from the song — see
[`docs/lighting/research/lighting-console-landscape.md`](docs/lighting/research/lighting-console-landscape.md)
for the full argument.

Short-term, Ignition needs to be a *good console* first: patch, groups,
referenced presets, tracking cues, phaser-style effects, a real 3D visualizer,
and now — projection mapping onto physical screens in the room, so video
content and lighting live in one system instead of a console plus a separate
Resolume box.

## Hard requirements

- **3D visualizer**, GPU-accelerated (wgpu), not a toy — a working tool the
  operator can point-and-click focus fixtures in, the way Augment3d and MA 3D
  are used.
- **Easy to use.** The primary operator is a volunteer, not a programmer.
- **Multi-window**, native desktop and web, from one Dioxus codebase.
- **Projection mapping** onto physical surfaces (TVs, screens) in the venue —
  Resolume-style slices/mapping, but living inside the same show file and the
  same patch as the lights, not a second application to sync.
- **Integrates with FastTrackStudio**: control surfaces, the session/setlist
  engine, and eventually the auto-generated-cues pipeline.

## Repo layout

Ignition is an [Architect](https://github.com/FastTrackStudios/architect)
extension (id `ignition`, `features/lighting/pane/extension.toml`): linked
into an Architect build, its view is a GPU pane drawn by the `ignition`
program as a process view; it also builds as a process extension. The
layout is the one the port had inside Studio (`features/lighting`,
`apps/ignition`), so the two agree on every path.
[`features/lighting/README.md`](features/lighting/README.md) is the guide
to running it.

```
features/lighting/       the lighting crates, one directory each
  proto                  ignition-proto — the types every crate shares
  daw-proto              ignition-daw-proto — musical time: bars, tempo map, sections
  colour · rig · show · effects · playback
                         the domain: colour presets, selections and tricks,
                         cues and recipes, the effect library, the priority stack
  core                   ignition-core — the domain as one namespace
  dmx                    ignition-dmx — sockets, rate limiting, sACN and Art-Net
  daw                    ignition-daw — a song's shape from a REAPER project or
                         an Ableton Live set, the cue generator, the playhead
  fixture · profile · viz
                         the fixture library, the frame profiler, the Bevy visualizer
  pane                   ignition-pane — the Architect extension
  data/                  venues, profiles, songs, shows, the GDTF library
  tools/                 GDTF generation, the recital's author and renderer,
                         the spec-coverage reporter
apps/
  ignition               the renderer the extension starts (`--process-view`),
                         and tooling: `render`, `--check`, `--at`, a debug runner
  ignition-engine        a smoke check that a venue's files parse
  ignition-mobile        the Ignition iPhone app
  ignition-live-web      the Live panes, built for a browser/iPad
crates/
  ignition-live-ui       Dioxus panes shared by the Live page (and the old desk)
vendor/                  one-hunk forks of `bevy_pbr` and `gdtf` (FTS-PATCH.md each)
legacy/                  not ported, not built: the operator desk and the public
                         site (legacy/README.md)
docs/lighting/           guides/ · spec/ (normative `r[topic.id]` requirements) ·
                         domain/ · research/ · ops/
nix/                     flake-parts modules: toolchain, dx, dev + CI shells
```

The domain's layering is one-way and was derived, not decided —
`colour` and `rig` are leaves, `show` is the mutually-recursive core
that cannot be split further without redesigning what a cue means, and
`effects` then `playback` sit above it. `ignition-core` re-exports the
lot so an application sees one flat namespace.

**What Ignition takes from Studio.** The "Studio crates" block of the root
`Cargo.toml` lists every crate taken from `FastTrackStudios/studio`, pinned
to one revision, with what each is for. The domain crates (`proto`,
`daw-proto`, `colour`, `rig`, `show`, `effects`, `playback`, `core`, `dmx`,
`profile`) take none.

The compiler is `rust-toolchain.toml`'s (1.99.1); `.cargo/config.toml` is
the fleet's. `just` lists the recipes.

Not yet built: `ignition-video` (media decode for mapped surfaces) and
the Graphics/Video studio modes. Lights first, deliberately
(`r[studio.modes.lights-first]`).

## Licence

GPL-3.0-or-later, matching FastTrackStudio. See `LICENSE`.

## Related

- [`FastTrackStudio`](https://github.com/FastTrackStudios/FastTrackStudio) — the
  audio/music product this integrates with.
- [`eos-toolkit`](https://github.com/Codys-Wright/eos-toolkit) — Cody's own
  ETC Eos OSC scripting toolkit; its `docs/` is the best single source of
  ground-truth Eos behaviour used in this project's research.
