# Venue reference: Rockstars of Tomorrow, Norco CA

**Source**: `Popstars_Playground_VERIFIED_20260823.esf3d`, a real, verified Eos
show file for the venue, provided 2026-08-24. Extracted with the method
documented in [Cody's eos-toolkit](https://github.com/Codys-Wright/eos-toolkit)
(`docs/norco-location.md`, `docs/norco-rig-facts.md`) — this doc is the
Ignition-side companion, focused on what the extract gives the visualizer.

**Not committed to this repo**: the `.esf3d`/`.a3d` show files themselves
(binary, large, and the venue operator's live show data — see `.gitignore`).
**Committed**: `data/venues/norco/*.json`, a clean derivation containing only
positions, orientations, sizes and tags — no cue/patch-address/show data.

## What's in the extract

An `.esf3d` is a ZIP of `version.json` + `showlog.log` + `showdat.dat`
(proprietary binary, the actual show — patch addresses, cues, palettes; not
touched here) + `working.a3d`, itself a ZIP holding the Augment3d scene:
`Scene/Patch.json` (fixture placements), `Scene/Primitive.json` (room
geometry + set dressing), `Scene/Scene.json` (the object tree), a material
library, and `Resources/*.glb` (ETC's stock Augment3d meshes — not
redistributed here, see below).

```
data/venues/norco/
  fixtures.json   69 patched fixtures: chan, tags, position, eulers, quat, size
  room.json       20 architectural objects: walls, stage decks, floor, ceiling
  screens.json    5 TVs — the first projection-mapping test surfaces
  props.json      29 set-dressing objects (speakers, mics, drum kit, pillars…)
```
(42 "Chair L*"/42 "Chair R*" seating objects were dropped from the extract —
audience seating, not relevant to lighting/video mapping.)

## Fixtures (`fixtures.json`)

69 entries, matching the 71-channel count in eos-toolkit's rig docs minus the
two phantom/unpatched channels (19, 98 — see `norco-location.md`). Tag
breakdown:

| Tag | Count | Maps to |
|---|---|---|
| `Luminaire_LED_Wash` | 59 | the Uking pars + Chauvet SlimPARs + Rockstrip foot lights |
| `Movers All`, `Luminaire_LED_Yoke_Spot` | 8 | the Riukoe overhead + Betopper floor moving heads |
| `Other` | 2 | the two hazers (100, 101) |

Every entry carries the **hang**, not the aim: `position` is world-space
metres (Augment3d convention: +X stage left, +Y upstage, +Z up, origin at
stage centre/deck), `eulers`/`quat` is the *mounting* rotation (yoke angle for
movers, aim angle for fixed wash fixtures), `size` is the fixture's rendered
bounding box. This is exactly the placement model `ignition-proto::Placement`
captures — see `crates/ignition-proto/src/lib.rs`. Live pan/tilt at showtime
is separate and is not in this file (there is no showtime — this is a design-
time patch snapshot).

Two things worth propagating into fixture-import code, both hard-won in
eos-toolkit:

- **Movers have colour wheels, not RGB mixing** (`docs: norco-rig-facts.md`).
  An `Attribute::ColorAdd` model does not fit channels 80–88 — they need
  `Attribute::ColorWheel { slot }`. Tagging fixtures by GDTF-style category
  (`Luminaire_LED_Yoke_Spot` vs `Luminaire_LED_Wash`) at import time, not by
  channel range, is what makes that swap correct automatically.
- **Orientation is the hang, not the aim.** Computing an "aim vector" for a
  mover and writing it as the placement orientation is a bug that has already
  bitten this exact venue once (`norco-location.md` §"Movers: orientation is
  the hang, not the aim") — it draws the fixture bolted on crooked *and*
  offsets every live pan/tilt/focus-palette value read against it. Ignition's
  fixture model should make `Placement` (rig-time, static) and live attribute
  state (pan/tilt, showtime) two different things that cannot be confused —
  which they already are in `ignition-proto`.

## Room (`room.json`)

20 architectural primitives — walls, the upstage lift, the downstage deck, the
audience floor, the ceiling, the stage lip face — enough to reconstruct the
room `a3d_room.py` builds parametrically from two numbers (stage width/depth
in feet) per `norco-location.md`. Two things to carry forward if Ignition ever
authors rooms parametrically the way `a3d_room.py` does:

- **Walls are one-sided planes** whose visible face depends on which side
  people stand on — not a fixed rule, derived per wall from which side of the
  room it's on. A naive "double every wall" fix makes walls opaque from the
  wrong side too (it blocked the sound-booth camera at this venue).
- **Props store height as an offset above their deck**, not an absolute
  height — a snare drum's `z` is centimetres above the riser it stands on, not
  metres above the world origin. Moving a deck must re-seat everything on it.

## Screens (`screens.json`) — the projection-mapping seed

Five TVs, real positions, the first concrete target for
[`docs/research/projection-mapping-resolume.md`](../research/projection-mapping-resolume.md):

**Sizes updated 2026-08-24** to real 16:9 TV dimensions per the operator's
call ("middle TVs are 60, 65, 60; the two outer ones call them 65\""),
computed from the diagonal rather than kept as the raw extraction's
AABB-derived (and, for the rotated flare screens, AABB-distorted) values:

| Name | Position (x, y, z, metres) | Diagonal | Size (w×h, metres) |
|---|---|---|---|
| `TV` (centre) | (0, 2.46, 1.63) | 65" | 1.439 × 0.809 |
| `TV` (stage-left of centre) | (-3.5, 2.46, 1.63) | 60" | 1.328 × 0.747 |
| `TV` (stage-right of centre) | (3.5, 2.46, 1.63) | 60" | 1.328 × 0.747 |
| `TV - Flare SL` | (4.90, -2.06, 1.94) | 65" | 1.439 × 0.809 |
| `TV - Flare SR` | (-4.90, -2.06, 1.94) | 65" | 1.439 × 0.809 |

Also corrected: every screen's `eulers.x` had local +Z (the render-facing
normal) pointing straight up — the raw extraction's rotation didn't account
for `ignition-viz`'s "+Z is front" quad convention. Added 90° so they face
horizontally into the room (the existing yaw on the two flare screens was
left untouched). Rendered as flat black panels (a screen with no content
assigned is a TV that's off), not the placeholder blue tint from the first
pass — see `docs/research/projection-mapping-resolume.md` for the mapping
model this feeds into.

Three screens sit in a row on the upstage wall (the "back wall" the eos-
toolkit rig docs describe faders 40/41/47/48 as lighting); two more sit on the
stage-side flares, angled into the room. This is a real, mixed-geometry
mapping surface set — not a single flat plane — which makes it a good first
test for Ignition's mapping model (§ in the Resolume research doc): each
screen needs its own quad in the composition, positioned/rotated to match its
`eulers`, sized to its `size`.

## Using this for the visualizer spike

This is the seed data for Phase 0 in
[`docs/research/lighting-console-landscape.md`](../research/lighting-console-landscape.md)
§9: load `fixtures.json` + `room.json`, render the room as simple boxes/planes
(no need for the ETC stock GLBs — author flat-shaded placeholders, or generate
them, rather than depend on assets that aren't ours to redistribute), place a
raymarched beam cone at each fixture using its real hang, and drive it from a
fake DMX frame. `screens.json` becomes the first mapping-surface test once the
video layer exists.

`apps/ignition-engine` already reads `fixtures.json`/`screens.json` at
startup as a load-bearing smoke test — see `apps/ignition-engine/src/main.rs`.

## Licensing note on the source assets

The `.glb` meshes inside `working.a3d` (`TV.glb`, `Wall - *.glb`, `Speaker
1.glb`, etc.) are ETC's stock Augment3d library content, not this venue's
property or this project's to redistribute. They are intentionally **not**
copied into this repo. `props.json`/`room.json` keep only geometry-agnostic
transform data (position/rotation/size) so the *layout* is preserved and
reusable without carrying ETC's assets — Ignition will need its own primitive
meshes (or a generator) for walls/TVs/speakers/etc.
