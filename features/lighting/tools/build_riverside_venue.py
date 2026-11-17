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
BEAM_BOTTOM = ft(19, 6)   # 19' 6"  room floor up to the BOTTOM of the beam
BEAM_DEPTH = ft(6, 0)     # 6' 0"   the beam's own depth: it is a downstand
                          #         from the ceiling, not a truss, so the
                          #         ceiling sits at its top
BEAM_FROM_BACK = ft(10, 0)  # 10' 0" beam to the AUDIENCE back wall, which
                            #        puts it slightly behind the middle of
                            #        the room. (An earlier "6 inches from
                            #        the back wall" was a mis-measure.)

POLE_PITCH = ft(0, 64)  # 64"     light pole to light pole, centre to centre
PAR_FIRST = ft(0, 40)   # 40"     lowest par, above the deck
PAR_PITCH = ft(2, 0)    # 2' 0"   par to par up a pole
AUDIENCE_WALL_H = ft(10, 8)   # 10' 8"  height of the AUDIENCE (back-of-house)
                              #         wall, and of the lights on it.
                              #         NOT the stage bars — that was this
                              #         model's earlier reading of the same
                              #         number, and it was wrong.

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
BEAM_PAR_COUNT = 8      # the white "fill lights" in a row along the beam
BEAM_BAR_COUNT = 4      # bars on the beam, two either side of the ball
FOH_SIDE_COUNT = 4      # pars on the bar, each side of the audience wall
                        # (plus one centred above each bar: 5 a side, 10)
# (FOH_Z / FOH_HIGH_Z are derived below, once FLOOR_Z exists.)
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
STAGE_BAR_H = 3.30      # how high the three stage back-wall bars hang.
                        # Unmeasured: 10' 8" turned out to be the audience
                        # wall, so these lost the number they were using.
STRIP_LEN = 1.0         # the beam's amber strips
WALL_INSET = 0.15       # how far a wall-mounted fixture stands off its wall

# Derived
HALF_W = STAGE_W / 2.0
UPSTAGE_Y = STAGE_D / 2.0
LIP_Y = -STAGE_D / 2.0
FLOOR_Z = -STAGE_H
# Everything vertical is stated above the room floor and re-expressed
# above the deck, which is where this venue's origin sits.
BEAM_BOTTOM_Z = BEAM_BOTTOM + FLOOR_Z     # underside, above the deck
BEAM_TOP_Z = BEAM_BOTTOM_Z + BEAM_DEPTH   # and its top, which is the ceiling
BEAM_Z = BEAM_BOTTOM_Z + BEAM_DEPTH / 2.0  # centre, for the room record
CEILING_Z = BEAM_TOP_Z
ROOM_H = CEILING_Z - FLOOR_Z               # floor to ceiling
HOUSE_BACK_Y = LIP_Y - HOUSE_D
HOUSE_MID_Y = (LIP_Y + HOUSE_BACK_Y) / 2.0
BEAM_Y = HOUSE_BACK_Y + BEAM_FROM_BACK
# The audience wall's lights hang at the wall's own height, which is
# measured from the room floor.
FOH_Z = AUDIENCE_WALL_H + FLOOR_Z
FOH_HIGH_Z = FOH_Z + 0.35               # the single centre par, on top
POLE_Y = UPSTAGE_Y - POLE_Y_INSET

# The mains stand on the room floor in front of the lip, on poles that
# also carry the sizzle bars — so this is room geometry, not dressing.
PA_X = ROOM_W / 2.0 - 0.55
PA_Y = LIP_Y - 0.60
PA_CAB_H = 0.95                       # the cabinet itself
PA_TOP = ft(9, 0)                     # measured: top edge, above the DECK
# The pole is measured from the room floor the tripod stands on, so it has
# to make up the deck height as well as reach the cabinet's underside.
PA_POLE_H = PA_TOP + STAGE_H - PA_CAB_H
FIXTURE_Y = POLE_Y - FIXTURE_Y_INSET

# Four poles, centred on the stage, 64" apart: a 16' overall span.
POLE_X = [r(-1.5 * POLE_PITCH + i * POLE_PITCH) for i in range(4)]

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

# Named outright by the console, so no inference needed.
NAMED = {
    51: (362, 8, "ENDYSHOW LED Stage Light Bar PL-32M"),  # "Endy Show Pole L"
    52: (370, 8, "ENDYSHOW LED Stage Light Bar PL-32M"),  # "Endyshow Pole R"
    # Stage-ceiling beam, in POSX order (house left to right).
    91: (8, 7, "Solena Professional Max Par 54 RGB"),    # side par
    92: (115, 8, "LED MINI BEAM WH"),                    # pinspot
    93: (131, 9, "MINI DERBY"),
    94: (15, 7, "Solena Professional Max Par 54 RGB"),   # sign par
    95: (22, 7, "Solena Professional Max Par 54 RGB"),   # sign par
    96: (140, 9, "MINI DERBY"),
    97: (123, 8, "LED MINI BEAM WH"),                    # pinspot
    98: (29, 7, "Solena Professional Max Par 54 RGB"),   # side par
    # The four ZKYMZL heads keep their own addresses when they come off
    # the pole tops and go on the floor — it is the incoming U'King 150s
    # that get new ones, so 400-433 travels with the old fixtures. Which
    # unit lands where on the floor is arbitrary; this is address order.
    # The beam's row of 8 fill pars: the "PARS" members the desk draws in
    # one evenly-spaced line (POSY 200, POSX 380..660), in POSX order.
    31: (43, 7, "Solena Professional Max Par 54 RGB"),
    32: (1, 7, "Solena Professional Max Par 54 RGB"),
    33: (50, 7, "Solena Professional Max Par 54 RGB"),
    34: (57, 7, "Solena Professional Max Par 54 RGB"),
    35: (64, 7, "Solena Professional Max Par 54 RGB"),
    36: (71, 7, "Solena Professional Max Par 54 RGB"),
    37: (78, 7, "Solena Professional Max Par 54 RGB"),
    38: (36, 7, "Solena Professional Max Par 54 RGB"),

    # The beam's 4 bars. The desk splits these across two groups by what
    # they are *used* for rather than where they hang — "CURTAIN BARS"
    # (Solena Max Bar) and "FLOOR BARS" (RockStrip) — which is exactly the
    # "house lights floor lights etc" double duty. In POSX order they read
    # 107, 390, 400, 666: two either side of the ball at the centre.
    81: (100, 6, "Solena Max Bar 28 RGB"),
    82: (92, 7, "RockStrip 252"),
    83: (216, 7, "RockStrip 252"),
    84: (106, 6, "Solena Max Bar 28 RGB"),

    # The pinspot on the ball.
    85: (85, 6, "RGBW Spot Light 6CH"),

    # Audience wall. "REAR PARS" is two runs of four (desk POSX 260-380
    # and 590-710); the two Solenas at POSX 295 and 660 land centred over
    # each run, which is what puts them on top rather than on the bar.
    41: (149, 7, "36 LED Par Can"),          # house-left bar, left to right
    42: (156, 7, "36 LED Par Can"),
    43: (163, 7, "36 LED Par Can"),
    44: (170, 7, "36 LED Par Can"),
    45: (209, 7, "Solena Professional Max Par 54 RGB"),   # centred above it
    46: (198, 7, "36 LED Par Can"),          # house-right bar, left to right
    47: (177, 7, "36 LED Par Can"),
    48: (184, 7, "36 LED Par Can"),
    49: (191, 7, "36 LED Par Can"),
    50: (225, 7, "Solena Professional Max Par 54 RGB"),   # centred above it

    71: (400, 11, "Mini Gobo Moving Head Light"),
    72: (411, 11, "Mini Gobo Moving Head Light"),
    73: (422, 11, "Mini Gobo Moving Head Light"),
    74: (433, 11, "Mini Gobo Moving Head Light"),
}

CONSOLE_FIXTURES = dict(NAMED)
for pole in range(1, 5):
    for row in range(1, 5):
        CONSOLE_FIXTURES[(pole - 1) * 4 + row] = (
            WALL_ADDR[(pole, row)], 7, "Solena Professional Max Par 54 RGB")
    # The pole tops are being re-fitted with U'King 150W beams. Their
    # addresses are not assigned yet, so they stay placeholders — the old
    # 400-433 belongs to the ZKYMZLs, which are moving to the floor.
    pass

# Everything else this venue models still has no position->fixture link,
# so it keeps a placeholder type and a placeholder address.
# Footprints for every type the desk actually carries, read from the same
# extract the addresses come from. A modelled fixture whose position is
# still unknown can therefore reserve the right number of channels even
# before it has an address.
def _console_footprints():
    path = (pathlib.Path(__file__).resolve().parent.parent
            / "data" / "venues" / "riverside" / "console-show.json")
    try:
        show = json.loads(path.read_text())
    except OSError:
        return {}
    return {p["profile"]: p["footprint"] for p in show.get("profiles", [])}


CONSOLE_FOOTPRINTS = _console_footprints()


def _console_occupied():
    """Every DMX channel the desk's own patch uses."""
    path = (pathlib.Path(__file__).resolve().parent.parent
            / "data" / "venues" / "riverside" / "console-show.json")
    try:
        show = json.loads(path.read_text())
    except OSError:
        return []
    return [range(f["address"], f["address"] + f["footprint"])
            for f in show.get("fixtures", [])]


CONSOLE_OCCUPIED = _console_occupied()

PROVISIONAL_TYPES = {
    "Par": ("Uking", "Par", 7),
    "Moving Head": ("Betopper", "Beam Moving Head", 12),
    "Bar": ("Rockville", "Rockstrip 252 7ch", 7),
    # Not in the showfile yet — the U'King 150s have not been patched. 16
    # channels is this class of beam mover's usual footprint; it is a
    # placeholder either way, and only reserves space.
    "U'King 150W Moving Head Beam": ("U'King", "150W Moving Head Beam", 16),
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
    # Addresses are only unique within a universe, so everything below
    # tracks (universe, address) pairs. The console is a single universe;
    # placeholders spill into the next one once it is full.
    real, taken = {}, set()
    for entry in CONSOLE_OCCUPIED:
        # Every channel the desk already uses, including the fixtures this
        # model has not placed yet. A placeholder landing on one of those
        # would silently drive a real fixture in the room.
        taken.update((1, a) for a in entry)
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
        taken.update((1, a) for a in range(addr, addr + footprint))

    def allocate(footprint):
        """First free run of `footprint` channels, in the lowest universe
        that has room."""
        universe = 1
        while True:
            for addr in range(1, 512 - footprint + 2):
                if all((universe, a) not in taken
                       for a in range(addr, addr + footprint)):
                    taken.update((universe, a)
                                 for a in range(addr, addr + footprint))
                    return universe, addr
            universe += 1

    patch = []
    for f in fixtures:
        if f["chan"] in real:
            patch.append(real[f["chan"]])
            continue
        if f["model"] in CONSOLE_FOOTPRINTS:
            # A real fixture type, so its footprint is known even though
            # its address is not — the placeholder reserves the right
            # number of channels.
            maker, model, footprint = "", f["model"], CONSOLE_FOOTPRINTS[f["model"]]
        else:
            maker, model, footprint = PROVISIONAL_TYPES[f["model"]]
        universe, address = allocate(footprint)
        patch.append({
            "chan": f["chan"],
            "manufacturer": maker,
            "model": model,
            "patched": True,
            "universe": universe,
            "address": address,
            "footprint": footprint,
            "provisional": True,
            "notes": ["No position->fixture link yet. See README."],
        })
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
     "size": v3(ROOM_W, 0.0, ROOM_H)},
    {"name": "Wall - House Back",
     "position": v3(0, HOUSE_BACK_Y, FLOOR_Z), "eulers": v3(0, 0, -180),
     "size": v3(ROOM_W, 0.0, ROOM_H)},
    {"name": "Wall - Stage Left",
     "position": v3(ROOM_W / 2.0, (UPSTAGE_Y + HOUSE_BACK_Y) / 2.0, FLOOR_Z),
     "eulers": v3(0, 0, 0),
     "size": v3(0.0, UPSTAGE_Y - HOUSE_BACK_Y, ROOM_H)},
    {"name": "Wall - Stage Right",
     "position": v3(-ROOM_W / 2.0, (UPSTAGE_Y + HOUSE_BACK_Y) / 2.0, FLOOR_Z),
     "eulers": v3(0, 0, -180),
     "size": v3(0.0, UPSTAGE_Y - HOUSE_BACK_Y, ROOM_H)},
    # A 6'-deep downstand hanging across the room, not a truss.
    {"name": "Beam - House",
     "position": v3(0, BEAM_Y, BEAM_Z), "eulers": v3(0, 0, 0),
     "size": v3(ROOM_W, 0.4064, BEAM_DEPTH)},
    # The second beam: same section and height as the house one, at the
    # stage back wall.
    {"name": "Beam - Stage",
     "position": v3(0, UPSTAGE_Y - 0.30, BEAM_Z), "eulers": v3(0, 0, 0),
     "size": v3(ROOM_W, 0.4064, BEAM_DEPTH)},
    {"name": "Ceiling",
     "position": v3(0, (UPSTAGE_Y + HOUSE_BACK_Y) / 2.0, CEILING_Z),
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
        # U'King ZQ02341US, 150W. 2.8 degrees is the manufacturer's own
        # figure — a true beam, an order tighter than the gobo heads it
        # replaces, which is the whole point of the swap.
        "U'King 150W Moving Head Beam", (x, POLE_Y, MOVER_Z), None,
        MOVER_SIZE, 2.8,
        hang=MOVER_HANG))

# Three bars across the top of the back wall, aimed straight downstage.
for i, x in enumerate(spread(3, STAGE_W * 2.0 / 3.0), start=1):
    pos = (x, UPSTAGE_Y - 0.10, STAGE_BAR_H + BACKWALL_LIFT)
    fixtures.append(fixture(
        20 + i, f"Back Wall Bar {i}",
        ["Luminaire_LED_Bar", "Luminaire_LED_Wash", "Back Wall", "Bars"],
        "Bar", pos, (x, LIP_Y, STAGE_BAR_H + BACKWALL_LIFT - 0.4),
        (BAR_LEN, 0.09, 0.09), 40.0))

# --- The house beam, chans 31-38 and 81-85 --------------------------
# One beam only: a 6'-deep downstand hanging across the room, slightly
# behind its middle. It carries a row of 8 pars pointing across at the
# stage — the white "fill lights" — plus 4 bars split two either side of
# the disco ball hanging at its centre, and a pinspot aimed at the ball.
BEAM_RIG_Z = BEAM_BOTTOM_Z - 0.20       # hung just under the beam
for i, x in enumerate(spread(BEAM_PAR_COUNT, ROOM_W * 0.86), start=1):
    pos = (x, BEAM_Y, BEAM_RIG_Z)
    fixtures.append(fixture(
        30 + i, f"Beam Par {i}",
        ["Luminaire_LED_Wash", "Beam", "Fill"],
        "Solena Professional Max Par 54 RGB",
        # Fanned across the stage rather than converging: these are fill,
        # so the row should cover the width evenly.
        pos, (x * (STAGE_W / (ROOM_W * 0.86)), 0.0, 1.5), PAR_SIZE, 30.0))

# Two bars each side of the ball. `spread` over the full span would put
# one at the centre, where the ball is, so the two sides are placed
# separately.
BEAM_BAR_X = ([r(-x) for x in spread(BEAM_BAR_COUNT // 2, 1.30, centre=1.85)]
              + spread(BEAM_BAR_COUNT // 2, 1.30, centre=1.85))
for i, x in enumerate(sorted(BEAM_BAR_X), start=1):
    pos = (x, BEAM_Y + 0.30, BEAM_RIG_Z - 0.15)
    fixtures.append(fixture(
        80 + i, f"Beam Bar {i}",
        ["Luminaire_LED_Bar", "Luminaire_LED_Wash", "Beam", "Bars", "House"],
        "Solena Max Bar 28 RGB", pos, (x, BEAM_Y + 0.30, 0.0),
        (STRIP_LEN, 0.08, 0.08), 40.0))

# The ball hangs at the beam's centre; the pinspot lives beside it and
# points at it. The Czgor colour pinspots are a 2-pack but the desk
# carries one RGBW Spot, so one is modelled.
DISCO = (0.0, BEAM_Y, BEAM_RIG_Z - 0.55)
fixtures.append(fixture(
    85, "Ball Pinspot",
    ["Luminaire_LED_Wash", "Beam", "Disco"],
    "RGBW Spot Light 6CH",
    (0.9, BEAM_Y + 0.15, BEAM_RIG_Z - 0.10), DISCO, (0.12, 0.12, 0.18), 8.0))

# --- The PA poles, chans 51-52 --------------------------------------
# "2 bar lights on the piles that lift up the studio speakers" — the
# sizzle lights. The console names these itself: "Endy Show Pole L" and
# "Endyshow Pole R", which is what pins them to this position rather than
# to any of the other bars in the rig.
#
# Left aims at the kit, right at the keys player — house left and house
# right, and the band happens to sit that way round, so the two agree.
SIZZLE_Z = FLOOR_Z + 1.60
# Named for what each one lights rather than for a side. The console calls
# these "Endy Show Pole L" / "Endyshow Pole R" in HOUSE left/right, while
# this model's props say "Stage Left" — opposite sides of the same room.
# "Sizzle Drums" cannot be read two ways.
for i, (side, target) in enumerate((
    ("Drums", (DRUM_X, DRUM_Y, 1.10)),
    ("Keys", (KEYS_X, KEYS_Y, HEAD_Z)),
)):
    x = -PA_X if side == "Drums" else PA_X
    fixtures.append(fixture(
        51 + i, f"Sizzle {side}",
        ["Luminaire_LED_Bar", "Luminaire_LED_Wash", "PA Pole", "Sizzle"],
        # ENDYSHOW PL-32M, 50W RGBW, 35 degrees.
        "ENDYSHOW LED Stage Light Bar PL-32M",
        (x, PA_Y, SIZZLE_Z), target, (0.90, 0.08, 0.08), 35.0))

# --- Stage-ceiling beam, chans 91-98 --------------------------------
# A second beam, same section and height as the house one, at the stage
# back wall. Its row reads outside-in on each side and mirrors about the
# centre — and the desk's own layout agrees, fixture for fixture, in POSX
# order: @8 side par, @115 pinspot, @131 derby, @15/@22 the two sign
# pars, then @140, @123, @29 back out again.
#
# The outermost pair sit above the midpoints of back-wall bars 1 and 3,
# which is what fixes the row's overall span.
BEAM2_Y = UPSTAGE_Y - 0.30
BEAM2_Z = BEAM_BOTTOM_Z - 0.20
BEAM2_OUTER = STAGE_W / 3.0          # over the middle of bars 1 and 3
# Four positions a side. The innermost pair are the two sign pars, which
# sit either side of centre rather than on it — the sign is one object and
# they light it from both sides.
BEAM2_INNER = 0.25
BEAM2_PITCH = (BEAM2_OUTER - BEAM2_INNER) / 3.0
SIGN = (0.0, UPSTAGE_Y, 2.60)        # the Rockstars sign on the back wall

BEAM2 = []
for side in (-1, 1):
    for slot, kind in enumerate(("Side Par", "Pinspot", "Derby", "Sign Par")):
        BEAM2.append((side * (BEAM2_OUTER - slot * BEAM2_PITCH), side, kind))
BEAM2.sort(key=lambda e: e[0])
for i, (x, side, kind) in enumerate(BEAM2, start=1):
    hand = "Left" if side < 0 else "Right"
    if kind == "Side Par":
        # Aimed across at the side wall it is nearest.
        target, model, tags = ((side * ROOM_W / 2.0, BEAM2_Y, 1.80),
                               "Solena Professional Max Par 54 RGB", ["Sides"])
    elif kind == "Pinspot":
        # Motorised, and pointed at the ball on the house beam.
        target, model, tags = (DISCO, "LED MINI BEAM WH",
                               ["Luminaire_LED_Yoke_Spot", "Pinspots", "Disco"])
    elif kind == "Derby":
        target, model, tags = ((x, LIP_Y, 0.0), "MINI DERBY", ["Derbys"])
    else:
        target, model, tags = (SIGN, "Solena Professional Max Par 54 RGB",
                               ["Sign"])
    fixtures.append(fixture(
        90 + i, f"{kind} {hand}" if kind != "Sign Par" else f"Sign Par {hand}",
        ["Luminaire_LED_Wash", "Stage Beam"] + tags,
        model, (x, BEAM2_Y, BEAM2_Z), target,
        (0.22, 0.22, 0.24) if kind in ("Derby", "Pinspot") else PAR_SIZE,
        14.0 if kind == "Pinspot" else 30.0))

# --- Floor movers, chans 71-74 --------------------------------------
# The four ZKYMZL 30W gobo heads coming off the pole tops when the U'King
# 150W beams replace them. A 12'-deep deck with this band on it has very
# little clear floor: mapping it leaves a 34 cm strip against the back
# wall between the poles, and the two downstage outer corners.
#
# Split rather than clustered — two upstage shooting up the back wall and
# through the band, two downstage in the corners for audience beams and
# front cross-light. Four in the back strip would all be hidden behind the
# band and unreachable behind the kit.
FLOOR_MOVERS = [
    (-1.63, POLE_Y + 0.02, "Upstage Left"),
    (1.63, POLE_Y + 0.02, "Upstage Right"),
    (-2.70, LIP_Y + 0.38, "Downstage Left"),
    (2.70, LIP_Y + 0.38, "Downstage Right"),
]
for i, (x, y, where) in enumerate(FLOOR_MOVERS):
    fixtures.append(fixture(
        71 + i, f"Floor Mover {where}",
        ["Luminaire_LED_Yoke_Spot", "Movers All", "Floor Movers"],
        # ZKYMZL TY-30 (mfr part RYON-30), 30W, 15 degrees.
        "Mini Gobo Moving Head Light", (x, y, 0.17), None,
        MOVER_SIZE, 15.0, hang=MOVER_HANG))

# --- Front of house (back-of-room wall), chans 41-49 -----------------
# Two brackets flanking the arch, plus the one par mounted above the
# house-right bracket. These are the front light on faces.
FOH_Y = HOUSE_BACK_Y + WALL_INSET
# Ten pars, five a side, mirrored: four on a horizontal bar with a fifth
# centred above it. Numbered house left to house right, bar first then the
# one on top, so each side is a contiguous block of five.
FOH_GROUP_X = 1.95          # centre of each bar
FOH_PITCH = 0.55            # par to par along a bar
foh = []
for centre in (-FOH_GROUP_X, FOH_GROUP_X):
    for x in spread(FOH_SIDE_COUNT, FOH_PITCH * (FOH_SIDE_COUNT - 1), centre=centre):
        foh.append((x, FOH_Z, "bar"))
    foh.append((centre, FOH_HIGH_Z, "top"))
for i, (x, z, kind) in enumerate(foh, start=1):
    pos = (x, FOH_Y, z)
    # "Colour washes spread evenly across the stage" — each one takes its
    # own share of the width rather than all converging on centre.
    fixtures.append(fixture(
        40 + i, f"FOH Par {i}",
        ["Luminaire_LED_Wash", "FOH", "Front Wash", "Key",
         "FOH Top" if kind == "top" else "FOH Bar"],
        # The bars are the U'King 36-LED cans the desk groups as "REAR
        # PARS"; the two on top are Solenas.
        "36 LED Par Can" if kind == "bar" else "Solena Professional Max Par 54 RGB",
        pos, (x * 0.55, LIP_Y + 0.8, 1.55), PAR_SIZE, 30.0))

# =====================================================================
# Groups — the selections this rig is actually programmed against.
# =====================================================================
groups = []


def group(label, channels):
    groups.append({"target": str(len(groups) + 1), "label": label,
                   "channels": channels})


BEAM_PAR_CH = [30 + i for i in range(1, BEAM_PAR_COUNT + 1)]
BEAM_BAR_CH = [80 + i for i in range(1, BEAM_BAR_COUNT + 1)]
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
group("Beam Bars", rng(BEAM_BAR_CH))
group("Beam All", rng(BEAM_PAR_CH) + rng(BEAM_BAR_CH))
group("FOH Pars", rng(FOH_CH))
group("FOH Bars", ["41-44", "46-49"])
group("FOH Tops", [45, 50])
# The two layers the profile's Key/Wash roles bind to. Front Wash is
# everything that lights the band from in front of them; Strips All is
# every linear fixture in the room regardless of position, which is what
# a charted hit wants.
group("Front Wash", rng(FOH_CH) + rng(BEAM_PAR_CH))
group("Strips All", ["21-23"] + rng(BEAM_BAR_CH) + ["51-52"])
group("Pars All", ["1-16"] + rng(BEAM_PAR_CH) + rng(FOH_CH))
group("Sizzle", ["51-52"])
group("Stage Beam", ["91-98"])
group("Side Pars", [91, 98])
group("Pinspots", [92, 97])
group("Derbys", [93, 96])
group("Sign Pars", [94, 95])
group("Floor Movers", ["71-74"])
group("Movers All", ["17-20", "71-74"])

# =====================================================================
# Palettes — the room's focus points. Colours are deliberately NOT
# defined here: `venue::inherit_colors` fills them from the profile, so
# Riverside means the same thing by "Deep Blue" as every other room
# until it has a reason not to.
# =====================================================================
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
prop("PA Speaker Stage Left", (PA_X, PA_Y, FLOOR_Z), (0.45, 0.50, PA_CAB_H))
prop("PA Speaker Stage Right", (-PA_X, PA_Y, FLOOR_Z), (0.45, 0.50, PA_CAB_H))

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
# Hung under the beam's underside, not inside it — the beam is 6' deep.
prop("Disco Ball", (DISCO[0], DISCO[1], DISCO[2]), (0.40, 0.40, 0.40))
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
print(f"chans: back wall 1-23, beam pars {BEAM_PAR_CH[0]}-{BEAM_PAR_CH[-1]}, "
      f"beam bars {BEAM_BAR_CH[0]}-{BEAM_BAR_CH[-1]}, ball 85, "
      f"FOH {FOH_CH[0]}-{FOH_CH[-1]}, sizzle 51-52, derbys 61-62, "
      f"floor movers 71-74  ({len(fixtures)} fixtures)")
print(f"patch: {matched}/{len(fixtures)} addressed from {origin}"
      + ("  <-- PROVISIONAL" if any(e.get("provisional") for e in patch) else ""))
