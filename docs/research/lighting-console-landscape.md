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

## Deferred (not built in this slice)

- **Visual realism** (real per-fixture point lights, additive beam cones,
  haze) — the shader still only does the original flat/procedural
  texturing; dimmer/colour changes the fixture mesh's own material colour
  but there's no light-emission or beam-cone rendering yet. QLC+ and ASLS
  Studio both keep beam rendering cheap (no full volumetric raymarch) —
  the right target when this is picked up.
- **GDTF/MVR import** — biggest value, least mature Rust tooling; right
  thing to defer until the live-DMX core (this slice) is proven.
- **Per-fixture confirmed channel maps** — every entry in `channel_map.rs`
  has a confirmed *footprint* (real DMX-address spacing from the live
  patch) but an *estimated* per-channel function order. See
  `docs/domain/dmx-channel-maps.md`.
