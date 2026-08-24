# Norco field measurements — 2026-08-24

**Source**: dictated on-site measurements from the operator, 2026-08-24 — real
tape-measure/pace-count numbers, not derived from the Eos show file or the
magic-sheet import. Per this project's own house rule (borrowed from
eos-toolkit: "after every write, read the state back" / never trust an
unverified position), **these numbers supersede** the hand-derived estimates
in [`norco-venue-reference.md`](norco-venue-reference.md) and eos-toolkit's
own `norco-location.md`/`norco-rig-facts.md` wherever they conflict — this is
ground truth, those were inference.

**Unit resolved**: "tile" = **2 ft × 2 ft** floor tile, confirmed by the
operator (matching the 2 ft ceiling grid eos-toolkit already used as a
measuring reference in the same room).

**Coordinate frame** (unchanged from `norco-location.md`): metric, origin at
stage centre on the downstage-deck surface, **+X stage left, +Y upstage, +Z
up**.

This doc is a working reference for updating `data/venues/norco/*.json`, not
a rewrite of it yet — several groups below reference "the outer back movers,"
"the center 12 pars," etc. without a confirmed channel number, and I'm not
willing to guess-assign those without saying so. Sections marked **CONFIRMED
— ready to apply** have no ambiguity left and should go into the JSON;
sections marked **NEEDS CHANNEL MAPPING** describe real geometry but need the
operator (or a look at the console) to say which patched channel is which
before I write coordinates for them.

---

## Room structure — CONFIRMED, revises `norco-location.md`

| Quantity | New measurement | Old estimate | Cross-check |
|---|---|---|---|
| Downstage extension (seam → lip) | **10 ft** | ~9 ft (implied: 22−13) | ✓ |
| Upstage platform depth (seam → columns → back wall) | **9 ft + 2 ft = 11 ft** | 13 ft | −2 ft, self-consistent (see below) |
| Total stage depth | 10 + 11 = **21 ft** | 22 ft (operator estimate) | within 1 ft — same room, better number |

Recomputed, holding the upstage back wall at its known position
(`y = +2.95`, an architectural fact, not something anyone remeasured today):

| Landmark | Old `norco-location.md` Y | New Y (derived) | Method |
|---|---|---|---|
| Upstage wall | +2.95 | **+2.95** (unchanged) | anchor |
| Columns | *(not previously modelled)* | **+2.34** | 2 ft downstage of the back wall |
| Seam (upstage lift → downstage deck) | −1.01 | **−0.40** | 11 ft downstage of the back wall |
| Downstage lip | −3.76 | **−3.45** | 10 ft downstage of the seam |

The seam and lip both moved ~0.3–0.6 m toward the back wall relative to the
old estimate, which is exactly what a 2 ft-shallower upstage platform
predicts — **the new numbers are internally consistent with the old room
depth (21 ft vs. 22 ft), not a contradiction of it.** This is a correction,
not a new room.

### Upstage platform layout — CONFIRMED

Center platform **10 ft wide**, flanked by two **8 ft wide** side platforms
(("hostage platform" in the dictation — near-certain mishearing/mistranscription
of "upstage platform," read that way throughout this doc)):

| Platform | X range (metres) |
|---|---|
| Center | −1.524 to +1.524 |
| Stage-left side | +1.524 to +3.962 |
| Stage-right side | −1.524 to −3.962 |

Total nominal width 10+8+8 = 26 ft, vs. the old undifferentiated "30 ft wide"
upstage lift — the 4 ft difference is very plausibly the columns themselves
plus seams between the three platform sections, which the old single-number
estimate had no way to account for.

### Columns — APPLIED

**32 in wide, 4 ft tall**, sitting at the boundary between the center and
side upstage platforms (i.e. at `x ≈ ±1.524 m`), at `y ≈ +2.34` (2 ft in from
the back wall, per the upstage-depth breakdown above). Not previously
modelled at all. **Added to `room.json`** as two new objects (`Column -
Upstage SL`/`SR`); depth (the Y-thickness) wasn't given, so it's placed as an
8 in guess and should be treated as approximate until measured — width and
height are the real, confirmed numbers.

---

## Floor movers — APPLIED

4 floor movers total (matches the existing 85–88 patch — channel 98 remains
the documented phantom, per `norco-rig-facts.md`).

Checked against the extracted show-file positions before writing anything:
the console's stored X values put the outer pair (85/88) **~7 ft** from the
side walls and the center pair (86/87) **~15 ft** in — nowhere close to
today's measurement (2 ft / 9 ft). Floor movers are portable and
operator-placed by hand in Augment3d (`norco-location.md`: "the builder must
never write channels 85–88... positioned by hand"), so the most likely
explanation is they've physically moved since the show file was last saved,
and today's tape measurement is the current truth. **Applied**:

| Position | Distance from side wall | X (metres, wall at ±5.944) | Channel |
|---|---|---|---|
| Outer pair | 2 ft in | **±5.334** | 85 (−), 88 (+) |
| Center pair | 9 ft in | **±3.201** | 86 (−), 87 (+) |

Channel assignment kept the existing left/right sign from the show file
(85/88 already had the larger `\|x\|`, 86/87 the smaller) — only the
magnitude changed, not which channel is which side. Y/Z/orientation
untouched (floor level, existing hung-upside-down orientation stands; the
"sitting upright" note applies to a different group — the back movers,
below).

---

## Overhead/back movers — reapplied 2026-08-24, twice

First pass (checking the static show-file extraction, before the live
console pull): confirmed correct, no write needed — `fixtures.json` already
had 81/82 truss-hung normally at `z=3.5` and 80/83 flipped-upright at
`z=2.5`, matching "outer ones sit upright, center ones hang normally"
exactly. See `docs/domain/norco-patch-and-groups.md`.

**Then the live pull overwrote it.** Rebuilding `fixtures.json` from the
live OSC read (see the "Status — superseded" section above) replaced *all
four* channels' data with `z=3.2`, `eulers=(0,0,0)` uniformly — the
center/outer split was gone, because the live console state at pull time
didn't carry it (or the pull captured a moment before that structure was
in place). This went unnoticed until the operator re-flagged it directly:
"you didn't seem to capture the new data that the OH movers are mounted
2ft down from the ceiling, and then the outside pair is upright instead of
upside down but around the same height."

**Reapplied, this time keyed to the operator's own numbers rather than the
old static-file values**:

| Chan | Z (was) | Z (now) | Eulers (now) | Quat (now) |
|---|---|---|---|---|
| 80 | 3.2 | **2.8448** | (180, 0, 0) | (w=0, x=1, y=0, z=0) |
| 81 | 3.2 | **2.8448** | (0, 0, 0) | (w=1, x=0, y=0, z=0) |
| 82 | 3.2 | **2.8448** | (0, 0, 0) | (w=1, x=0, y=0, z=0) |
| 83 | 3.2 | **2.8448** | (180, 0, 0) | (w=0, x=1, y=0, z=0) |

`z = 2.8448` is exactly 2 ft below the ceiling (`3.4544 − 0.6096`, from
`room.json`'s `Ceiling` object). All four now share that height — "around
the same height" per the operator's own phrasing, rather than the earlier
static file's `2.5`/`3.5` split, which put the outer pair a full metre
lower. **Both `eulers` and `quat` were updated** — `ignition-viz` renders
from `quat` only; `eulers` is metadata and doesn't affect the picture on
its own, a trap worth remembering next time a position gets hand-edited.

Lesson for next time: **the live pull is authoritative for position, but a
full-file rebuild from it silently discards any hand-applied correction
that isn't also present in the live console state.** Should have
re-verified this specific channel group immediately after the live-pull
rebuild instead of trusting it carried forward.

---

## Par/light groups — resolved 2026-08-24 against the live patch/positions

Cross-checked every one of these against the actual live position of all 48
pars + 8 strips (`docs/domain/norco-patch-and-groups.md`) and the 112 real
group names/membership from the console. Every one of the 48 par channels
falls into a named group or a previously-documented old-rig-facts pattern —
there is no leftover, unaccounted set of pars waiting to be discovered. That
matters: it means the items below that *don't* match are not "unmapped
channels I haven't found yet," they're descriptions that don't correspond to
anything currently in this show's patch/position data at all.

**Confirmed, no change needed**:
- **2 pars angled toward the drum kit** — 42/46, the documented
  drum-kit-convergence pair. Matches exactly.
- **"The center 12 pars"** — channels 19–30, group 94 **`Pars Middle`**.
  Spatially centred (x from −3.6 to +3.6), 12 channel numbers, no better
  match among all 112 groups.
- **"3 pars on the tile line above the upstage edge, 13 ft in," pointed at
  the drum kit** — **channels 43/44/45**, confirmed directly by the
  operator 2026-08-24. My first pass had guessed 90/91/92 (wrong — those
  are strips, not pars, and I hadn't separated 43–45 from the drum-kit
  convergence pair even though they're functionally distinct). The live
  position proves it exactly: `y = −1.01`, which is **exactly** 13 ft
  downstage of the back wall (`2.95 − 13ft(3.962m) = −1.01`, no rounding
  needed at all). Tightly centred (`x = −0.55, 0, 0.55`) with a gentle
  8.6° forward tilt and *no* pan — unlike 42/46's steep ±104° pan, these
  three point straight at the kit from directly overhead rather than
  converging in from the sides. **`norco-rig-facts.md`'s "Drums" group
  (42-46, 50-53) was correct as a set but flattened two functionally
  different sub-groups into one** — the wide-angle convergence pair
  (42/46) and this centred, tile-line trio (43/44/45) — worth noting for
  anyone building effects/palettes off that group.

**Plausible candidates, not confirmed**:
- **"Side rail angled bars... 12 lights on 1 bar"** — best candidate is
  **95/96** (`x = ±5.1–5.25`, nearest the side walls of anything in the
  patch). But their stored rotation is flat (`eulers.z = 0`, no yaw) — the
  "angled" the dictation describes isn't reflected in the console's own
  orientation data for these, so either the model doesn't capture the real
  physical angle (plausible — this is exactly the kind of thing this whole
  measurement session has been correcting elsewhere), or this isn't the
  right pair. Flagged, not applied.

**Doesn't map onto anything in the current patch** — checked and ruled out,
not just "not yet found":
- **The 2 flanking cans at the edge of "the center 12 pars"**, asymmetric
  (1 stage-right instead of 2). The only spatially-adjacent channel
  (31, right next to Pars Middle's edge) sits at the *same* Y as the
  group's own edge, not "a tile back" as described — doesn't fit the
  offset the dictation describes.
- **The back row's "2 and 4" split**, staggered in three depths. Nothing in
  the remaining pars (32–39, already the named `Wide Left`/`Wide Right`
  groups) sits at three different depths — they're all on one row
  (`y = −1.01`).
- **"2 par cans each side, middle of each side section, 2 tiles from the
  back wall"** — no pars sit near `y ≈ 1.73` (2 tiles/4 ft in from the
  `y = 2.95` back wall) other than 90–92, already claimed above.

Most likely explanation for the three that don't map: they describe a
planned change (new fixtures/positions not yet patched or repositioned in
Augment3d) rather than something already in this show, consistent with how
much *else* in today's session turned out to be the operator correcting the
model to match reality rather than the reverse. Worth asking directly next
time, rather than guessing further from position data that's already been
checked as exhaustively as it usefully can be.

---

## Speakers — CONFIRMED position, ambiguous reference side

**6 tiles (12 ft) in from "side far"**, but **5 tiles (10 ft) from the stage
left side** — read as: the two speakers are *not* symmetric (matches the
room's real asymmetry — the booth/closet alcove per `norco-location.md`
exists only on one side), one measured from the far/right wall, the other
from the stage-left wall. `room.json`/`props.json` already has 4 "Speaker"
+ 2 "Speaker Small" entries from the show-file extract; this is a candidate
correction to those, not new objects — needs the existing prop positions
compared against these numbers rather than blindly overwritten.

---

## Status — superseded 2026-08-24 by a live console pull

Everything below this line was written before `fixtures.json` was rebuilt
from a live OSC read of the running Nomad on `voyager` (see
`docs/domain/norco-patch-and-groups.md`). Two things changed as a result:

1. **The floor-mover "2 ft in / 9 ft in" correction below was wrong** — not
   the measurement, the *wall I measured it against*. I used the full 39 ft
   room width (half-width 5.944 m), which is only correct for the wide
   downstage/audience area. Floor movers sit at `y ≈ −1.25`, in the narrower
   **30 ft-wide upstage portion** (half-width 4.572 m, per the "upstage
   platform" section above). Redone against the correct wall: outer pair
   ≈1.88 ft in, center pair ≈10.1 ft in — matches your 2 ft/9 ft within
   normal tape-measure rounding. The live positions already reflect this
   (they were never touched by hand for this correction — they're straight
   off the console); my earlier edit using the wrong wall has been
   overwritten by the live pull, which is strictly better data.
2. **Every fixture position in `fixtures.json` is now the live console
   state**, not the show file's saved-yesterday state — 26 of 71 channels
   had moved by more than 5cm since the file was last saved (the four
   downstage pars 1/2 moved ~5m; that's real repositioning, not
   measurement noise).

The full DMX universe/address for all 71 channels — the original ask that
started the live pull — is in `docs/domain/norco-patch-and-groups.md`. The
par-group questions from the section above are resolved as far as the live
data allows in the **"Par/light groups"** section above (rewritten
2026-08-24 with the live positions and full group membership); speakers are
untouched by the live pull (Eos patch is lighting channels only — TVs and
speakers aren't DMX-addressed) and the partial cross-check from earlier
still stands, still unresolved.

## The 8 bar lights — repositioned 2026-08-24

The operator flagged these as missing; they weren't — all 8 Rockstrip
channels (90–97) were already in the patch and already rendering, just
sitting at their show-file positions rather than where the operator
described ("one underneath each TV and one underneath both columns").
Confirmed explicitly: *"the patch data is good we just need them to show
up in the right spot."* Repositioned, orientation untouched (all 8 already
shared the same `eulers (180,0,0)`, so nothing to change there):

| Chan | New position | Placement |
|---|---|---|
| 90 | (−3.5, 2.4649, 0.20) | under the centre-left TV (60") |
| 91 | (0, 2.4649, 0.20) | under the centre TV (65") |
| 92 | (3.5, 2.4649, 0.20) | under the centre-right TV (60") |
| 96 | (4.8992, −2.0568, 0.05) | under TV – Flare SL (65") |
| 95 | (−4.8992, −2.0568, 0.05) | under TV – Flare SR (65") |
| 94 | (1.524, 2.34, 0.05) | under Column – Upstage SL |
| 93 | (−1.524, 2.34, 0.05) | under Column – Upstage SR |
| 97 | (0, 2.34, 0.05) | under the centre platform, between the columns |

Two columns don't divide evenly into the 3 remaining bars once the 5 TV
positions are accounted for — read "one underneath both columns" as
covering that whole back-platform area rather than literally 1 bar per
column, and put the third (97) centred between them. Flagging the
read, not hedging the placement — worth a quick confirm next time the
operator's in front of the rig.

## Outer OH mover height — recomputed by lens, not mount, 2026-08-24

The outer OH pair (chan 80/83) went through three attempts before landing
right. First: mount box sitting directly on the column's own top
(1.778m) — operator: "way lower" than expected. Second: outer *mount*
height offset −6in from the center pair's mount height (2.6924m) — still
wrong, because the operator's actual complaint was about the visible
business end (lens), and the 180° hang-vs-upright flip means the lens
sits at the *opposite end of the mesh* from the mount point for each
pair: the center pair hangs (lens at the mesh's bottom, mount at top),
the outer pair stands upright on its mount box (lens at the mesh's top,
mount at the bottom). Aligning *mount* heights therefore misaligns *lens*
heights by roughly twice the mesh height.

Recomputed from the operator's own framing ("mounted lower [but] the lens
just ends up roughly in the same position"):

```
center_lens  = center_mount(2.8448) − mesh_height(0.235) = 2.6098
target_outer_lens = center_lens − 6in(0.1524)             = 2.4574
outer_mount  = target_outer_lens − mesh_height(0.235)      = 2.2224
```

`fixtures.json` chan 80/83 `position.z` → 2.2224; `room.json`'s two
"Mount Box - OH Outer" entries repositioned under it (bottom 1.6636,
centre 1.943, unchanged 0.5588m height).

Separately, the mount box turned out to be rendering correctly all
along — it was just the same colour as the pillar/beam right next to it
(`PILLAR_COLOR`), so it read as "not there." Gave it its own
`MOUNT_BOX_COLOR` (medium grey, distinct from both the near-black
pillar/beam and the cooler wall grey) in `scene.rs`.

## 4 drum-fill pars — attached to the pillars, not floating, 2026-08-24

Chan 50/51/52/53 (Chauvet SlimPAR Tri 7 IRC, the drum-kit-convergence
pair's overhead fill) weren't in `fixture_profile.rs`'s shape table at
all — every one of them fell back to the generic floating box+cone
marker with no mesh and nothing anchoring it to the rig. Two separate
bugs, both from the same root cause (never having a real shape/mount
model for this fixture):

1. **No par-can mesh.** Added a `chauvet`/`slimpar` match in
   `shape_for()` (target size 0.16m, same `par_mesh()` QLC+ asset the
   Uking pars already use, `Anchor::None` — the yoke-clamp origin already
   reads as the mount point).
2. **Sitting behind the column, not in front of it.** The live position
   had these at `y = 2.85` — 0.1m off the back wall (`y = 2.95`),
   *upstage* of the column/pillar structure (`y = 2.34`). From every
   audience-facing camera angle the solid column occluded them entirely
   — which also directly contradicts "they go in the audience side."
   Operator's call: `y` should be based directly on the pillar, not a
   separate plane — set to `y = 2.34`, the pillar's own `y`, so the pars
   sit at the same depth as the thing they're clamped to. (First pass
   used `y = 2.2`, matching the outer OH movers' plane instead — closer
   than the original 2.85, but not what was asked for.)
   `x` also snapped flush against the pillar's outer face
   (`column/pillar x ± half-width(0.09) + half the par mesh's own depth`,
   the same computed-not-guessed approach used for the OH mount box) —
   was floating 0.48m out in open air with nothing bridging the gap.

Not fully confirmed: the real mounting hardware (a clamp/bracket
connecting the par to the pillar) isn't modelled, and the exact stand-off
distance is derived from mesh geometry, not a field measurement of this
specific bracket. Flagged for a look next time the operator's at the rig.

## Ceiling corrected to 9ft — collapses the outer-mover lens-alignment hack, 2026-08-24

The three-attempt saga on the outer OH mover height (this doc, "Outer OH
mover height — recomputed by lens, not mount") was chasing a symptom, not
the cause. Operator, after confirming the column (4ft) and mount box
(22in) heights independently: **"we shouldn't have to move them up to
match the top ones. That tells me our ceilings are too high."** Confirmed:
**ceiling is 9ft** (2.7432m), not 11'4" (3.4544m, the number every wall
height had been cross-checked against and was itself apparently wrong,
not a bug in the cross-check).

With the real ceiling, the two independently-confirmed measurements
(4ft column + 22in box) can be **stacked directly** — box bottom = column
top, no lens-position algebra required — and it *already* produces
"outer lens slightly lower than center," matching the operator's original
description, without forcing it:

| Object | Old (11'4" ceiling) | New (9ft ceiling) |
|---|---|---|
| Ceiling | 3.4544 | **2.7432** |
| Every full-height wall | `size.z` s.t. base+height=3.4544 | reduced by 0.7112 (the ceiling drop), base unchanged |
| Beam - OH Mount (bottom, "2ft below ceiling") | 2.8448 | **2.1336** |
| Center OH movers (chan 81/82) | 2.8448 | **2.1336** |
| Pillar (top = beam bottom, bottom = column top, unchanged) | 1.2192–2.8448 (h=1.6256) | **1.2192–2.1336 (h=0.9144)** |
| Mount Box - OH Outer (bottom = column top, unchanged) | 1.6636–2.2224 | **1.2192–1.7780** |
| Outer OH movers (chan 80/83) | 2.2224 | **1.7780** |

`Wall - Booth Front` (a partial-height divider, top 1.6256, never reached
the old ceiling either) is untouched — this only rescales objects whose
top matched the ceiling to begin with. The annex walls (Closet/Alcove,
also full-height in the source data) were rescaled the same way for
consistency, though they sit outside the camera-framing bounds and
haven't been independently field-checked.

## Drum-fill pars — centred on the pillar face, not offset to its side

Follow-up to the two pars fixes above: **"The pars should be in the
middle of the wooden beam not to the sides of it."** Chan 50/51/52/53
had `x` snapped flush against the pillar's *side* face (offset from the
pillar's own centreline) — moved to `x` = the pillar's own `x` exactly
(centred, matching "the middle") and `y` moved instead, to the pillar's
*front* (audience-facing) face: `pillar_y − half_depth(0.09) − half the
par mesh's own depth(0.08) = 2.17`. `z` rescaled to the same relative
stacked heights within the pillar's new (shorter, post-ceiling-fix)
1.2192–2.1336 range that the original two heights held in the old range.

## TVs lowered — clipping the new 9ft ceiling, 2026-08-24

Follow-up to the ceiling correction above: the TVs' Z positions weren't
touched by that fix (they aren't wall/ceiling-referenced geometry, they
came from the original show-file extraction), so relative to the new,
7 in-lower ceiling they now crowded it — the two angled "Flare" TVs
(eulers `x=98°`) worst of all: top came out to 2.744m, essentially
exactly at the new 2.7432m ceiling, i.e. clipping through it.

No field measurement exists for intended TV mount height, so this is an
eyeball call, not a confirmed number like the rest of this doc: lowered
all 5 so every TV's top sits a consistent 16in (0.4m) below the new
ceiling, accounting for each screen's own tilt (the two Flare TVs are 8°
off vertical, so their local "up" axis isn't pure +Z — used the actual
rotated axis, not size.y directly, so all 5 land on the same real-world
top height despite different tilts/sizes):

| Screen | Z (was) | Z (now) |
|---|---|---|
| TV - Flare SL | 1.9425 | **1.5417** |
| TV - Flare SR | 1.9425 | **1.5417** |
| TV (x=3.5) | 1.63 | **1.596** |
| TV (x=0) | 1.63 | **1.5338** |
| TV (x=-3.5) | 1.63 | **1.596** |

Worth a real measurement next time the operator's at the rig — 16in is a
reasonable-looking default, not a dictated number.

## Center back-wall TV lowered again — was hidden behind the beam

The three back-wall TVs (x=−3.5/0/+3.5) sit at `y=2.4649`, upstage of
the "Beam - OH Mount" arch (`y=2.34`) — so as seen from the audience,
the beam is the nearer object and anything taller than the beam's
bottom edge (`2.1336`, per the ceiling-fix section above) reads as
poking up *behind* it, not clearing underneath it. The center TV's top
(`2.3432`, from the ceiling-clearance pass) exceeded that, so it visibly
overlapped the beam.

Operator: **"the middle 3 they are at the same height"** — read as "lower
all three back-wall TVs together, keep them uniform," not just the one
that visibly clips. Set all three to the same top height, clear of the
beam: `beam_bottom(2.1336) − 0.15m clearance = 1.9836`, back-solved per
TV's own `size.y` (they aren't all the same height):

| Screen | Z (was, ceiling-pass) | Z (now) |
|---|---|---|
| TV (x=3.5) | 1.596 | **1.2364** |
| TV (x=0) | 1.5338 | **1.1742** |
| TV (x=-3.5) | 1.596 | **1.2364** |

The two Flare TVs are outside the beam's x-span (beam runs x ±1.624,
Flare TVs sit at x=±4.8992) and don't have this problem — left at their
ceiling-clearance heights from the prior fix.

## OH mount beam: 1.5ft below ceiling, not 2ft, 2026-08-24

Operator: **"let's have the top across beam by 1.5ft down from ceiling
instead of 2 feet down."** Cascades the same way the ceiling fix did —
everything hung off "beam is X below ceiling" moves with it, everything
independently anchored (column, outer mount box, stacked on the column
top) does not:

| Object | 2ft-below (previous) | 1.5ft-below (now) |
|---|---|---|
| Beam - OH Mount (bottom) | 2.1336 | **2.286** |
| Center OH movers (chan 81/82) | 2.1336 | **2.286** |
| Pillar (top = beam bottom, bottom = column top, unchanged) | 1.2192–2.1336 (h=0.9144) | **1.2192–2.286 (h=1.0668)** |
| Drum-fill pars (chan 50/51 low, 52/53 high) — same relative stack position on the pillar | 1.6021 / 1.8834 | **1.6659 / 1.9941** |

Outer OH movers and their mount box are unaffected — they stack directly
on the column top (4ft, confirmed), independent of the beam. The
back-wall TVs' beam-clearance target (1.9836) still clears the new,
higher beam bottom (2.286), so they didn't need touching either.
