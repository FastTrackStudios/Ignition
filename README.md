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

```
crates/
  ignition-proto       wire contract — the types every other crate shares
  ignition-core        the lighting domain, no I/O: attribute model, patch
                        resolve, selections and tricks, recipes/effects,
                        the cue + tracking engine, the priority stack
  ignition-profile     the frame profiler — a tracing Layer that splits a
                        frame into Blitz's half and Bevy's. Nothing to do
                        with a lighting "profile", which is a domain term
                        this crate name unfortunately collides with
  ignition-io          sACN / Art-Net transmit, rate + keep-alive, and the
                        loopback that feeds the visualizer the same bytes
  ignition-song        the song level: DAW song map, hit chart, transport,
                        timecode, cue generation and the show lint
  ignition-viz         Bevy/wgpu 3D visualizer, GDTF meshes, haze, gobos,
                        canvases and the projection-mapping render path
  ignition-live-ui     Dioxus panes shared by the desk and the iPad Live
                        page — cue list, faders, library, cameras
  *-vendored           one-hunk forks of upstream crates, each with a
                        NOTICE.md saying exactly what changed and why
apps/
  ignition-studio      the operator desk: multi-window Dioxus shell with
                        the visualizer embedded on Blitz's own wgpu device
  ignition-live-web    the same Live panes built for a browser/iPad
  ignition-engine      the headless engine binary
  mobile               the Ignition iPhone app (package `ignition-mobile`)
docs/
  spec/                normative requirements, `r[topic.id]` per paragraph,
                        traced to code by tracey (`just`-free:
                        `python3 tools/spec_coverage.py`)
  domain/              domain model (ASLS-style, grounded in a real venue)
  research/            landscape studies — OSS/industry comparison, grandMA3
                        recipes/phasers, Resolume mapping, venue reference
  ops/                 profiling and the iPad Live runbook
data/
  venues/norco/        real fixture patch + room geometry, extracted from an
                        actual Eos show file, used as the first visualizer
                        test case (see docs/domain/norco-venue-reference.md)
  profiles/            the shipped profile, baked from `ignition-core`
  songs/               song maps, hit charts and per-song cue lists
  gdtf/                the fixture library, real and generated
tools/                 GDTF generation and the spec-coverage reporter
nix/                   flake-parts modules: toolchain, dx, dev + CI shells
```

Not yet built: `ignition-video` (media decode for mapped surfaces) and the
Graphics/Video studio modes — the only five requirements in `docs/spec/`
with no implementation. Lights first, deliberately (`r[studio.modes.lights-first]`).

## Licence

GPL-3.0-or-later, matching FastTrackStudio. See `LICENSE`.

## Related

- [`FastTrackStudio`](https://github.com/FastTrackStudios/FastTrackStudio) — the
  audio/music product this integrates with.
- [`eos-toolkit`](https://github.com/Codys-Wright/eos-toolkit) — Cody's own
  ETC Eos OSC scripting toolkit; its `docs/` is the best single source of
  ground-truth Eos behaviour used in this project's research.
