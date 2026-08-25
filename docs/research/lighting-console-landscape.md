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

## Slice 7 — cloned ASLS Studio's actual visualizer source (2026-08-24, same day)

Prior research on ASLS Studio (top of this doc) was web-search-only —
"in-process DMX state, Vue3+Three.js, gobos/multi-bulb not emulated." The
operator asked to actually clone it and look. Cloned
`github.com/ASLS-org/studio` (GPL-3.0, fully compatible) to a scratch
directory (not vendored into this repo — only two techniques were ported,
both individually attributed below, not the codebase itself). Concretely,
their beam rendering (`src/plugins/visualizer/shaders/beam.{vertex,fragment}.glsl`,
`moving_head.js`) does three things Ignition's beam cones didn't:

1. **Real cone-angled spotlights, not omnidirectional point lights** —
   `moving_head.js` attaches an actual `THREE.SpotLight` (angle = the
   fixture's real beam angle) to a yoke→head→beam `Object3D` chain. Also
   reconfirms the Geometry-tree gap already flagged as deferred: they
   tilt only the head node, this project still rotates the whole fixture
   body around its anchor point.
2. **Layered 3D simplex noise for beam haze density** (`fogging()` in
   `beam.fragment.glsl`) — real spatially-varying turbulence, not a flat
   brightness multiplier.
3. **View-alignment brightening** — their `alignmentFactor` term makes a
   beam read brighter viewed across its length (grazing angle) than
   straight down the barrel, the standard cheap trick for making
   additively-blended cone geometry read as a volumetric shaft.

**Ported into `live_shader.wgsl`** (mechanical GLSL→WGSL translation,
same MIT terms as the source — Ashima Arts/`webgl-noise`, see the
translated functions' own header comment):

- `mesh::PointLight` gained `direction`/`cone_half_angle_deg`;
  `fixture_profile.rs::emit_light_and_beam` now passes the fixture's real
  aim direction and beam angle through to it. The point-light loop in
  `fs_main` gates contribution by a soft-edged `smoothstep` cone check
  (`direction_cos_angle` in the uploaded `GpuPointLight`/WGSL
  `PointLight`) — a wall a fixture isn't aimed at no longer gets lit.
- `fs_glow` gained a WGSL port of `snoise`/`fogging` (4-octave turbulence,
  same weights as the original) and a view-alignment term computed from
  the beam cone's own surface normal vs. camera direction (their
  per-instance-attribute approach doesn't map directly onto this
  project's baked-geometry beams, so this uses what's already
  available — the vertex normal — instead of adding new vertex
  attributes for it).

Not ported (deliberately, scope cuts for this pass): GPU instancing for
beam geometry (ASLS uses `THREE.InstancedMesh`; Ignition bakes triangles
into one shared buffer — fine at this fixture count, would matter at much
larger rig sizes) and time-animated fog turbulence (needs a `time`
uniform threaded through, the current fog is spatially-varying but static
per frame).

Verified: WGSL passes wgpu's runtime shader validation; 8/8 tests still
pass; `shot`'s regression PNG stayed byte-identical (MD5). A `--snapshot`
close-up (`--view stage`) shows soft-edged, properly cone-shaped colour
spill on the ceiling instead of the old flat omnidirectional glow — a
real, visible quality jump, not just more shader code.

## Slice 8 — the "ASLS file format" question: it's just Open Fixture Library

The operator asked directly whether ASLS's own file format is worth
adopting. Answer, from reading their actual source (`show.model.js`,
`fixture.model.js`):

**Project files (`.asls`)**: a plain flat JSON — `{name, bpm, fixtures[],
universes[], groups[], visualizer{}, outputs{}}`, each fixture entry
`{id, model, manufacturer, name, universe, chStart, mode, position,
rotation}`. Nothing to adopt here — comparing it directly against
`fixtures.json`'s own schema, this project's venue data already
independently converged on essentially the same shape (manufacturer/
model/universe/address/position/orientation per fixture). Good sanity
check, not a design change.

**Fixture definitions**: this is the real finding. ASLS has *no* fixture
format of its own at all — `show.model.js`'s `prepareFixtures()` fetches
`fixtures/{manufacturer}/{model}.json` and stores it as `fixtureData.
OFLData`, and `public/fixtures/` in their repo is a literal, unmodified
mirror of the **Open Fixture Library** (OFL,
`github.com/OpenLightingProject/open-fixture-library`, MIT) — every file
has OFL's own `$schema` URL at the top. So "should Ignition use ASLS's
format" resolves to "should Ignition use OFL," which turned out to be a
clearly *better* answer than the GDTF path (Slice 4) for the specific gap
this project has (confirming channel-function-order, not importing 3D
geometry):

- **Plain JSON, not zip+XML** — `ofl_import.rs` is ~90 lines using
  `serde_json::Value` directly, no zip crate, no XML parser, versus
  GDTF's zip-of-XML-plus-assets pipeline.
- **Real coverage for this venue's actual budget/no-name fixtures** —
  checked directly against Norco's 7 fixture types: Uking (a `uking/`
  manufacturer directory exists, with the exact Par Light B262 already
  guessed at in `fixture_profile.rs`'s shape code) and Chauvet Hurricane
  Haze 1DX both have real, current OFL profiles. Riukoe and Betopper have
  **no manufacturer entry in OFL at all** (checked, not just unsearched)
  — expected for genuinely obscure brands, but also true of GDTF, so
  neither path helps those two.
- **Immediately paid off**: fetched both real files, and one revealed an
  actual bug — the Uking Par's estimated layout (in `channel_map.rs`,
  written before this slice) had guessed a White channel that doesn't
  exist on the real fixture, at the position Strobe actually occupies.
  Uking Par is 47 of Norco's 71 fixtures. Also corrected: Chauvet
  Hurricane Haze's real footprint is 1 channel (just Haze), not the
  estimated 2ch dimmer+fan.
- **`ofl_import.rs`** (mirrors `gdtf_import.rs`'s shape) now exists,
  tested against the two real files that produced those corrections
  (vendored MIT under `assets/ofl-samples/`, same `LICENSE-NOTICE.txt`
  pattern as `assets/qlc-meshes/` and `assets/gdtf-samples/`) — running
  it reproduces the by-hand fix exactly, so it's ready to check any
  future fixture the moment a real OFL profile exists for it, not a
  one-off manual correction.
- Also revisited the Rockville Rockstrip 252 7ch entry, which used the
  *same* "generic 7ch par" template the Uking Par entry did before being
  corrected — flagged as suspect in `channel_map.rs`/`docs/domain/
  dmx-channel-maps.md` rather than left looking equally confident, since
  OFL has no profile for this specific Rockville model to check it
  against (only a different one, "rockpar50").

Net effect on the GDTF-vs-OFL question from Slice 4's original research:
not either/or. GDTF is still the right (eventual) path to real 3D
fixture geometry (yoke/head Geometry tree, actual manufacturer models) —
OFL doesn't carry geometry at all. For the channel-map-only need this
project actually had unresolved, OFL was faster to reach for and covered
more of Norco's real, obscure fixtures than GDTF did.

## Slice 9 — animated beam haze (2026-08-24, same day)

Closed the one deferred item explicitly flagged in Slice 7: the noise-
based haze turbulence (`fs_glow`'s `fogging()`) sampled world position
only, so it was spatially varying but frozen — the same mottled pattern
every frame, no drift. `GpuSettings`/WGSL `Settings` gained a `time: f32`
field; `LivePipeline::render_frame` takes a `time_secs` parameter and
`fs_glow` samples a moving slice of the 3D noise field
(`world_pos.z * 1.5 + time * 0.15`, same "advance one axis of a 3D noise
field over time" trick as ASLS's own `fogTime`) instead of a fixed
coordinate — the haze drifts without the beam geometry itself moving.

`LiveRenderer` (the real window) tracks a real `std::time::Instant` from
construction and passes elapsed seconds each frame — genuinely animated
when actually watching it. `--snapshot` (a single static frame has no
"next frame" to animate through) gained a `--time <seconds>` flag to pick
which phase of the drift to render, defaulting to the real wall-clock
time of day so repeated snapshots aren't all identical; `Live
HeadlessRenderer::render_to_png` takes `time_secs` as a parameter rather
than hardcoding 0.0.

Verified: two `--snapshot`s of the same live-DMX scene 5 seconds apart
produce different PNGs (different MD5) — the haze pattern actually
changed, not just the fixture state. 13/13 tests still pass, `shot`'s
regression PNG byte-identical (MD5) — this only touches the live shader's
glow pass.

## Slice 10 — yoke/head geometry split for moving heads (2026-08-24, same day)

The most-repeated deferred item (flagged in Slices 1, 2, and 7): a live
tilt reading rotated the fixture's *whole mesh* around its mount anchor,
base bracket included, instead of leaving the base fixed and tilting only
the head — a real moving head's yoke stays bolted to its mount; only the
head assembly moves.

**The split point**: `assets/qlc-meshes/moving_head.obj` has no named
sub-parts to split by, so it's a geometric heuristic instead — histogrammed
the mesh's own vertex Z distribution and found a real "waist" (28 vertices,
far fewer than the ~150-230 in every neighbouring bucket) around z≈0,
separating a wider cluster from z=-0.49 to -0.1 (yoke arms + base) from
another from z=0.1 to 0.33 (head/lens housing). `MOVING_HEAD_SPLIT_Z =
-0.02` in `fixture_profile.rs`, a new `split_z: Option<f32>` field on
`Shape::Mesh` (`None` for pars/hazers — nothing to split, they don't tilt).

**The pivot bug caught before it shipped**: the first implementation
rotated head-side triangles around the mesh's own local origin (same as
the base), which — since the origin isn't where the head actually
attaches to the yoke — would have made the head swing through a wide arc
from a pivot point far below itself, a worse artifact than the whole-body
rotation it was meant to fix. Corrected: `mesh::add_mesh_asset_split`
pivots head vertices around `split_z` transformed into `pre_rotate`'s
frame (`pivot_pre = pre_rotate * (0,0,split_z) * scale`), not the mesh
origin — `p_final = pivot_pre + tilt * (p_pre - pivot_pre)` for head
vertices, matching how a real hinge works. Pan still applies to
everything uniformly (a real yoke rotates on its mount when panning, base
included) via `scene.rs` now passing pan and tilt to `fixture_profile.rs`
*separately* instead of pre-combined into one quaternion.

**A second real bug this surfaced**: the very first `--snapshot` with the
split active showed a moving head's vertex count changed *even with no
DMX source running at all* — tracing it back: `dmx.rs`'s pan/tilt
resolution ran the raw byte-0 "nothing received yet" state through the
same degrees-per-byte formula real data uses, landing on -270°/-135°
(nowhere near neutral) purely from never having heard from a console.
Same class of bug as Slice 5's dimmer default-to-off fix, one level up:
`DmxUniverses::resolve()` now checks whether a universe has *ever* been
received at all (not just whether this particular byte is zero) and
returns fully neutral defaults if not — a universe that's genuinely
receiving all-zero bytes from a real, connected console (a legitimate
DMX state) still resolves normally.

**Also fixed a real regression before it landed**: gating the split path
on `split_z` alone (a shape property) meant `shot`'s headless path — which
never has live data — started taking the vertex-duplicating split-draw
code path too, breaking the byte-identical regression PNG invariant this
whole slice sequence has maintained. Fixed by gating on `(split_z,
live_pan_tilt)` together — the split draw only runs when there's an
actual live tilt reading to apply; idle/no-data fixtures use the exact
same `add_mesh_asset` path as before this slice existed.

Verified: 14/14 tests pass (two new regressions: universe-never-received
neutrality, and the existing suite); `shot`'s regression PNG byte-identical
(MD5) both before and after the gating fix; a `--snapshot` with a real
sACN packet driving a large tilt swing shows the head visibly displaced
from its neighbours while staying attached at its mount point, not
detached or floating.

## Slice 11 — closing the "still looks worse than ASLS" gap: AA + tone mapping (2026-08-24, same day)

Operator, after seeing several rounds of `--snapshot` renders: the
visualizer still looked noticeably worse than ASLS's own. Re-checked
ASLS's actual renderer setup (`visualizer.js::prepareRenderer()`) rather
than guessing what "worse" meant — it does three things this project's
live pipeline didn't:

```js
this.renderer = new THREE.WebGLRenderer({ canvas: this.domElement, antialias: true });
this.renderer.physicallyCorrectLights = true;
this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
this.renderer.outputColorSpace = THREE.SRGBColorSpace;
```

No bloom pipeline in use despite a `finalComposer` field existing (dead
code, never wired up) — so the gap wasn't missing bloom, it was these two
much cheaper things:

- **No anti-aliasing at all.** `antialias: true` is a one-line default in
  `THREE.WebGLRenderer`; wgpu has no such default, so every straight edge
  (walls, ceiling grid, beam cone silhouettes) was fully aliased. Added
  `SAMPLE_COUNT = 4` (4x MSAA) to both `live_pipeline.rs` render
  pipelines. Both callers (`live_renderer.rs`'s window,
  `live_headless_renderer.rs`'s `--snapshot`) now render into a
  multisampled colour target (`LivePipeline::make_msaa_color_view`) and
  resolve to their real final target — resolving on whichever of the two
  passes runs last (the glow pass when there's glow geometry, the opaque
  pass otherwise), since resolving mid-sequence would discard the
  following pass's additive blending against the multisampled buffer.
  Depth attachments needed the same sample count (`make_depth_view`
  updated) — wgpu requires every attachment in a pass to agree.
- **Naive clamp instead of a filmic tone curve.** Added an ACES filmic
  tonemap (Narkowicz 2015 fit) to both `fs_main` and `fs_glow`'s final
  output — a bright point-light spill or beam glow (routinely >1.0 before
  this) no longer just clips flat-white, it rolls off with a proper
  shoulder. Applied per-pass rather than once on a combined HDR buffer
  (no separate post-process pass exists to do that yet) — a known
  approximation, not physically identical to ASLS's single-buffer
  tonemap, but a real improvement over the flat clamp it replaced.
  `physicallyCorrectLights`/proper inverse-square units wasn't chased
  further this slice — the point-light falloff formula already existing
  (Slice 2) is a cheap approximation, not physically calibrated, and
  redoing its units is a separate, smaller task from what actually
  explained the "looks worse" gap.

`shader.wgsl`/`renderer.rs` (the headless `shot` regression path)
untouched — this is entirely inside `live_shader.wgsl`/`live_pipeline.rs`.
Verified: 14/14 tests pass, `shot`'s PNG byte-identical (MD5); a
`--snapshot` of the same lit scene as Slice 2's shows visibly smoother
ceiling-grid lines and cone silhouettes.

## Slice 12 — real GDTF 3D geometry import, forking `gdtf` (2026-08-24, same day)

Operator: "let's go ahead and get the GDTF working even if we need to
fork and expand the existing crates." Slice 4 only imported GDTF's
*channel map* (which DMX byte drives Pan/Tilt/Dimmer/Colour); this slice
imports the actual `<Geometries>` tree — the yoke/head/beam hierarchy and
each node's real position/rotation/dimensions, per DIN SPEC 15800.

**Blocker: `gdtf` 0.3.0's `Matrix` type has no public accessor at all.**
Every `<Geometry>` node's placement is a `Position="{...}"` attribute
parsed into `Matrix([[f64; 4]; 4])`, but the inner array is a private
field with zero getters — only `identity()` and (de)serialize impls.
There is no way to read a parsed geometry node's real transform through
the crate's public API. Confirmed by reading
`gdtf-0.3.0/src/description/values.rs` directly rather than guessing from
docs.rs (which doesn't show private fields either, but confirms the same
absence of accessor methods).

Per the operator's pre-authorization, vendored the crate rather than
working around it: `crates/gdtf-vendored/` (copied from
`~/.cargo/registry/src/.../gdtf-0.3.0/`, MIT-licensed, added as a
workspace member), redirected via root `Cargo.toml`'s
`[patch.crates-io] gdtf = { path = "crates/gdtf-vendored" }` — the same
mechanism this project's parent repo (FastTrackStudio) documents for
cross-repo co-development, here used for a single-crate API gap. Three
patches, all logged in `crates/gdtf-vendored/PATCH-NOTES.md`:

1. `impl Matrix` gained `pub const fn rows(&self) -> [[f64; 4]; 4]` — the
   actual fix, a one-method accessor.
2. Vendoring pulled the crate's own test suite into `cargo test
   --workspace` for the first time; its `geometry.rs` tests used
   `std::assert_matches!`, an unstable nightly-only macro that doesn't
   compile on stable — replaced 4 call sites with `assert!(matches!(...))`,
   the stable equivalent. Not a functional change, just made the vendored
   copy buildable on this project's toolchain.
3. The crate-doc example in `lib.rs` opens a fixture file
   (`Generic@RGBW8@test.gdtf`) that lived at the upstream repo's root and
   wasn't vendored (only `src/` was copied) — changed the fence from
   ```` ```rust ```` to ```` ```no_run ```` so the doctest still compiles
   without needing that file at runtime.

**The importer**: `crates/ignition-viz/src/gdtf_geometry.rs`,
`import_geometry(path, mode_name) -> GdtfFixture`. Walks the fixture
type's `<Geometries>` tree recursively (`build_node`), producing a
`GdtfNode` tree with real local position/rotation (decomposed from each
node's `Matrix` via the new `rows()` accessor — translation is column 3
of rows 0–2, rotation via `glam::Mat3::from_cols` → `Quat::from_mat3`)
and a `GdtfShape` (`Box`/`Cylinder`/`None`, from the node's referenced
`<Model>` primitive dimensions, Width→X/Length→Y/Height→Z to match this
project's Z-up convention; `Beam` nodes always resolve to `None` — a
light-exit marker, not a drawn mesh). Separately scans the DMX mode's
channels for which geometry names Pan/Tilt actually target
(`is_pan`/`is_tilt` flags on the matching node), confirming the crate's
own `AnyGeometry` trait doesn't expose `.position()` — needed an
exhaustive match over all 18 `Geometry` enum variants
(`geometry_position()`) to get at it uniformly.

**Real-mesh import (parsing `ResourceMap::read_model_mesh`'s raw
GLB/3DS bytes into actual triangle data) is explicitly out of scope** —
deferred as a later slice; this one only gets primitive-shape fallbacks
(Box/Cylinder), which is what the overwhelming majority of
community-contributed GDTF files actually ship (most don't reference a
real 3D model file at all).

**No real manufacturer `.gdtf` file was available to test against** —
GDTF Share (gdtf-share.com), the real fixture database, requires a
registered account this session doesn't have. Instead, hand-authored
`crates/ignition-viz/assets/gdtf-samples/basic-moving-head.gdtf`, whose
`<Geometries>`/`<DMXChannels>` structure (Base → Yoke(Axis) → Head(Axis)
→ Beam, with the exact Position matrices) is copied verbatim from the
GDTF spec's own official reference example
(`mvrdevelopment/spec/examples/geometry.md`, MIT-licensed spec repo) —
schema-accurate real GDTF structure, just not manufacturer-sourced.
Provenance logged in `gdtf-samples/LICENSE-NOTICE.txt`. Two new tests in
`gdtf_geometry.rs` verify against it: the full Base/Yoke/Head/Beam
hierarchy with `is_pan`/`is_tilt` on the right nodes and the exact
Z-translations from the spec example, and that primitive-shape fallback
dimensions resolve to positive values.

Verified: `cargo test --workspace` — 27 tests total across all crates
(16 in `ignition-viz` incl. the 2 new ones, 6 in the vendored `gdtf`
crate itself, 1 doctest, rest 0/trivial) — all pass. `shot`'s regression
PNG stayed byte-identical (MD5 `d4c0f1b2...`, matching Slice 10/11's
baseline) — expected, since this slice only adds an importer module with
no caller yet; nothing in the render path changed. **Not yet wired to
rendering** — `GdtfNode` trees aren't drawn anywhere (`fixture_profile.rs`/
`scene.rs`/`mesh.rs` untouched this slice); recursively drawing the tree
with pan/tilt rotation applied at the flagged nodes is the natural next
slice.

## Slice 13 — drawing the real Geometry tree (2026-08-24, same day)

Operator: "keep going so that we get the actual fixtures." Slice 12
built the importer but drew nothing — this slice adds the drawing side:
`gdtf_geometry.rs::draw_gdtf_fixture` walks a `GdtfFixture`'s tree and
bakes real triangles via `mesh.rs`'s existing `add_box`/`add_cylinder`
primitives (already exactly what `GdtfShape::Box`/`Cylinder` need — no
new mesh-builder primitives required), following the standard
kinematic-chain convention: each node's world position offsets from its
*parent's* orientation (a joint's own live rotation moves what's
downstream of it, not its own mount point), and a node's live pan/tilt
(applied only when the file's own `is_pan`/`is_tilt` flag says so — the
real manufacturer-declared joint, not a guessed split point) becomes the
rotation basis fed to its children, so the whole sub-tree under a
rotating joint carries with it, matching a real fixture. A `Beam` node
(always `GdtfShape::None` — nothing to draw) registers a light + glow
cone there instead, when a `LiveEmission` is passed, reusing
`mesh.rs`'s existing `add_light`/`add_glow_cone` rather than adding a
GDTF-specific light path — a fixture can have more than one beam exit
(a multi-lens unit), so this lights every `Beam` node found, not one
fixed anchor the way `fixture_profile.rs::emit_light_and_beam` does for
the QLC+-mesh path.

New test `draws_real_geometry_and_a_tilted_beam_moves_only_the_head_chain`
confirms both directions of the kinematic-chain claim against the same
sample file as Slice 12: an idle (pan=tilt=identity) draw bakes real
triangles and registers no light with no `LiveEmission` passed; a 90°
tilt moves the Beam node's world position (it sits under Head, the real
tilt target per the file's `<DMXChannel>`) while leaving Base's own
baked vertices byte-identical — a live tilt must never move geometry
above the joint it targets.

Wired into `shot` as a new standalone mode (additive, no existing flag
touched): `--gdtf <path> [--gdtf-mode <name>] [--pan <deg>] [--tilt
<deg>]` skips venue loading entirely and renders one fixture's real
Geometry tree alone, camera auto-framed to its own baked geometry
(`Camera::frame_points`) — so the imported shape can actually be looked
at, not just asserted on. Rendered
`assets/gdtf-samples/basic-moving-head.gdtf` idle and at `--tilt 45`:
both show real Base/Yoke/Head box primitives at their real relative
sizes and positions (the spec example's own dimensions), the head
box visibly rotated in the tilted render. `--venue`'s existing path is
untouched — `main()` branches to the new `render_gdtf_fixture` before
`Venue::load` only when `--gdtf` is passed.

Still not wired into `scene.rs`'s real venue rendering — no Norco
fixture has a real (or even schema-sample) GDTF file behind it in
`fixture_profile.rs::shape_for`, so nothing changed there this slice;
adding a `Shape::Gdtf` variant that plugs a `GdtfFixture` into
`add_typed_fixture` alongside `Shape::Mesh`/`Shape::Bar` is the
mechanical next step once a real manufacturer file exists to hang it
on (GDTF Share access is still the blocker — see Slice 12).

Verified: `cargo test --workspace` — 27 tests, all green (17 in
`ignition-viz`, one more than Slice 12's 16 — the new draw test).
`shot`'s regression PNG re-confirmed byte-identical (MD5 `d4c0f1b2...`,
same hash as Slice 10 through 12's baseline) after adding the `--gdtf`
branch to `main()` — the new code path never executes on the existing
`--venue` invocations `shot`'s other callers use.

## Slice 14 — a cue-list engine: programming actual light shows (2026-08-24, same day)

Operator: "let's get the shaders and visualizer and cue list going so we
can program light shows." Everything through Slice 13 could only
*receive* live DMX from an external console (`dmx.rs`'s sACN/Art-Net
listeners) — nothing in this project could originate a show of its own.
This slice adds that: a real cue-list engine, wired all the way through
to the live 3D view.

**Split across the domain boundary the project already has** (`proto` =
wire types, `core` = fixture-agnostic domain logic, `viz` = rendering +
I/O), matching `ignition-core`'s own stated aspiration
("`no_std`-compatible core... a placeholder crate boundary... rather
than being retrofitted"): this is the first real logic to land in that
boundary.

- **`ignition_core::cue`** (new): `CueValue { chan, attr, value }`, `Cue
  { name, fade_secs, values }`, `CueList { name, cues }`, and
  `CuePlayer` — a **tracking** cue-list playback state machine (Eos/
  grandMA's default cue-list behaviour: a cue only needs to list what
  *changes*; any `(chan, attr)` it doesn't mention holds wherever the
  previous cue left it, rather than snapping to a default — what makes
  programming a real multi-cue show practical). Deliberately has zero
  I/O, no DMX byte encoding, no fixture/channel-map knowledge, and no
  wall-clock access — `tick(dt_secs)` is handed elapsed time as data,
  the same convention `daw-audio-graph`-style processing crates in the
  sibling FastTrackStudio repo use for anything that must stay testable
  without a real clock. `go()` snapshots the *actual* current
  interpolated output as the next fade's start (not the previous cue's
  resting value) so re-firing GO mid-fade chains smoothly instead of
  jumping — confirmed by a dedicated test
  (`refiring_go_mid_fade_chains_from_the_actual_current_position`).
  `jump_to_end_of(index)` resolves every cue up to and including
  `index` instantly, for headless/automated testing without stepping
  through real elapsed time. 7 tests cover tracking, zero/non-zero
  fades, re-fire-mid-fade, end-of-list no-op, and the jump helper.
  Required adding `Hash` to `ignition_proto::Attribute`/`ColorChannel`
  (additive, nothing existing broke) so `(ChanId, Attribute)` pairs can
  key a `HashMap`.

- **`ignition_viz::show`** (new): the bridge — encodes a `CuePlayer`
  output frame into real DMX bytes and writes them into the same
  `DmxUniverses` shared state real sACN/Art-Net packets land in
  (`DmxUniverses::set_channel`, a new public one-byte setter next to
  the existing whole-universe `write_universe`), by resolving each
  `(chan, attr)` through the venue's own patch (`FixtureRecord::
  dmx_address()`) and `channel_map.rs`'s `ChannelMap::offset_of`. Byte
  encoding is the literal inverse of `dmx.rs::resolve()`'s
  byte-to-value formulas for `Pan`/`Tilt` (the only two with a
  non-linear/offset range); everything else is a plain linear 0-1
  fraction of the byte range. A cue targeting an unpatched channel or a
  fixture with no known `ChannelMap` is silently skipped, matching
  `scene.rs`'s existing tolerance for the same case. One test proves
  the actual round trip, not just internal self-consistency: a cue's
  Dimmer/Red values, after `apply_cue_output`, resolve back out through
  `dmx.rs::resolve()` — the exact path `scene.rs` reads — to
  approximately the original cue values.

- **Wired into `live`** (additive — no existing flag or behaviour
  touched): `--cuelist <path>` loads a JSON `CueList`. In the windowed
  mode, **Space = GO** (matching a real console's own convention),
  advancing to the next cue and fading into it over real elapsed time
  each redraw (`show::tick_and_apply`, called from `RedrawRequested`
  with `Instant`-measured `dt`). With `--snapshot`, `--cue N` jumps
  straight to the end of cue N's fade and captures that moment
  headlessly — proving a show without a window or a stopwatch.

Demo cue list at `data/shows/demo-wash.json` (3 cues: Blackout, Red
Wash, Blue Wash on Norco's 4 first-patched Uking pars, real
manufacturer/model/chan/universe/address straight from
`fixtures.json`) rendered via `--snapshot --cue 1` and `--cue 2`: both
show 4 real par fixtures lit and beaming through the actual DMX-encode
path, and Blue Wash — which never re-states Dimmer — correctly holds
full brightness while only the colour crosses over, proving tracking
semantics survive the full round trip through real bytes, not just the
in-memory engine.

**Not yet built**: no group/preset authoring convenience (cues target
raw `chan` numbers today, not the venue's own `groups.json`/
`group-names.txt`); no actual DMX *output* onto the network (this only
drives the in-process visualizer's shared state, not real sACN/Art-Net
transmission to a real console or rig — `docs/research/lighting-
console-landscape.md`'s own DMX-architecture research would guide that
if/when it's wanted); no chase/effect generators (a cue list is still
hand-authored JSON, one cue at a time).

Verified: `cargo test --workspace` — 35 tests, all green (7 new in
`ignition-core`, 3 new in `ignition-viz::show`, everything else
unchanged). `shot`'s regression PNG stayed byte-identical (MD5
`d4c0f1b2...`, unchanged) — this slice only adds new modules and an
additive `live` flag, nothing in the static venue-render path changed.

## Slice 15 — Groups, Colors, Focus Points, and Recipes (2026-08-24, same day)

Operator, invoking grandMA3's own model by name: "we need to support
Groups, Colors, Focus Points. Then we can start with Recipes... Recipes
is extremely powerful and flexible and should be the foundation of what
we do." Slice 14's cue engine could only target raw channel numbers with
raw attribute values — this slice adds the authoring layer real desks
build cues from, compiling down to Slice 14's already-tested flat `Cue`
format rather than changing it.

**Real data, not invented data**: Norco's live Eos patch exports its own
`groups.json` — 112 real named groups ("Pars", "OH Movers", "Chase
Quarters", ...) with Eos's own range-string channel shorthand. Wired
straight in rather than inventing a parallel group format:
`ignition_viz::venue::Venue` gained `group_records` (loaded from
`groups.json` if present — optional, so a venue extract without one
just has no named groups to target by name) and `groups()`, which
resolves them into `ignition_core::Group`'s plain `(name, chans)` shape.
Eos's export turned out to use *two* different shapes for `channels` in
the same file — most groups are range strings (`"1-48"`), but some
(Norco's "Pars Odd") are a plain JSON array of individual channel
numbers — caught by testing against the real file rather than an
assumption; `ChannelListEntry` is `#[serde(untagged)]` so either shape
in the same array parses.

- **`ignition_core::group`** (new): `Group { name, chans }` — the *who*.
- **`ignition_core::preset`** (new): `ColorPreset { name, red, green,
  blue }` and `FocusPointPreset { name, target: Vec3 }` — the *what*.
  Only these two preset types (the ones asked for first); more (Gobo,
  Beam, Shapers) are additive later, not a redesign.
- **`ignition_core::focus`** (new): the actual reason a Focus Point
  preset is more than "a stored value" here — real inverse pan/tilt
  math. Given a fixture's real hung `Placement` (position +
  orientation, already extracted from the live rig) and an arbitrary
  XYZ room location, `pan_tilt_deg_to_point` solves the Pan/Tilt pair
  that aims the fixture's beam there, targeting the same canonical
  `mount_rot * RotZ(pan) * RotX(tilt) * NEG_Z` convention `dmx.rs` and
  `gdtf_geometry.rs`'s kinematic drawing both already use (deliberately
  *not* `fixture_profile.rs`'s `moving_head_pre_rotate` — that is a
  QLC+-placeholder-mesh-authoring correction, not a physical/DMX-level
  fact). This is a real 3D-geometry-driven feature this project's own
  extracted venue data makes possible that a flat DMX-only cue engine
  couldn't do. Pure `ignition_proto::Vec3`/`Quat` (f64) math, no
  `glam` dependency added to `ignition-core`. Proven with a genuine
  round-trip test: pick arbitrary pan/tilt, compute where that beam
  physically points under a *non-identity* mount rotation (exercising
  the inverse-quaternion step, not just the identity case), place a
  target far out along that exact direction, solve back from the
  target, and confirm the original angles come back out.
- **`ignition_core::recipe`** (new): `Recipe { target: RecipeTarget,
  apply: RecipeApply }` (`RecipeTarget::Group(name)` or
  `::Chans([...])`; `RecipeApply::Dimmer`/`Color`/`FocusPoint`/`Raw`),
  `RecipeCue`/`RecipeCueList`, and `expand_cue`/`expand_cue_list` —
  compiles a list of recipes into the exact flat `Cue`/`CueValue`
  `CuePlayer` already plays back unchanged. An unknown group name
  resolves to zero fixtures rather than erroring (same tolerance as
  `channel_map_for`/`shape_for`'s unknown-fixture fallback); a
  `FocusPoint` recipe silently skips a channel with no known
  `Placement`. `expand_recipe` takes the venue's placement lookup as a
  caller-supplied closure rather than reading it directly — keeps
  `ignition-core` free of any venue-loading/I-O knowledge, the same
  "no I/O" rule `cue.rs` already states.
- **Wired into `live`**: new `--recipes <path>` flag (mutually exclusive
  with `--cuelist`), loads a `RecipeCueList` and compiles it against
  `venue.groups()` + `venue.placement_of` before handing the result to
  the same `CuePlayer` `--cuelist` uses — Space-to-GO and
  `--snapshot --cue N` both work identically regardless of which
  authoring format built the cues.

Demo at `data/shows/demo-recipes.json`, run against real Norco data
(`--recipes ... --cue 1 --snapshot ... --view stage`): a "Movers on
Drums, Pars Red" cue built from 5 recipes (Group "Pars" -> Dimmer +
Red, Group "OH Movers" -> Dimmer + Color + FocusPoint at the real drum
kit's position from `props.json`) compiled against the venue's 112 real
groups and rendered 51 real lights — the "Pars" group alone resolved to
its full 48 real channels, all lit red in the render. The OH Movers'
`Color` recipe correctly produced no visible RGB change — confirmed
against `channel_map.rs`: Riukoe/Betopper movers use a real physical
colour *wheel* (`Attribute::ColorWheel`), which this project doesn't
model as an RGB target, so a `ColorAdd`-based recipe has nothing to
attach to and is silently skipped, the same tolerance
`apply_cue_output` already documents for any unmatched attribute — not
a bug, a correct reflection of what that real fixture type can actually
do.

**Deliberately not built this slice**: Phaser Recipes (grandMA3's
effect-generator engine — a waveform driving an attribute across a
group with per-fixture phase offset, a continuous function of time
rather than a fixed target state) — the operator's own stated next
step after Recipes land, not part of "start with." Also not built:
Gobo/Beam/Shaper preset types, and no UI/console for authoring
recipes/cues beyond hand-written JSON.

Verified: `cargo test --workspace` — 47 tests, all green (16 in
`ignition-core`, up from 7; 24 in `ignition-viz`, up from 20 — new
tests in `venue.rs` for the real-groups-file parsing, including the
two-different-`channels`-shapes case). `shot`'s regression PNG stayed
byte-identical (MD5 `d4c0f1b2...`, unchanged).

## Slice 16 — Effect Recipes (grandMA3's "Phasers"), and a full-rig look (2026-08-24, same day)

Operator: "let's build that out and then I want to see a look that has
all the lights on." Mid-build, on naming: grandMA3 calls this engine
"Phasers," but that's grandMA3-specific jargon — asked which name to
use (`Effect`/`Chase`/`Wave`/keep `Phaser`), operator picked **Effect**,
the term Eos/QLC+/most other consoles actually use. Every type is named
accordingly (`EffectRecipe`, not `PhaserRecipe`) rather than shipping
the grandMA3 name and renaming later.

**`ignition_core::effect`** (new): the genuinely different kind of thing
Slice 15's static `Recipe`s aren't — a **continuous function of time**
rather than a fixed target state. `Waveform` (`Sine`/`Square`/`RampUp`/
`RampDown`/`Triangle`, all sampled in `[-1, 1]` over one cycle so a
`Dimmer` effect and a `Pan` effect share the same waveform code despite
totally different value ranges), `EffectRecipe { target, attr, waveform,
rate_hz, size, base, phase_spread_deg, phase_offset_deg, direction }`,
and `EffectPlayer` (no `go()`/stepping — every loaded effect evaluates
fresh every `tick()`, forever, unlike `CuePlayer`). Reuses
`recipe::RecipeTarget`/the same `resolve_target` a static `Recipe`
uses (exposed `pub(crate)`), so `Group`/`Chans` targeting stays
identical between the two authoring formats. `phase_spread_deg` spreads
one full cycle evenly across the target group's fixtures (360 = a
classic chase, each fixture visibly offset from its neighbour; 0 = every
fixture in lockstep, a pulse); `phase_offset_deg` is a *fixed* shift
applied equally to every fixture — the trick for building a circle/
figure-8 without a dedicated "Position effect" type: two `EffectRecipe`s,
one on `Pan` one on `Tilt`, same rate, 90 degrees of `phase_offset_deg`
apart, is a circle. Proven with a dedicated test doing exactly that
(`a_90_degree_offset_pair_traces_a_circle_on_two_attributes`) — at t=0
pan sits at its base while tilt sits at its own peak, a quarter-cycle
later they've swapped, exactly what two sine waves 90° apart should do.

**Bridged into `ignition_viz::show`**: `tick_and_apply_effects`, the
`EffectPlayer` counterpart to Slice 14's `tick_and_apply`. **Wired into
`live`**: `--effects <path>` loads an `EffectList` and runs it
continuously from the moment it's loaded (no GO — effects aren't
stepped); ticked/applied *after* the cue player each redraw, so a
running effect layers on top of whatever a cue set (last-write-wins
per byte, not true HTP blending — noted as a real limitation, not
silently glossed over). `--effect-time <secs>` (paired with
`--snapshot`, same idea as `--time` for haze phase) freezes an effect
at a specific elapsed-seconds moment to capture headlessly.

**Visually proven through the real render pipeline, not just unit
tests**: `data/shows/demo-effect-chase.json` (a `Sine` Dimmer chase,
360° spread, across the real "Pars" group) rendered at
`--effect-time 0` and `--effect-time 2` — the two renders show a
visibly different bright/dim pattern across the same 47 real pars,
confirming the waveform actually animates end-to-end through
`show.rs`'s byte encoding and `scene.rs`'s live rendering, not just
`EffectPlayer::output()` in isolation.

**"All lights on"**: `data/shows/all-lights-on.json`, a `RecipeCue`
built from Slice 15's Group/Color/FocusPoint recipes — `Group "All"`
(Norco's own real Eos group, 1-18+20-97) at full Dimmer + warm white,
the two hazer channels (100-101, outside "All"'s range) added
explicitly, and `FocusPoint` recipes aiming "Movers OH" at the real
drum kit position and "Movers Beam" further downstage — rendered
house and top-down views: 69 of Norco's 71 real patched fixtures lit
simultaneously, built entirely from real venue data (the real group,
the real drum-kit coordinates from `props.json`) through recipes, no
hand-listed channel numbers.

Verified: `cargo test --workspace` — 53 tests, all green (22 in
`ignition-core`, up from 16 — 6 new `effect` tests). `shot`'s
regression PNG stayed byte-identical (MD5 `d4c0f1b2...`, unchanged) —
purely additive, same as every slice since Slice 12.

## Slice 17 — real throw distance and beam-cone shape (2026-08-24, same day)

Operator, after seeing the all-lights-on renders: "the lighting columns
look really bad, like they are cones that just stop within a few feet
and stuff so our visualizer needs a lot of updating to have stuff like
ASLS to be more like that." Root cause, in `fixture_profile.rs`'s
`emit_light_and_beam`: `let length = 2.5f32;` — a flat constant
regardless of the room's real size, unchanged since it was first written
("there's no throw distance concept here yet," flagged in that comment
from day one). Norco's real truss height is ~3.4m; a 2.5m beam from a
downward-aimed ceiling fixture stops well short of the floor — exactly
"cones that stop within a few feet."

**Real throw distance**: new `BeamThrow` (`fixture_profile.rs`),
computed once per `build_scene` call from the venue's own real bounds
(`Venue::bounds()`), not per-fixture. `reach(origin, direction)`
intersects the beam's real aim direction against the floor plane
(`floor_z = venue.bounds().0.z`) for any beam with a real downward
component, so a beam now travels exactly as far as it would in reality
before hitting the floor — not a guess. Beams aimed level/upward (no
floor to hit within this simple model) fall back to a capped reach.
That cap needed real tuning: an uncapped room diagonal (Norco's is
~22m) produced beams so large on near-horizontal aims that their
additive glow blew the entire render out to solid white — capped at
`min(room_diagonal, 10m)`, a realistic stage-throw distance, applied to
both the floor-intersection and fallback cases (a near-horizontal
*downward* aim has the same runaway-division problem).

**The beam-cone shape itself was backwards.** `mesh::build_cone` (used
for both `add_cone`'s tiny opaque decorative fixture-body stub *and*,
until now, `add_glow_cone`'s actual light-beam glow) puts its wide
circular base at the fixture and tapers to a single point at the far
end — fine for a ~0.15m decorative stub, exactly backwards for a real
light beam, which should be narrow near the lens and flare *wider* as
it travels (and dim as it does, since intensity per unit area falls off
over the growing spread). This was always wrong but invisible while
beams were a fixed short 2.5m; once they got long enough to matter, it
became the dominant visual bug — long, uniformly-bright, wide-at-the-
top icicle shapes instead of soft light shafts.

New `mesh::build_glow_cone` (a real frustum, not shared with
`build_cone`'s decorative-stub use, which is left untouched): narrow
near ring at the fixture (~55% of the emission colour), flaring to the
beam's real spread at the far end (~10% of emission colour, dimming with
distance/throw — the physically correct direction). This alone mostly
fixed the "icicle" look, but surfaced a second, deeper bug.

**`fs_glow` tonemaps *before* additive blending** (a known approximation
flagged back in Slice 11: "an approximation, not physically identical to
[a] single-buffer tonemap"). While beams were short and rarely overlapped
on screen, several already-tonemapped-toward-1.0 fragments summing past
white in the frame buffer was a rare, minor artifact. Once beams got
long *and* correctly wide, a dense rig's beams overlap on far more
pixels, and summing many near-1.0 fragments reliably clips solid white —
reproduced directly with the "all lights on" look's tightly-clustered
OH movers viewed nearly straight up the beam column. Not fixed at the
root (a real fix needs an HDR intermediate target and one final tonemap
pass over the composited result, not per-fragment) — that is a real,
scoped follow-on, not attempted this pass. Mitigated by cutting the
glow pass's own headroom: `fs_glow`'s view-alignment term capped at
`mix(0.4, 1.0, edge)` (was `mix(0.55, 1.4, edge)` — the >1x boost on
top of an already-bright fragment was the most direct contributor), plus
the near/far colour cuts above.

Verified visually, not just by absence of a regression failure (`shot`'s
static path never touches this code — `dmx: None` means `emit` is
always `None`, `shot`'s MD5 stayed unchanged as expected): a moderate,
realistic cue (`demo-recipes.json`'s 4 movers + 47 pars) now shows real
flared, floor-reaching beams; the extreme `all-lights-on.json` look
(69 fixtures at once) still saturates in a top-down/house view aimed
squarely into the packed OH-mover cluster (the additive-blending root
cause above), but its **top-down view** — a much more representative
camera angle for evaluating a look, not staring straight up a beam
column — shows correctly flared, soft, haze-lit beams reaching the
floor with visible individual pools of light, a real, substantial step
toward ASLS's own look. `cargo test --workspace`: 53/53 (unchanged —
this slice is shader/geometry tuning, no new testable logic). `shot`'s
regression PNG byte-identical (MD5 `d4c0f1b2...`).

**Not done this pass, flagged honestly**: the HDR-intermediate +
single-final-tonemap fix for the additive-overlap saturation root
cause; a real occlusion raycast for throw distance (currently floor-
plane only — a beam aimed at a wall or riser still reaches the capped
fallback distance rather than stopping at that surface); ASLS's other
still-unported techniques (GPU instancing, animated-not-just-drifting
fog turbulence, from Slice 7's own "not ported" list).
