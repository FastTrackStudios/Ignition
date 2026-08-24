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

## Repo layout (proposed — see `docs/domain/`)

```
crates/
  ignition-proto     wire contract — architect-style RPC service traits
  ignition-core       no_std+alloc: attribute model, patch resolve, cue/
                       tracking engine, phaser maths, merge stack — no I/O
  ignition-io          sACN / Art-Net / USB widget adapters (native only)
  ignition-fixtures    GDTF / MVR / OFL / QLC .qxf importers → one model
  ignition-viz         wgpu 3D visualizer + projection-mapping render path
  ignition-video       media decode/playback for mapped surfaces
  ignition-ui          Dioxus panels: patch, programmer, cue list, timeline,
                       mapping editor
apps/
  ignition-engine      the headless engine binary (console + I/O + viz host)
docs/
  domain/              domain model (ASLS-style, grounded in a real venue)
  research/            landscape studies — OSS/industry comparison, grandMA3
                       recipes/phasers, Resolume mapping, venue reference
data/
  venues/norco/        real fixture patch + room geometry, extracted from an
                       actual Eos show file, used as the first visualizer
                       test case (see docs/domain/norco-venue-reference.md)
```

## Licence

GPL-3.0-or-later, matching FastTrackStudio. See `LICENSE`.

## Related

- [`FastTrackStudio`](https://github.com/FastTrackStudios/FastTrackStudio) — the
  audio/music product this integrates with.
- [`eos-toolkit`](https://github.com/Codys-Wright/eos-toolkit) — Cody's own
  ETC Eos OSC scripting toolkit; its `docs/` is the best single source of
  ground-truth Eos behaviour used in this project's research.
