# Lighting-console landscape — DMX architecture research

Research behind the live-DMX visualizer work (`crates/ignition-viz/src/
dmx.rs`, `channel_map.rs`, `live_renderer.rs`, `src/bin/live.rs`) —
2026-08-24. Companion docs: `docs/research/grandma3-recipes-and-phasers.md`,
`docs/research/projection-mapping-resolume.md`,
`docs/domain/dmx-channel-maps.md` (per-fixture channel map provenance).

## §9 — Phase 0 spike: headless renderer

Done — `crates/ignition-viz`'s `HeadlessRenderer`/`shot` binary (render a
venue to a PNG, no window, no display attached) was the first deliverable,
proving an agent (or CI) can see the visualizer without a display. See
`renderer.rs`.

## §7.1 — a real window

Was an open question (how a wgpu surface composes with Dioxus/Blitz, since
FastTrackStudio's own signal UI stack renders through
`nice-plug-dioxus`/Blitz rather than a raw window). Resolved for now by not
answering that question at all: `LiveRenderer`/`live.rs` opens a plain
`winit` window directly — a real interactive surface, independent of any
future Dioxus/Blitz embedding, which stays an open question for later if
Ignition ever wants its live view inside a larger desktop app shell instead
of standalone.

## What QLC+ actually does

**Fixture definitions (`.qxf`, XML):** each fixture is a manufacturer/model
XML file with a `<Channel>` list, where each channel declares its **type**
(Intensity, Colour, Pan, Tilt, Gobo, Shutter, Speed, ...) and a set of
**Capabilities** — byte-value ranges mapped to behaviours (e.g. byte 0-9 =
"shutter closed", 10-130 = "strobe slow→fast"). `<Physical>` carries
PanMax/TiltMax in degrees, bulb wattage, lens data, and a bounding-box size.
`<Mode>` blocks map a fixture's different DMX personalities (8ch/11ch/16ch
mode) to channel offsets. QLC+ ships thousands of community-contributed
`.qxf` files; Ignition already vendors QLC+'s generic per-category OBJ
meshes (`assets/qlc-meshes/`), so reusing the .qxf *channel model* (not
necessarily the XML file format) is a natural fit.

**Live DMX → 3D view:** QLC+'s I/O system is plugin-based — Art-Net,
sACN/E1.31, MIDI, USB DMX (Enttec/FTDI) all implement a common input/output
interface and write into per-universe DMX buffers. The Simple Desk and
Virtual Console read/write those buffers directly. QLC+'s 3D/fixture view
reads the same buffer: for each patched fixture, it resolves current
channel bytes through the fixture's Capabilities to get intensity/color/
pan/tilt/gobo, then poses the mesh accordingly, redrawing continuously —
exactly the shape `dmx.rs` + `channel_map.rs` + `scene.rs`'s live fixture
loop now implement.

## GDTF + MVR — the more vendor-neutral target

GDTF (DIN SPEC 15800) is the vendor-neutral successor concept to `.qxf`,
built by Vectorworks/MA Lighting/Robe, and is what grandMA3, Vectorworks,
Capture, and WYSIWYG all speak. A `.gdtf` file is a zip containing a
`description.xml` plus real assets (3D models as glTF/COLLADA, PNG gobo
wheels) — unlike `.qxf`, GDTF ships *actual geometry*, not a generic
placeholder mesh. Its `<DMXChannel>`/`<LogicalChannel>`/`<ChannelFunction>`
model is the same idea as `.qxf`'s Channel/Capability, but with an explicit
**Geometry** tree (yoke → head → beam-source as separate transformable
nodes) — the piece `fixture_profile.rs`'s single-mesh-plus-anchor model
doesn't have yet (see the note on `add_typed_fixture`'s `live_rot`
parameter: the whole fixture body rotates around the anchor point today,
since there's no yoke/head split — a documented approximation, not a bug).

MVR (DIN SPEC 15801) is the companion "scene" format: a zip of GDTF
references plus a scene XML describing *instances* (which GDTF fixture, at
what position/rotation, on what DMX address). This is functionally what
`fixtures.json`/`room.json`/`props.json` already do by hand.

**Practical read:** GDTF/MVR is the more future-proof target (real fixture
libraries at gdtf-share.com with actual manufacturer 3D models), but Rust
tooling is immature (`gdtf_parser` is pre-release, GDTF 1.0/1.1 only, spec
is now at 1.2). `.qxf` is simpler and Ignition already has a QLC+
mesh/licensing relationship. Neither is adopted yet — `ignition-proto`'s
`Attribute`/`ChannelMap` types are deliberately GDTF-*shaped* without
depending on either file format, so an importer for either can populate the
same internal schema later.

## ASLS Studio

Vue 3 + Three.js r170 (WebGL2) + Vite, packaged via Electron. Fixtures come
from a built-in XML fixture library (`xml2js` — same Channel/Capability
idea as `.qxf`). Critically: **the visualizer is in-process, not
networked** — it reads the app's own internal DMX state directly each
frame; Art-Net/sACN are output-only (to real hardware), not how its own 3D
view gets driven. Their docs candidly flag real limitations: gobos,
multi-color wheel slots, multiple bulbs per fixture, and "blading" are
explicitly *not* emulated — only position, color, intensity, pan/tilt,
zoom, strobe. Useful calibration: even a serious, actively-developed
open-source competitor stops short of full beam/gobo fidelity, which is
why Ignition's own beam/gobo work is phased as "later," not part of the
first DMX-input slice.

## Rust crates evaluated

| Crate | Protocol | Notes |
|---|---|---|
| `artnet_protocol` 0.4.4 | Art-Net v4 | Encode/decode only, no I/O loop — used in `dmx.rs`'s `spawn_artnet_listener` over a plain `UdpSocket` |
| `sacn` 0.11.1 | sACN / E1.31-2018 | Full streaming spec incl. universe discovery/sync; used in `dmx.rs`'s `spawn_sacn_listener` via `SacnReceiver` |
| `rust_dmx` | Raw USB DMX | Only relevant if Ignition ever needs to *output* to real hardware, not evaluated further |
| `gdtf` / `gdtf_parser` | GDTF XML | Both exist, `gdtf_parser` explicitly pre-release/unstable — not adopted yet |

No mature MVR-specific Rust crate exists; there's a C++ reference
(`libMVRgdtf`) but nothing turnkey in Rust.

## Architecture implemented (2026-08-24)

- **Data model split**: `fixtures.json` stays the fixed *mount* pose
  (unchanged). `ignition-proto`'s `DmxAddress`/`ChannelMap`/`Attribute`
  types are the live patch layer; `channel_map.rs` hand-authors one
  `ChannelMap` per manufacturer/model (see `docs/domain/
  dmx-channel-maps.md` for provenance/confidence per fixture).
- **DMX input**: `dmx.rs`'s `DmxUniverses` (an `Arc<RwLock<HashMap<u16,
  [u8;512]>>>`) fed by two always-listening OS threads, one per protocol —
  sACN via the `sacn` crate's multicast receiver, Art-Net via a raw
  `UdpSocket` on :6454 decoding `ArtCommand::Output`. Both protocols
  supported from day one since, per this research, adding the second is
  cheap once the buffer abstraction exists.
- **Resolution**: `DmxUniverses::resolve()` reads a fixture's patched
  bytes through its `ChannelMap` into `ResolvedAttributes` (dimmer 0-1,
  pan/tilt in degrees, RGB 0-1) — visualizer code downstream never touches
  a raw DMX byte.
- **Composition**: `scene.rs`'s fixture loop composes live pan/tilt as an
  *additional* rotation on top of the fixed mount rotation
  (`mount_rot * pan_quat * tilt_quat`) and live colour in place of the
  static `FixtureKind::color()` — only for fixtures with both a
  `dmx_address` (from the venue data) and a known `ChannelMap`; anything
  else renders exactly as it did before this work.
- **Live render mode**: `live_renderer.rs`/`src/bin/live.rs` — a real
  `winit` window, continuously redrawn (`RedrawRequested` → rebuild scene →
  render → `request_redraw` again), reading `DmxUniverses` fresh every
  frame. `shot.rs`/`HeadlessRenderer` is untouched and stays the headless
  regression-screenshot path — `build_scene`'s new `dmx: Option<&
  DmxUniverses>` parameter is `None` there.
- **Verified end-to-end**: a real sACN packet sent over the network (via
  `SacnSource`, unicast to bypass this dev sandbox's multicast routing)
  was received by `spawn_sacn_listener`, resolved through chan 50's
  channel map, and rendered — the fixture visibly turned red in the output
  PNG in response to a live network packet, not a static value.

## Slice 2 — visual realism (2026-08-24, same day as Slice 1)

Live-mode-only, exactly like Slice 1's philosophy: a separate shader
(`live_shader.wgsl`) and a second pipeline in `live_renderer.rs`, so
`renderer.rs`'s headless `shot` path — and every regression screenshot
this whole modelling session has been run against — stays byte-identical
(verified: same MD5 before/after this slice).

- **Real point lights**: `mesh.rs`'s `PointLight` (position + dimmer-
  scaled colour) is a new field on `MeshBuilder`; `fixture_profile.rs`'s
  `add_typed_fixture` takes an optional `LiveEmission` and, when present
  and the resolved dimmer is above a small noise floor, calls
  `mesh.add_light()` at the fixture's already-computed anchor point (the
  same point the Bottom/Top anchor maths already resolves — no separate
  "lens position" concept needed). `live_shader.wgsl`'s `fs_main` loops
  over a `var<storage, read> point_lights: array<PointLight>` (bind group
  1) with a cheap inverse-square-ish falloff, so a lit fixture actually
  illuminates the wall/floor around it, not just its own mesh.
- **Beam cones**: a second, separately-blended geometry list
  (`glow_vertices`/`glow_indices`) built by `add_glow_cone` (shares its
  geometry code with the existing `add_cone` via a new `build_cone` free
  function) — a cone from the fixture's anchor point along its local -Z
  (this project's aim convention), radius derived from the real fixture's
  `beam_angle_deg` (now parsed from `fixtures.json`, previously ignored)
  when known. Drawn in a second render pass with additive blending
  (`fs_glow`, pure emissive, no lighting) and depth-tested-but-not-
  depth-written against the opaque pass, so a beam doesn't glow through a
  wall but overlapping beams blend into each other instead of z-fighting.
- **Hazers get a white beam**: a fixture whose channel map has no colour
  channel at all (the Hurricane Haze's plain Dimmer map) still emits, as
  white — "no beam" would read as broken, and a real hazer genuinely does
  haze up whatever colour light is already hitting it.
- **Verified**: WGSL passes wgpu's runtime shader validation (no errors on
  module creation); `live` ran clean for repeated multi-second sessions
  with real sACN packets actively driving a moving head's pan/tilt/dimmer
  and a par's colour — both the point-light and glow-cone code paths
  exercised every frame, no panics, no wgpu validation errors. Could not
  get a pixel-level screenshot of the actual window in this dev sandbox
  (no working Wayland screen-capture protocol, ImageMagick's X11 `import`
  path blocked by policy) — resolved below (`--snapshot`).

## Slice 5 — headless live snapshots (2026-08-24, same day)

Follow-up to Slice 2's "couldn't screenshot it" gap: `live_headless_renderer.rs`
(`LiveHeadlessRenderer`) is `renderer.rs`'s `HeadlessRenderer` pattern
(render-to-texture, no window/surface, readback to PNG) rebuilt against
`live_shader.wgsl` instead — same point-light + additive-glow two-pass
pipeline as the real window (`live_renderer.rs`), just writing to an
offscreen texture instead of presenting to a surface. `src/bin/live.rs`'s
new `--snapshot <path>` flag uses it: start the DMX listeners, wait
`--warm-up-ms` (default 300) for anything already broadcasting to arrive,
build one frame of the scene, render, write the PNG, exit — no window, no
display, no `winit` event loop at all in this path.

This is what actually let the live-DMX and beam/light work get visually
verified: sent real sACN packets (pan/tilt/dimmer on two movers, RGB on
two pars, across both universes 1 and 2) and captured
`live --snapshot` output showing real coloured light spill on the
walls/ceiling and beam-cone glow at each lit fixture — not claimed, seen.

**Bug this surfaced and fixed**: `ResolvedAttributes::default()` defaulted
`dimmer` to `1.0` ("full on"). Fine for a fixture actively being read (an
explicit `Dimmer` channel byte of 0 correctly resolves to 0.0), wrong for
a fixture with *no* live data at all — which is the overwhelming common
case for most of a rig at any moment, and also every fixture the instant
`live` starts before any packets arrive. The very first `--snapshot` (no
DMX sender running yet) showed 2 phantom lights: the two Rockville
Rockstrip 3ch pars, the one fixture type with no dedicated Dimmer channel
in its map. Fixed by defaulting to *off* and handling the "no dimmer
channel, colour bytes ARE the brightness" case explicitly and only when
the colour is actually non-zero — two new regression tests
(`no_live_data_never_reads_as_lit`,
`bare_rgb_par_with_no_dimmer_channel_is_governed_by_its_colour`) lock
both halves of this in. A real bug this workflow was specifically built
to catch, caught on its first real use.

## Slice 4 — GDTF channel-map import (2026-08-24, same day)

The research flagged `gdtf_parser` (the crate name originally found) as
pre-release and stuck on GDTF 1.0/1.1. Re-checked before starting this
slice: a *different*, better crate exists — [`gdtf`
0.3.0](https://github.com/cpdt/gdtf-rs) (MIT), which targets the current
GDTF 1.2 (DIN SPEC 15800:2022) and has a real, complete object model
(`Description` → `FixtureType` → `DmxMode` → `DmxChannel` →
`LogicalChannel` → `Attribute`). This de-risked the slice considerably —
worth re-checking crates.io before writing off a whole approach based on
one crate's maturity.

**What's built** (`gdtf_import.rs`): `import_channel_map(path, mode_name)`
opens a real `.gdtf` file, picks a `DmxMode`, and walks its `DmxChannel`
list into this project's own `ChannelMap`/`Attribute` types — the exact
same shape `channel_map.rs` hand-authors. A GDTF channel with no `Offset`
(a "virtual" channel — e.g. a dimmer implemented by an RGBW colour mix
rather than a real DMX byte) is correctly skipped rather than mapped to a
bogus offset. `map_attribute_name` translates GDTF's standard attribute
names (`"ColorAdd_R"`, `"Pan"`, ...) to `ignition-proto::Attribute`;
anything not yet modelled round-trips via `Attribute::Custom` instead of
silently disappearing.

**Verified against a real file, not synthetic test data**: vendored the
`gdtf` crate's own MIT-licensed test fixture
(`assets/gdtf-samples/Generic@RGBW8@test.gdtf`, `LICENSE-NOTICE.txt` has
the full notice) and asserted the *real* parsed output — footprint 4,
R/G/B/W at offsets 0/1/2/3, the virtual Dimmer channel correctly absent
(no offset in the source file). Both tests pass;
`shot`'s regression PNG stayed byte-identical (this slice touches nothing
in the render path, only adds a new, unused-by-default import function).

**Not wired up to Norco yet, deliberately**: `channel_map.rs`'s hand-
authored entries for Norco's 7 fixture types are still what `scene.rs`
actually uses — none of those manufacturers (Uking, Chauvet, Riukoe,
Betopper, Rockville) had a real `.gdtf` file available to import in this
sandbox (no path to gdtf-share.com from here). `import_channel_map` is
real, tested, working code ready to replace any `channel_map.rs` entry
the moment a real file for that fixture exists — see
`docs/domain/dmx-channel-maps.md` for which entries are still estimated.

**Not built**: the fixture's real 3D geometry (GDTF's Geometry tree —
yoke/head as separate nodes — and the glTF/3DS models each references).
This is the "biggest value, biggest lift" half of GDTF import — a glTF
mesh parser plus a yoke/head-aware render path replacing
`fixture_profile.rs`'s current single-mesh-plus-anchor model — and MVR
import (still no mature Rust crate; a hand-rolled zip+XML scene reader on
top of `gdtf_import.rs`'s parsing would be the path, not evaluated
further this slice). Both are real follow-on work, not started.

## Deferred

- **GDTF 3D geometry + MVR scene import** — see Slice 4 above for what's
  actually left (channel/attribute import is done; geometry/scene import
  is not).
- **Per-fixture confirmed channel maps** — every entry in `channel_map.rs`
  has a confirmed *footprint* (real DMX-address spacing from the live
  patch) but an *estimated* per-channel function order. See
  `docs/domain/dmx-channel-maps.md`.
- **Gobo/colour-wheel rendering** — `Attribute::GoboWheel`/`ColorWheel`
  resolve in `dmx.rs` but nothing downstream reads them yet (no gobo
  texture, no wheel-slot colour swap). Both QLC+'s and ASLS Studio's own
  visualizers treat this as later/optional work too.
- **Haze/fog volumetrics** — the beam cones are solid additive geometry,
  not a scattering/haze simulation. Matches where ASLS Studio's own docs
  say their fog support stops (a toggle, no real volumetric technique).

## Slice 6 — ambient/haze settings + de-duplicating the two live renderers (2026-08-24, same day)

Two things prompted by the operator, back to back:

**Ambient/haze controls.** The same idea every viz app in this research
exposes (QLC+, grandMA3's 3D view, WYSIWYG) — a `Settings` uniform
(`ambient: f32, haze: f32`) in `live_shader.wgsl`, `--ambient`/`--haze`
flags on `bin/live.rs`. First implementation only zeroed the flat
"ambient floor" term and left the directional key light's diffuse+
specular contribution untouched — at `ambient=0` the room still rendered
almost fully lit, because that key light (standing in for a generic house
light) was never gated by the ambient setting, only added on top of it.
Fixed: `ambient` now scales the *entire* non-fixture lighting contribution
(key light diffuse and its specular highlight both), so at the default
`ambient=0` a surface with no fixture light on it renders genuinely black
— live point lights (`dmx.rs`-resolved, added after) are the only thing
that lights anything, matching a real dark venue. `haze` scales the beam-
cone pass's brightness (`fs_glow`) — 0 = beams invisible (no haze to
scatter them), the default 1.6 makes them read as solid visible shafts.
Verified visually via `--snapshot`: before the fix, floor/walls stayed
bright at `ambient=0`; after, only the pillars/walls near actual lit
fixtures show anything, everything else fades to black/haze exactly like
a real unlit rig.

**De-duplicating `LiveRenderer`/`LiveHeadlessRenderer`.** Flagged directly
by the operator ("did we just have to wire it in like 5 places? it should
all be the same code path") — fair: adding the settings uniform meant
touching the shader, pipeline layout, bind group, and buffer-upload code
in *two* nearly-identical ~450-line files, because the windowed and
headless renderers each had their own full copy of the shader-compile +
pipeline + two-pass-draw setup. Refactored into `live_pipeline.rs`
(`LivePipeline`): owns the shader, both render pipelines, both bind group
layouts, and `render_frame()` (the full two-pass opaque+glow draw) — the
one thing that's identical either way. `live_renderer.rs` and
`live_headless_renderer.rs` are now thin wrappers (~110 and ~140 lines)
holding only what's genuinely different: acquiring a device/queue
(windowed needs a surface-compatible adapter; headless doesn't) and the
final target (present a surface vs. read back an offscreen texture to a
PNG). A new setting is now one change, not two. `shot`'s regression PNG
stayed byte-identical through this refactor (MD5-verified, as with every
prior slice) — `renderer.rs`/`HeadlessRenderer` were never part of this
duplication and weren't touched.
