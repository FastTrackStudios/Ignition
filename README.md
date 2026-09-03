# Ignition

A lighting-and-visual control system: DMX/Art-Net/sACN console, GPU-accelerated
real-time 3D visualizer, and video/projection mapping, in one Rust/Dioxus
application — desktop and web, from the same codebase.

**Status**: pre-alpha. Domain modelling and research phase; see `docs/`.

## Why

FastTrackStudio (the sibling repo) is a music-production and live-worship
platform that already owns the *musical* structure of a show: song sections,
tempo maps, chord detection, setlists, a guide/count-in engine. No lighting
console on the market has access to that. Ignition's long-term bet is cues
that write themselves from the song — see
[`docs/research/lighting-console-landscape.md`](docs/research/lighting-console-landscape.md)
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

Follows [architect](https://github.com/FastTrackStudios/architect)'s
monorepo shape: `features/` for capability slices, `crates/` for the
libraries that compose them, `apps/` for things you run.

```
features/                one capability per directory, backends beside the facade
  colour/                what a value is — presets, splits, emitter resolve
  rig/                   what it lands on — selections, groups, tricks, focus
  show/                  recipes, cues, tracking, the programmer, the show file
  effects/               the shipped effect library
  playback/              the priority stack and the desk macros
  dmx/                   ignition-dmx-proto · -sacn · -artnet · the facade
  daw/                   ignition-daw-proto · -reaper · -transport · the facade
crates/
  ignition-proto         the types every crate shares
  ignition-core          composes the five domain slices into one namespace
  ignition-profile       the frame profiler — a tracing Layer that splits a
                          frame into Blitz's half and Bevy's. Nothing to do
                          with a lighting "profile", which is a domain term
                          this crate name unfortunately collides with
  ignition-viz           Bevy/wgpu visualizer, GDTF meshes, haze, gobos, canvases
  ignition-live-ui       Dioxus panes shared by the desk and the iPad Live page
  *-vendored             one-hunk forks of upstream crates, each with a NOTICE.md
apps/
  ignition-studio        the operator desk — multi-window Dioxus, viz embedded
  ignition-live-web      the Live panes, built for a browser/iPad
  ignition-engine        the headless engine binary
  ignition-mobile        the Ignition iPhone app
  ignition-web           the public site — landing page and guide (`just site`)
docs/guides/             the public guide — a wiki vault, compiled into the site
docs/spec/               normative `r[topic.id]` requirements, traced by tracey
docs/domain/             the domain model, grounded in a real venue
docs/research/           landscape studies — grandMA3, Resolume, OSS comparison
data/                    venues, profiles, songs, the GDTF library
tools/                   GDTF generation and the spec-coverage reporter
nix/                     flake-parts modules: toolchain, dx, dev + CI shells
```

The domain's layering is one-way and was derived, not decided —
`colour` and `rig` are leaves, `show` is the mutually-recursive core
that cannot be split further without redesigning what a cue means, and
`effects` then `playback` sit above it. `ignition-core` re-exports the
lot so an application sees one flat namespace.

Package names carry an `ignition-` prefix rather than architect's bare
`<feature>-<role>`: `daw`, `daw-proto`, `daw-audio-io`, `daw-control`,
`daw-module` and `daw-standalone` are already in this workspace's
dependency graph from the `FastTrackStudios/daw` repo, so the bare
namespace is taken and the tree is consistent about it.

Not yet built: `ignition-video` (media decode for mapped surfaces) and
the Graphics/Video studio modes — the only five requirements in
`docs/spec/` with no implementation. Lights first, deliberately
(`r[studio.modes.lights-first]`).

## Licence

GPL-3.0-or-later, matching FastTrackStudio. See `LICENSE`.

## Related

- [`FastTrackStudio`](https://github.com/FastTrackStudios/FastTrackStudio) — the
  audio/music product this integrates with.
- [`eos-toolkit`](https://github.com/Codys-Wright/eos-toolkit) — Cody's own
  ETC Eos OSC scripting toolkit; its `docs/` is the best single source of
  ground-truth Eos behaviour used in this project's research.
