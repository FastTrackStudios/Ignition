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

## Overhead/back movers — ALREADY CORRECT, no write needed

This looked like the highest-priority bug in the set, but checking the
actual extracted data instead of the old eos-toolkit prose it quotes from
resolved it immediately: **`fixtures.json` does *not* treat channels 80–83
uniformly**, and the asymmetry it already has matches this measurement
exactly.

| Chan | X | Z | Eulers | Reading |
|---|---|---|---|---|
| 80 | −2.33 | **2.50** | (0, −180, −180) | outer, flipped upright |
| 81 | −1.00 | **3.50** | (0, 0, 0) | center, truss-hung |
| 82 | +1.00 | **3.50** | (0, 0, 0) | center, truss-hung |
| 83 | +2.33 | **2.50** | (0, −180, −180) | outer, flipped upright |

81/82 (center, `x = ±1`) are truss-hung at `z = 3.5` — the "2 ft down from the
ceiling" group. 80/83 (outer, `x = ±2.33`, sitting just outside the columns
at `x = ±1.524`) already carry a 180° flip on Y and Z — exactly "sitting
upright instead of mounted upside-down" — at a lower `z = 2.5`, consistent
with standing on a box rather than hanging from the truss.

This means **the console's own model was already corrected** before this
measurement session — the operator fixed it in Augment3d, and this
dictation was describing/confirming that fix, not reporting a bug in
`fixtures.json`. The stale claim ("80-83 eul (0,0,0), hung straight down")
was in eos-toolkit's older `norco-location.md`, from before that fix — this
project's own extraction is already ahead of it. No JSON change made here.
`docs/domain/norco-venue-reference.md`'s fixture table should be read as
superseded by this table for channels 80/83 specifically.

**What I need before writing this into the JSON**: which of 80/81/82/83 are
"center" vs. "outer"? The overhead layout table in `norco-rig-facts.md`
doesn't distinguish them (all four listed identically), and I'm not willing
to guess a channel-to-role mapping for a live rig's orientation data. My best
guess by typical numbering convention would be 81/82 = center, 80/83 = outer,
but that's a guess, not a read-back — flagging per this project's own rule
rather than writing it silently.

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
