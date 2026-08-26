#!/usr/bin/env python3
"""Builds `data/venues/riverside/*.json` from the tape-measure numbers.

Riverside is measured, not extracted: unlike Norco (which came out of a
real Eos `.esf3d` — see `docs/domain/norco-venue-reference.md`), there is
no show file to derive from, so the room is authored parametrically from
the operator's own measurements. Keeping it a generator rather than
hand-edited JSON means a re-measure is a one-line change here, not a
hunt through 200 coordinate literals.

Every number below sits in one of three tiers, and each is labelled:

  MEASURED     the operator's tape. Trust it.
  PHOTO        counted or estimated off the room photos. Plausible, and
               the first thing a real patch list should overwrite.
  ASSUMED      neither measured nor visible — a working value chosen so
               the room renders. Correct freely.

Convention, identical to the Norco extract so both venues load through
the same `ignition_viz::venue::Venue`:

    +X stage left   +Y upstage   +Z up      metres
    origin = centre of the stage deck, at deck level (z = 0 on the deck,
             so the room floor is at z = -STAGE_H)

Run: python3 tools/build_riverside_venue.py
"""

import json
import math
import pathlib

IN = 0.0254  # one inch, in metres


def ft(feet: float, inches: float = 0.0) -> float:
    return (feet * 12.0 + inches) * IN


def r(v: float) -> float:
    """Round to 0.1 mm — the JSON is data, not a float-noise museum."""
    return round(v + 0.0, 4)


def v3(x, y, z):
    return {"x": r(x), "y": r(y), "z": r(z)}


# =====================================================================
# MEASURED — the operator's tape, 2026-08-25.
# =====================================================================
STAGE_W = ft(19, 8)     # 19' 8"  front-of-stage width
STAGE_D = ft(12, 0)     # 12' 0"  back wall to stage lip
STAGE_H = ft(0, 20)     # 20"     deck height above the room floor
ROOM_W = ft(21, 0)      # 21' 0"  back-of-room width
BEAM_H = ft(19, 6)      # 19' 6"  room floor up to the beam

POLE_PITCH = ft(0, 64)  # 64"     light pole to light pole, centre to centre
PAR_FIRST = ft(0, 40)   # 40"     lowest par, above the deck
PAR_PITCH = ft(2, 0)    # 2' 0"   par to par up a pole
BACKWALL_H = ft(10, 8)  # 10' 8"  the back wall bar lights

# How much the whole back-wall lighting structure is raised above the
# measured heights: the poles get taller, and the pars, movers and bars
# all rise with them, keeping their measured spacing to each other. The
# operator's call after seeing the first model — the rig reads low in the
# room. ONE number: everything on that wall moves together, so re-run
# after changing it and nothing goes out of register.
BACKWALL_LIFT = ft(1, 0)

# =====================================================================
# PHOTO — counted off the room photos, not measured.
# =====================================================================
BEAM_PAR_COUNT = 6      # pars in the row along the beam
BEAM_STRIP_COUNT = 2    # amber LED strips under the beam
FOH_LEFT_COUNT = 3      # pars house-left of the arch, on the back wall
FOH_RIGHT_COUNT = 4     # pars house-right of the arch, on a T-bar
FOH_HIGH_COUNT = 1      # the odd par mounted above the T-bar
FOH_Z = 3.60            # FOH par height above the deck
FOH_HIGH_Z = 4.30       # the high one
ARCH_W, ARCH_H = 1.30, 2.60   # the red neon arch on the back-of-house wall

# =====================================================================
# ASSUMED — no measurement and nothing legible in a photo.
# =====================================================================
HOUSE_D = ft(18, 0)     # audience depth, stage lip to back of room
POLE_H = 3.0            # pole top: ~6" of pole above the topmost par
                        # (before BACKWALL_LIFT, which is added below)
POLE_TOP = POLE_H + BACKWALL_LIFT   # the poles stand on the deck and grow
MOVER_Z = POLE_TOP + 0.12  # mover body centre, sitting on the pole top
POLE_Y_INSET = 0.20     # pole centre, downstage of the back wall
FIXTURE_Y_INSET = 0.12  # fixture face, downstage of its pole centre
BAR_LEN = 1.2           # the three back-wall bars
STRIP_LEN = 1.0         # the beam's amber strips
WALL_INSET = 0.15       # how far a wall-mounted fixture stands off its wall

# Derived
HALF_W = STAGE_W / 2.0
UPSTAGE_Y = STAGE_D / 2.0
LIP_Y = -STAGE_D / 2.0
FLOOR_Z = -STAGE_H
BEAM_Z = BEAM_H + FLOOR_Z          # beam, expressed above the deck
HOUSE_BACK_Y = LIP_Y - HOUSE_D
HOUSE_MID_Y = (LIP_Y + HOUSE_BACK_Y) / 2.0
POLE_Y = UPSTAGE_Y - POLE_Y_INSET
FIXTURE_Y = POLE_Y - FIXTURE_Y_INSET

# Four poles, centred on the stage, 64" apart: a 16' overall span.
POLE_X = [r(-1.5 * POLE_PITCH + i * POLE_PITCH) for i in range(4)]


def spread(count, width, centre=0.0):
    """`count` evenly spaced positions filling `width`, centred."""
    if count == 1:
        return [r(centre)]
    step = width / (count - 1)
    return [r(centre - width / 2.0 + i * step) for i in range(count)]


# ---------------------------------------------------------------------
# Aim
# ---------------------------------------------------------------------
def aim(pos, target):
    """The mount rotation that points a fixture's default -Z at `target`.

    Returns `(eulers, quat)` in the venue JSON's own shape. Eulers are
    ZYX with no roll, matching `venue::euler_to_quat`; the quaternion is
    the same rotation, and the loader reads the quaternion.

    This is the **hang**, exactly as `docs/domain/norco-venue-reference.md`
    insists: for a fixed par or bar the mount angle *is* the aim, so
    solving it from a target is the same statement as writing the angle
    down. A mover is different and never goes through here — its hang is
    which way the base is bolted, and pan/tilt is live show data.
    """
    dx, dy, dz = (target[i] - pos[i] for i in range(3))
    n = math.sqrt(dx * dx + dy * dy + dz * dz)
    if n == 0:
        return v3(0, 0, 0), {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0}
    dx, dy, dz = dx / n, dy / n, dz / n
    tilt = math.acos(max(-1.0, min(1.0, -dz)))       # about +X
    yaw = math.atan2(-dx, dy) if math.sin(tilt) > 1e-6 else 0.0  # about +Z
    # Rz(yaw) * Rx(tilt), as a quaternion.
    cz, sz = math.cos(yaw / 2.0), math.sin(yaw / 2.0)
    cx, sx = math.cos(tilt / 2.0), math.sin(tilt / 2.0)
    quat = {"w": r(cz * cx), "x": r(cz * sx), "y": r(sz * sx), "z": r(sz * cx)}
    return v3(math.degrees(tilt), 0.0, math.degrees(yaw)), quat


def fixture(chan, name, tags, model, pos, target, size, beam_deg, hang=None):
    eulers, quat = aim(pos, target) if hang is None else hang
    record = {
        "chan": chan,
        "name": name,
        "tags": tags,
        "patched": True,
        "manufacturer": "",
        "model": model,
        "position": v3(*pos),
        "eulers": eulers,
        "quat": quat,
        "size": v3(*size),
        "beam_angle_deg": beam_deg,
    }
    # `patch.json` is the authority on manufacturer, model and address —
    # see `apply_patch` — and is merged in after every fixture is built.
    return record


def apply_patch(fixtures, patch):
    """Merges `patch.json` onto the fixtures, by channel.

    Placement and patch are two different facts about a fixture and they
    arrive from two different places: geometry is measured off the room,
    while manufacturer / model / universe / address come off the console.
    Keeping the patch in its own file — the same shape Norco's
    `patch.json` already has — means the operator can hand over a patch
    list without touching a single coordinate, and re-running this script
    never overwrites it.

    A fixture with no patch entry keeps a blank manufacturer and no
    address at all, which is what `FixtureRecord::dmx_address()` reads as
    "not DMX-controlled". That is deliberate: a fixture the console
    cannot reach should render dark rather than silently pick up whatever
    happens to live at some invented address.
    """
    by_chan = {int(e["chan"]): e for e in patch}
    hit = 0
    for f in fixtures:
        entry = by_chan.get(f["chan"])
        if entry is None:
            continue
        hit += 1
        f["manufacturer"] = entry.get("manufacturer", "")
        f["model"] = entry.get("model", f["model"])
        f["patched"] = entry.get("patched", True)
        if entry.get("universe") is not None:
            f["universe"] = entry["universe"]
        if entry.get("address") is not None:
            f["address"] = entry["address"]
    return hit


# ---------------------------------------------------------------------
# The real console patch.
# ---------------------------------------------------------------------
# `console-show.json` is the desk's own patch and fixture library, read
# out of the myDMX 5 showfile by `tools/dvc_parse.py`: 57 fixtures, one
# universe, 419 channels, verified non-overlapping. It is ground truth
# for address, footprint, type and channel layout.
#
# Two of its groups map to modelled positions with no ambiguity:
#
#   WALL    16 x Solena Professional Max Par 54 RGB, 7ch, @250..355
#   MOVERS   4 x Mini Gobo Moving Head Light,       11ch, @400..433
#
# The desk's own 2D stage view settles the ordering. The WALL block sits
# at POSX 430/490/550/610 (four poles, left to right) x POSY -230/-190/
# -150/-110 (four rows). Smaller POSY is higher up the pole, so address
# ascends DOWN a pole and then moves to the next pole:
#
#     POSX      430  490  550  610      <- pole 1..4, house left to right
#     POSY -230 250  278  306  334      <- par row 4 (top, 9'4")
#          -190 257  285  313  341      <- row 3
#          -150 264  292  320  348      <- row 2
#          -110 271  299  327  355      <- row 1 (bottom, 3'4")
#
# This is why the group's own ordering is not the address ordering: the
# desk lists WALL row-major (250, 278, 306, 334, ...) while the patch runs
# pole-major. Reading either as the other transposes the grid — a chase
# meant to sweep across the poles would instead run up and down one.
WALL_ADDR = {  # (pole 1-4, row 1-4 bottom-up) -> address
    (pole, row): 250 + (pole - 1) * 28 + (4 - row) * 7
    for pole in range(1, 5) for row in range(1, 5)
}
# Movers, ordered by the desk's POSX (110, 323, 676, 841) -> poles 1..4.
# Note the desk draws them wider apart than the poles they sit on; its 2D
# view is schematic, so it is trusted for order and not for position.
MOVER_ADDR = {1: 433, 2: 411, 3: 400, 4: 422}

CONSOLE_FIXTURES = {}
for pole in range(1, 5):
    for row in range(1, 5):
        CONSOLE_FIXTURES[(pole - 1) * 4 + row] = (
            WALL_ADDR[(pole, row)], 7, "Solena Professional Max Par 54 RGB")
    CONSOLE_FIXTURES[16 + pole] = (
        MOVER_ADDR[pole], 11, "Mini Gobo Moving Head Light")

# Everything else this venue models still has no position->fixture link,
# so it keeps a placeholder type and a placeholder address.
PROVISIONAL_TYPES = {
    "Par": ("Uking", "Par", 7),
    "Moving Head": ("Betopper", "Beam Moving Head", 12),
    "Bar": ("Rockville", "Rockstrip 252 7ch", 7),
}


def provisional_patch(fixtures):
    """A sequential single-universe patch, so the room is drivable today.

    Written only when there is no `patch.json` yet, and every entry is
    flagged `"provisional": true`. It exists so cues, roles and the
    studio can be exercised against this room before the console has been
    read — not as a claim about where anything actually is addressed.
    """
    # Real entries first, so the placeholder allocator can route around
    # the addresses they really occupy.
    real, taken = {}, set()
    for chan, (addr, footprint, model) in CONSOLE_FIXTURES.items():
        real[chan] = {
            "chan": chan,
            # No manufacturer: the showfile records a library path, not a
            # separate manufacturer field, and `channel_map_for` matches
            # this model string exactly.
            "manufacturer": "",
            "model": model,
            "patched": True,
            "universe": 1,
            "address": addr,
            "footprint": footprint,
            "source": "console-show.json",
        }
        taken.update(range(addr, addr + footprint))

    patch, universe, next_addr = [], 1, 1
    for f in fixtures:
        if f["chan"] in real:
            patch.append(real[f["chan"]])
            continue
        maker, model, footprint = PROVISIONAL_TYPES[f["model"]]
        # Skip over anything the console already uses, so a placeholder
        # can never sit on top of a real fixture and drive it by accident.
        while any(a in taken for a in range(next_addr, next_addr + footprint)):
            next_addr += 1
        if next_addr + footprint - 1 > 512:
            universe += 1
            next_addr = 1
        patch.append({
            "chan": f["chan"],
            "manufacturer": maker,
            "model": model,
            "patched": True,
            "universe": universe,
            "address": next_addr,
            "footprint": footprint,
            "provisional": True,
            "notes": ["No position->fixture link yet. See README."],
        })
        taken.update(range(next_addr, next_addr + footprint))
        next_addr += footprint
    return patch


PAR_SIZE = (0.3097, 0.3057, 0.3622)
MOVER_SIZE = (0.24, 0.24, 0.34)

# =====================================================================
# Room
# =====================================================================
# `spawn.rs` reads these name prefixes: "Wall"/"Face" pivot at their BASE,
# everything else at its centre; "Ceiling"/"Stage"/"Floor"/"Column"/"Beam"
# each get their own surface colour.
room = [
    {"name": "Stage - Deck",
     "position": v3(0, 0, 0), "eulers": v3(0, 0, 0),
     "size": v3(STAGE_W, STAGE_D, 0.0)},
    {"name": "Face - Stage Lip",
     "position": v3(0, LIP_Y, FLOOR_Z), "eulers": v3(0, 0, 0),
     "size": v3(STAGE_W, 0.0, STAGE_H)},
    {"name": "Floor - House",
     "position": v3(0, HOUSE_MID_Y, FLOOR_Z), "eulers": v3(0, 0, 0),
     "size": v3(ROOM_W, HOUSE_D, 0.0)},
    {"name": "Wall - Upstage",
     "position": v3(0, UPSTAGE_Y, FLOOR_Z), "eulers": v3(0, 0, 0),
     "size": v3(ROOM_W, 0.0, BEAM_H)},
    {"name": "Wall - House Back",
     "position": v3(0, HOUSE_BACK_Y, FLOOR_Z), "eulers": v3(0, 0, -180),
     "size": v3(ROOM_W, 0.0, BEAM_H)},
    {"name": "Wall - Stage Left",
     "position": v3(ROOM_W / 2.0, (UPSTAGE_Y + HOUSE_BACK_Y) / 2.0, FLOOR_Z),
     "eulers": v3(0, 0, 0),
     "size": v3(0.0, UPSTAGE_Y - HOUSE_BACK_Y, BEAM_H)},
    {"name": "Wall - Stage Right",
     "position": v3(-ROOM_W / 2.0, (UPSTAGE_Y + HOUSE_BACK_Y) / 2.0, FLOOR_Z),
     "eulers": v3(0, 0, -180),
     "size": v3(0.0, UPSTAGE_Y - HOUSE_BACK_Y, BEAM_H)},
    {"name": "Beam - House",
     "position": v3(0, HOUSE_MID_Y, BEAM_Z), "eulers": v3(0, 0, 0),
     "size": v3(ROOM_W, 0.3048, 0.3048)},
    {"name": "Ceiling",
     "position": v3(0, (UPSTAGE_Y + HOUSE_BACK_Y) / 2.0, BEAM_Z + 0.1524),
     "eulers": v3(0, 0, 0),
     "size": v3(ROOM_W, UPSTAGE_Y - HOUSE_BACK_Y, 0.0)},
]

# The four light poles the back wall rig hangs on — modelled as room
# structure so a fixture can be rigged to one, same as Norco's columns.
for i, x in enumerate(POLE_X, start=1):
    room.append({
        "name": f"Column - Pole {i}",
        "position": v3(x, POLE_Y, POLE_TOP / 2.0), "eulers": v3(0, 0, 0),
        "size": v3(0.09, 0.09, POLE_TOP),
    })

# =====================================================================
# Fixtures
# =====================================================================
# Channel blocks are left with gaps between positions so a real patch can
# land without renumbering: 1-23 back wall, 31-38 beam, 41-49 FOH.
fixtures = []

# --- Back wall (stage), chans 1-23 — the measured position -----------
# Poles are numbered house-left to house-right, i.e. ascending +X, which
# is stage right to stage left. Rows are numbered bottom-up.
for c, x in enumerate(POLE_X, start=1):
    for row in range(1, 5):
        z = PAR_FIRST + (row - 1) * PAR_PITCH + BACKWALL_LIFT
        pos = (x, FIXTURE_Y, z)
        fixtures.append(fixture(
            (c - 1) * 4 + row, f"Pole {c} Par {row}",
            ["Luminaire_LED_Wash", "Back Wall", f"Pole {c}", f"Par Row {row}"],
            "Par", pos, (x, LIP_Y - 1.0, 1.0), PAR_SIZE, 30.0))

# A mover on each pole top. Base-down on the pole, so the hang is a
# 180-degree flip and the head points up: the hang, not the aim.
MOVER_HANG = (v3(180.0, 0.0, 0.0), {"w": 0.0, "x": 1.0, "y": 0.0, "z": 0.0})
for c, x in enumerate(POLE_X, start=1):
    fixtures.append(fixture(
        16 + c, f"Pole {c} Mover",
        ["Luminaire_LED_Yoke_Spot", "Movers All", "Back Wall", f"Pole {c}"],
        "Moving Head", (x, POLE_Y, MOVER_Z), None, MOVER_SIZE, 14.0,
        hang=MOVER_HANG))

# Three bars across the top of the back wall, aimed straight downstage.
for i, x in enumerate(spread(3, STAGE_W * 2.0 / 3.0), start=1):
    pos = (x, UPSTAGE_Y - 0.10, BACKWALL_H + BACKWALL_LIFT)
    fixtures.append(fixture(
        20 + i, f"Back Wall Bar {i}",
        ["Luminaire_LED_Bar", "Luminaire_LED_Wash", "Back Wall", "Bars"],
        "Bar", pos, (x, LIP_Y, BACKWALL_H + BACKWALL_LIFT - 0.4),
        (BAR_LEN, 0.09, 0.09), 40.0))

# --- Beam (overhead, mid-house), chans 31-38 -------------------------
# The row of pars along the house beam plus the two amber strips slung
# under it — top light over the stage, and the only thing at Riverside
# that can key the band from in front. Each par aims at the patch of
# stage directly ahead of it, so the row covers the width.
for i, x in enumerate(spread(BEAM_PAR_COUNT, ROOM_W * 0.8), start=1):
    pos = (x, HOUSE_MID_Y, BEAM_Z - 0.25)
    fixtures.append(fixture(
        30 + i, f"Beam Par {i}",
        ["Luminaire_LED_Wash", "Beam", "Front Wash"],
        "Par", pos, (x * (STAGE_W / (ROOM_W * 0.8)), 0.0, 1.5),
        PAR_SIZE, 30.0))

for i, x in enumerate(spread(BEAM_STRIP_COUNT, ROOM_W * 0.45), start=1):
    pos = (x, HOUSE_MID_Y + 0.35, BEAM_Z - 0.30)
    fixtures.append(fixture(
        30 + BEAM_PAR_COUNT + i, f"Beam Strip {i}",
        ["Luminaire_LED_Bar", "Luminaire_LED_Wash", "Beam", "Bars"],
        "Bar", pos, (x, HOUSE_MID_Y + 0.35, 0.0),
        (STRIP_LEN, 0.08, 0.08), 40.0))

# --- Front of house (back-of-room wall), chans 41-49 -----------------
# Two brackets flanking the arch, plus the one par mounted above the
# house-right bracket. These are the front light on faces.
FOH_Y = HOUSE_BACK_Y + WALL_INSET
foh = []
for x in spread(FOH_LEFT_COUNT, 0.55 * (FOH_LEFT_COUNT - 1), centre=-1.9):
    foh.append((x, FOH_Z))
for x in spread(FOH_RIGHT_COUNT, 0.55 * (FOH_RIGHT_COUNT - 1), centre=1.9):
    foh.append((x, FOH_Z))
for x in spread(FOH_HIGH_COUNT, 0.55, centre=1.9):
    foh.append((x, FOH_HIGH_Z))
for i, (x, z) in enumerate(foh, start=1):
    pos = (x, FOH_Y, z)
    # Aimed at the downstage third of the stage, fanned across its width
    # — front light wants the faces, not the back wall.
    fixtures.append(fixture(
        40 + i, f"FOH Par {i}",
        ["Luminaire_LED_Wash", "FOH", "Front Wash", "Key"],
        "Par", pos, (x * 0.5, LIP_Y + 0.8, 1.55), PAR_SIZE, 30.0))

# =====================================================================
# Groups — the selections this rig is actually programmed against.
# =====================================================================
groups = []


def group(label, channels):
    groups.append({"target": str(len(groups) + 1), "label": label,
                   "channels": channels})


BEAM_PAR_CH = [30 + i for i in range(1, BEAM_PAR_COUNT + 1)]
BEAM_STRIP_CH = [30 + BEAM_PAR_COUNT + i for i in range(1, BEAM_STRIP_COUNT + 1)]
FOH_CH = [40 + i for i in range(1, len(foh) + 1)]


def rng(chans):
    return [f"{chans[0]}-{chans[-1]}"] if len(chans) > 1 else [str(chans[0])]


group("Back Wall Pars", ["1-16"])
for c in range(1, 5):
    group(f"Pole {c}", [f"{(c - 1) * 4 + 1}-{(c - 1) * 4 + 4}"])
for row in range(1, 5):
    group(f"Par Row {row}", [row, row + 4, row + 8, row + 12])
group("Movers", ["17-20"])
group("Back Wall Bars", ["21-23"])
group("Back Wall All", ["1-23"])
group("Beam Pars", rng(BEAM_PAR_CH))
group("Beam Strips", rng(BEAM_STRIP_CH))
group("Beam All", rng(BEAM_PAR_CH + BEAM_STRIP_CH))
group("FOH Pars", rng(FOH_CH))
# The two layers the profile's Key/Wash roles bind to. Front Wash is
# everything that lights the band from in front of them; Strips All is
# every linear fixture in the room regardless of position, which is what
# a charted hit wants.
group("Front Wash", rng(FOH_CH) + rng(BEAM_PAR_CH))
group("Strips All", ["21-23"] + rng(BEAM_STRIP_CH))
group("Pars All", ["1-16"] + rng(BEAM_PAR_CH) + rng(FOH_CH))

# =====================================================================
# Palettes — the room's focus points. Colours are deliberately NOT
# defined here: `venue::inherit_colors` fills them from the profile, so
# Riverside means the same thing by "Deep Blue" as every other room
# until it has a reason not to.
# =====================================================================
# The band, as the operator described it — and they described it from the
# *audience's* side of the room ("back left corner", not "upstage right"),
# so `left` here means house left, which is stage right, which is -X. The
# palette below keeps the stage's own naming, because that is what a desk
# says: the bass player at house left stands on `Vocal Stage Right`.
#
#         ┌──────────────── upstage wall ────────────────┐
#         │  DRUMS          GUITAR           KEYS        │   back line
#         │                                              │
#         │  BASS           VOCAL                        │   front line
#         └───────────── stage lip / house ──────────────┘
#            house left        centre        house right
#            (-X, stage R)                   (+X, stage L)
VOCAL_Y = LIP_Y + 0.75      # the front line: just upstage of the lip
BAND_Y = UPSTAGE_Y - 1.10   # the back line
HEAD_Z = 1.55               # face height, standing on the deck
OUTER_X = HALF_W * 0.68     # how far out the outer two players stand

# The kit goes in the house-left back corner. Its centre sits half its own
# depth off the upstage wall, and half its own width off the stage edge,
# so the whole footprint lands on the deck rather than the middle of it
# being at the corner.
KIT_W, KIT_D, KIT_H = 1.926, 1.632, 1.206
DRUM_X = -(HALF_W - KIT_W / 2.0 - 0.15)
# Far enough off the wall to clear the light poles, which stand on the
# deck at y = POLE_Y — a kit pushed flat against the back wall would have
# pole 1 coming up through the floor tom.
DRUM_Y = UPSTAGE_Y - KIT_D / 2.0 - 0.45

GUITAR_X, GUITAR_Y = 0.0, BAND_Y          # back centre
KEYS_X, KEYS_Y = OUTER_X, BAND_Y          # back, house right
BASS_X, BASS_Y = -OUTER_X, VOCAL_Y        # front, house left
LEAD_X, LEAD_Y = 0.0, VOCAL_Y             # front centre

focus = [
    # The blocking grid, in the stage's own naming.
    ("Vocal Centre",      (LEAD_X, LEAD_Y, HEAD_Z)),
    ("Vocal Stage Left",  (OUTER_X, VOCAL_Y, HEAD_Z)),
    ("Vocal Stage Right", (-OUTER_X, VOCAL_Y, HEAD_Z)),
    ("Upstage Centre",    (0.0, BAND_Y, HEAD_Z)),
    ("Upstage Left",      (OUTER_X, BAND_Y, HEAD_Z)),
    ("Upstage Right",     (-OUTER_X, BAND_Y, HEAD_Z)),
    # The same five places again, named for who is standing there. Both
    # sets are wanted: the grid is where a *position* is, and survives the
    # band changing; these are what an operator actually calls for when
    # busking ("hit the keys"), and move if the band re-blocks.
    ("Drums",             (DRUM_X, DRUM_Y, 1.20)),
    ("Guitar",            (GUITAR_X, GUITAR_Y, HEAD_Z)),
    ("Keys",              (KEYS_X, KEYS_Y, HEAD_Z)),
    ("Bass",              (BASS_X, BASS_Y, HEAD_Z)),
    ("Lead Vocal",        (LEAD_X, LEAD_Y, HEAD_Z)),
    # The room.
    ("Stage Wide",        (0.0, 0.0, 1.20)),
    ("Back Wall",         (0.0, UPSTAGE_Y, 2.00)),
    ("Audience Front",    (0.0, LIP_Y - 2.00, 1.60)),
    ("Audience Back",     (0.0, HOUSE_BACK_Y + 1.50, 1.60)),
]
palettes = {
    "colors": [],
    "focus": [{"name": n, "target": v3(*t)} for n, t in focus],
}

# =====================================================================
# Profile binding — which of this room's groups plays each portable role.
# =====================================================================
profile = {
    "profile": "Ignition",
    "groups": {
        "Key": {"Group": "FOH Pars"},
        "Wash": {"Group": "Beam Pars"},
        "Back": {"Group": "Back Wall Pars"},
        "Movers": {"Group": "Movers"},
        "Bars": {"Group": "Strips All"},
    },
    "focus": {
        "Vocal": "Vocal Centre",
        "Stage": "Stage Wide",
        "Audience": "Audience Front",
        "Band": "Upstage Centre",
        "Drums": "Drums",
        "Back Wall": "Back Wall",
        "House": "Audience Back",
    },
    "canvases": {},
    "colors": [],
}

# Riverside's blocking grid. Venue-owned, not a profile binding — the
# number of areas is a property of the stage.
areas = {
    "about": ("Riverside's blocking grid — where people actually stand on a "
              "19' 8\" x 12' deck. Six areas: a downstage line for the front "
              "of the band and an upstage line for the back line and the kit. "
              "Generic shows reach the talent through the profile's focus "
              "roles; these are for this room's own programming and for "
              "busking."),
    "areas": {
        "Downstage Left": "Vocal Stage Left",
        "Downstage Centre": "Vocal Centre",
        "Downstage Right": "Vocal Stage Right",
        "Upstage Left": "Upstage Left",
        "Upstage Centre": "Upstage Centre",
        "Upstage Right": "Upstage Right",
    },
}

# =====================================================================
# Props — set dressing. `spawn.rs` gives "Person"/"Drum Kit"/"Mic"/
# "Pillar" real shapes; everything else renders as a labelled box, which
# is honest about how much is known about it.
# =====================================================================
props = []


def prop(name, pos, size, eulers=(0, 0, 0)):
    props.append({"name": name, "position": v3(*pos),
                  "eulers": v3(*eulers), "size": v3(*size)})


PERSON = (0.678, 0.302, 1.83)
BOOM_MIC = (0.5933, 1.2137, 1.634)     # size.y past 0.55 is the boom reach
STRAIGHT_MIC = (0.5933, 0.52, 1.6007)

# Back line, house left to house right: drums, guitar, keys.
prop("Drum Kit", (DRUM_X, DRUM_Y, 0.0), (KIT_W, KIT_D, KIT_H))
prop("Person - Drummer", (DRUM_X + 0.18, DRUM_Y + 0.45, 0.0), (0.68, 0.30, 1.80))
prop("Person - Guitar", (GUITAR_X, GUITAR_Y, 0.0), PERSON)
prop("Person - Keys", (KEYS_X, KEYS_Y, 0.0), PERSON)
# size.x is the keyboard's real length — what scales the model, since a
# keyboard is defined by its length and not its height (see `props::Fit`).
# It stands on the deck; `props.rs` draws the X-stand and lifts it to
# playing height.
prop("Keyboard", (KEYS_X, KEYS_Y - 0.44, 0.0), (1.30, 0.40, 0.26))

# Front line: bass at house left, lead vocal centre.
prop("Person - Bass", (BASS_X, BASS_Y, 0.0), PERSON)
prop("Person - Vocal", (LEAD_X, LEAD_Y, 0.0), PERSON)

prop("Mic - Lead Vocal", (LEAD_X, LEAD_Y - 0.35, 0.0), STRAIGHT_MIC)
prop("Mic - Bass", (BASS_X, BASS_Y - 0.45, 0.0), BOOM_MIC)
prop("Mic - Guitar", (GUITAR_X, GUITAR_Y - 0.45, 0.0), BOOM_MIC)
# Beside the keyboard, not in it — the boom reaches back over the keys.
prop("Mic - Keys", (KEYS_X + 0.62, KEYS_Y - 0.05, 0.0), BOOM_MIC)

# The mains are not on the stage at all: they stand on the room floor in
# front of the lip, on tripods, with the cabinet flown near head height.
# Their record therefore sits at FLOOR_Z, and `props::Stand::Tripod`
# lifts the cabinet — so `size.z` is the cabinet's own height, not how
# high off the ground it ends up.
#
# Being off the deck, they can also sit wider than the stage is: the room
# is 21' across where the stage is 19' 8".
PA_X = ROOM_W / 2.0 - 0.55
PA_Y = LIP_Y - 0.60
prop("PA Speaker Stage Left", (PA_X, PA_Y, FLOOR_Z), (0.45, 0.50, 0.95))
prop("PA Speaker Stage Right", (-PA_X, PA_Y, FLOOR_Z), (0.45, 0.50, 0.95))

# Wedges along the front of the deck, one per front-line position plus the
# two back-line players who need one. Angled up at the band, which is what
# `eulers.z = 180` means here: the model faces upstage.
# One per player, directly downstage of them — the front line's sit on the
# deck edge, the back line's just in front of where they stand.
for _n, _x, _y in (("Vocal", LEAD_X, LIP_Y + 0.30),
                   ("Bass", BASS_X, LIP_Y + 0.30),
                   ("Guitar", GUITAR_X, GUITAR_Y - 0.85),
                   ("Keys", KEYS_X, KEYS_Y - 0.95)):
    prop(f"Monitor - {_n}", (_x, _y, 0.0), (0.62, 0.38, 0.33), eulers=(0, 0, 180))
prop("Disco Ball", (0.0, HOUSE_MID_Y, BEAM_Z - 0.70), (0.40, 0.40, 0.40))
prop("Arch Neon", (0.0, HOUSE_BACK_Y + 0.06, ARCH_H / 2.0), (ARCH_W, 0.04, ARCH_H))

# =====================================================================
out = pathlib.Path(__file__).resolve().parent.parent / "data" / "venues" / "riverside"
out.mkdir(parents=True, exist_ok=True)

# Patch. A real `patch.json` is never overwritten — it is the one file
# here that does not come from this script.
patch_path = out / "patch.json"
if patch_path.exists() and not any(
        e.get("provisional") for e in json.loads(patch_path.read_text())):
    patch = json.loads(patch_path.read_text())
    origin = "patch.json"
else:
    patch = provisional_patch(fixtures)
    patch_path.write_text(json.dumps(patch, indent=2) + "\n")
    origin = "patch.json (provisional, newly written)"
matched = apply_patch(fixtures, patch)


def write(name, value):
    (out / name).write_text(json.dumps(value, indent=2) + "\n")
    n = len(value["areas"]) if name == "areas.json" else len(value)
    print(f"{name}: {n} entries")


write("fixtures.json", fixtures)
write("room.json", room)
write("groups.json", groups)
write("palettes.json", palettes)
write("profile.json", profile)
write("areas.json", areas)
write("props.json", props)
write("screens.json", [])
print(f"\nstage {r(STAGE_W)} x {r(STAGE_D)} m, deck {r(STAGE_H)} m up, "
      f"house {r(HOUSE_D)} m deep, beam {r(BEAM_Z)} m above deck")
print(f"chans: back wall 1-23, beam {BEAM_PAR_CH[0]}-{BEAM_STRIP_CH[-1]}, "
      f"FOH {FOH_CH[0]}-{FOH_CH[-1]}  ({len(fixtures)} fixtures)")
print(f"patch: {matched}/{len(fixtures)} addressed from {origin}"
      + ("  <-- PROVISIONAL" if any(e.get("provisional") for e in patch) else ""))
