#!/usr/bin/env python3
"""Generate the CBU Room 138 venue.

A venue authored from a description rather than extracted from a console
show, which is what makes it worth having as a script: every number below
is derived from the room's stated dimensions, so a correction to one of
them ("the towers are at nine feet, not six") is one edit and a re-run
rather than thirty-six hand-edited quaternions.

    python3 tools/make_room138.py

Coordinates match every other venue: metres, x = stage right(-)/left(+),
y = downstage(-)/upstage(+), z = up, with the band area centred on the
origin. See docs/domain/norco-venue-reference.md.
"""
import json
import math
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "data" / "venues" / "room138-cbu"
FT = 0.3048

# ── The room, as given: 48 ft along the wall the band plays against,
#    35 ft deep. Ceiling height was not stated; 16 ft is a plain
#    classroom/black-box height and is flagged as an assumption in the
#    README.
WIDTH, DEPTH, HEIGHT = 48 * FT, 35 * FT, 16 * FT
BAND_DEEP = 12 * FT                    # the playing area, centred on y=0
Y_BAND_WALL = BAND_DEEP / 2            # +y: the 48 ft wall, drums against it
Y_HOUSE_BACK = Y_BAND_WALL - DEPTH     # -y: the back of the room
X_HALF = WIDTH / 2


def quat_aim(dx, dy, dz):
    """The mount quaternion that points a fixture's beam along (dx,dy,dz).

    The renderer aims along `mount * pan * tilt * -Z`, so this is the
    shortest arc from -Z to the given direction — see
    `FixtureRecord::placement`.
    """
    n = math.sqrt(dx * dx + dy * dy + dz * dz)
    dx, dy, dz = dx / n, dy / n, dz / n
    ux, uy, uz = 0.0, 0.0, -1.0
    dot = ux * dx + uy * dy + uz * dz
    if dot > 0.999999:
        return {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0}
    if dot < -0.999999:                      # straight up: spin about X
        return {"w": 0.0, "x": 1.0, "y": 0.0, "z": 0.0}
    cx, cy, cz = uy * dz - uz * dy, uz * dx - ux * dz, ux * dy - uy * dx
    w = 1.0 + dot
    n = math.sqrt(cx * cx + cy * cy + cz * cz + w * w)
    return {"w": round(w / n, 6), "x": round(cx / n, 6),
            "y": round(cy / n, 6), "z": round(cz / n, 6)}


def v3(x, y, z):
    return {"x": round(x, 4), "y": round(y, 4), "z": round(z, 4)}


room = [
    {"name": "Floor", "position": v3(0, (Y_BAND_WALL + Y_HOUSE_BACK) / 2, 0),
     "eulers": v3(0, 0, 0), "size": v3(WIDTH, DEPTH, 0)},
    {"name": "Wall - Upstage", "position": v3(0, Y_BAND_WALL, 0),
     "eulers": v3(0, 0, 0), "size": v3(WIDTH, 0, HEIGHT)},
    {"name": "Wall - House Back", "position": v3(0, Y_HOUSE_BACK, 0),
     "eulers": v3(0, 0, -180), "size": v3(WIDTH, 0, HEIGHT)},
    {"name": "Wall - Stage Left", "position": v3(X_HALF, (Y_BAND_WALL + Y_HOUSE_BACK) / 2, 0),
     "eulers": v3(0, 0, 0), "size": v3(0, DEPTH, HEIGHT)},
    {"name": "Wall - Stage Right", "position": v3(-X_HALF, (Y_BAND_WALL + Y_HOUSE_BACK) / 2, 0),
     "eulers": v3(0, 0, -180), "size": v3(0, DEPTH, HEIGHT)},
    {"name": "Ceiling", "position": v3(0, (Y_BAND_WALL + Y_HOUSE_BACK) / 2, HEIGHT),
     "eulers": v3(0, 0, 0), "size": v3(WIDTH, DEPTH, 0)},
]

fixtures, patch = [], []
chan, addr = 1, 1


def add(name, model, tags, pos, aim, size, beam, footprint):
    global chan, addr
    fixtures.append({
        "chan": chan, "name": name, "tags": tags, "patched": True,
        "manufacturer": "", "model": model,
        "position": pos, "eulers": v3(0, 0, 0), "quat": quat_aim(*aim),
        "size": size, "beam_angle_deg": beam,
        "universe": 1, "address": addr,
    })
    patch.append({"chan": chan, "manufacturer": "", "model": model,
                  "patched": True, "universe": 1, "address": addr,
                  "footprint": footprint, "source": "make_room138.py"})
    chan += 1
    addr += footprint


# ── 4 towers, symmetric about centre, 6 lights each. Just upstage of the
#    band so they rim the players and wash the wall behind them.
TOWER_X = [-18 * FT, -6 * FT, 6 * FT, 18 * FT]
TOWER_Y = Y_BAND_WALL - 1.0 * FT
# A ~5 ft stand with the six pars stacked touching, as photographed: six
# 7-inch bodies is about 3.5 ft of pole, sitting under the 5 ft top.
TOWER_H = 5 * FT
TOWER_Z = [TOWER_H - i * 0.7 * FT for i in range(5, -1, -1)]
for t, x in enumerate(TOWER_X, start=1):
    for n, z in enumerate(TOWER_Z, start=1):
        room.append({
            "name": f"Column - Tower {t}", "position": v3(x, TOWER_Y, TOWER_H / 2),
            "eulers": v3(0, 0, 0), "size": v3(0.08, 0.08, TOWER_H),
        } if n == 1 else None)
        add(f"Tower {t} Light {n}", "CBU Tower Blinder Ring",
            ["Luminaire_LED_Wash", "Towers", f"Tower {t}", f"Tower Row {n}"],
            # Tilted well down rather than level: these sit behind the
            # band facing downstage, so level means firing the length of
            # the room and washing it flat. Down puts them on the players
            # and the floor around them, which is what backlight is for.
            v3(x, TOWER_Y, z), (0, -1, -0.55), v3(0.18, 0.18, 0.12), 40.0, 4)
room = [r for r in room if r is not None]

# ── 2 trees in the house, 4 parcans each, facing the stage.
# ── Where the band stands. Positions are AUDIENCE-relative: "left of
#    the drums" is the audience's left, which is stage right and -x here.
#    One constant, so the whole band mirrors if that reading is wrong.
AUDIENCE_LEFT = -1.0
DRUMS_Y = Y_BAND_WALL - 4 * FT
BACK_LINE = Y_BAND_WALL - 5 * FT
VOCAL = (0.0, Y_BAND_WALL - 9 * FT)      # out in front of the kit

BAND = [
    ("Person - Bass", AUDIENCE_LEFT * 6.5 * FT, BACK_LINE, 0),
    ("Person - Guitar 1", -AUDIENCE_LEFT * 5.5 * FT, BACK_LINE, 0),
    ("Person - Guitar 2", -AUDIENCE_LEFT * 11 * FT, BACK_LINE, 0),
    # The frontman: centre, downstage of the kit, and playing bass — so
    # he is on a mic and holding an instrument, not just standing. The
    # bass itself is not modelled: `assets/props/` has a drum kit, a
    # keyboard, a PA cabinet and a floor monitor, and no stringed
    # instrument, so this is a person and a mic stand until there is one.
    ("Person - Vocal Bass", VOCAL[0], VOCAL[1], 0),
]

# ── 4 floor bars.
#    Two at the band wall throwing up it. Two flanking the frontman,
#    downstage of him and angled in, so his face is lit from both sides
#    rather than flat from one — the reason there are two and not one.
for n, x in enumerate([-9 * FT, 9 * FT], start=1):
    add(f"Wall Bar {n}", "Solena Max Bar 28 RGB",
        ["Luminaire_LED_Bar", "Bars", "Wall Bars"],
        v3(x, Y_BAND_WALL - 0.4 * FT, 0.15), (0, 0.15, 1), v3(1.0, 0.09, 0.09), 30.0, 6)

# A T-bar stand about 8 ft tall with the four pars side by side along
# the crossbar, as photographed — not stacked up the pole.
TREE_H = 8 * FT
TREE_Y = -14 * FT
TREE_SPAN = [-1.5 * FT, -0.5 * FT, 0.5 * FT, 1.5 * FT]
# Left to right on the audience-left tree: keys, bass, drums, singer.
# Heads at 1.55, except the drummer, who is sitting.
TREE_AIM = [
    ("Keys", AUDIENCE_LEFT * 15.5 * FT, BACK_LINE - 3 * FT, 1.55),
    ("Bass", AUDIENCE_LEFT * 6.5 * FT, BACK_LINE, 1.55),
    ("Drums", 0.17, DRUMS_Y + 0.45, 1.15),
    ("Vocal", VOCAL[0], VOCAL[1], 1.55),
]
for t, x in enumerate([-16 * FT, 16 * FT], start=1):
    side = "Right" if x < 0 else "Left"
    room.append({"name": f"Column - Tree {t}", "position": v3(x, TREE_Y, TREE_H / 2),
                 "eulers": v3(0, 0, 0), "size": v3(0.09, 0.09, TREE_H)})
    room.append({"name": f"Column - Tree {t} Bar", "position": v3(x, TREE_Y, TREE_H),
                 "eulers": v3(0, 0, 0), "size": v3(4 * FT, 0.06, 0.06)})
    # Each par is aimed at one player, so the four of them cover the
    # band rather than washing it flat. The far tree takes them in the
    # opposite order, which gives every player light from both sides —
    # one key, one fill — instead of two pars from the same direction.
    order = TREE_AIM if t == 1 else list(reversed(TREE_AIM))
    for n, (dx, (who, tx, ty, tz)) in enumerate(zip(TREE_SPAN, order), start=1):
        add(f"Tree {t} Par {n} — {who}", "36 LED Par Can",
            ["Luminaire_LED_Wash", "Trees", f"Tree {t}", f"Tree {side}", who],
            v3(x + dx, TREE_Y, TREE_H),
            (tx - (x + dx), ty - TREE_Y, tz - TREE_H),
            v3(0.2, 0.2, 0.24), 25.0, 7)

# ── 4 movers on the band wall, 8 ft up — half the wall's height, above
#    the 5 ft towers so they clear the band's heads and can sweep the
#    whole room. Placed in the gaps between and outside the towers
#    (which sit at +/-6 and +/-18) so the two packages interleave rather
#    than stack: -21, -18T, -12, -6T, +6T, +12, +18T, +21.
#
#    Hung facing downstage with a little tilt down; pan and tilt are
#    live, so this is only where they point with the desk at zero.
MOVER_Z = HEIGHT / 2
for n, x in enumerate([-21 * FT, -12 * FT, 12 * FT, 21 * FT], start=1):
    add(f"Mover {n}", "150W Moving Head Beam",
        ["Luminaire_Beam", "Movers"],
        v3(x, Y_BAND_WALL - 0.5 * FT, MOVER_Z), (0, -1, -0.35),
        v3(0.22, 0.26, 0.36), 14.0, 12)

VOCAL_HEAD = 1.55
KEY_BAR_X = 5.5 * FT
KEY_BAR_Y = VOCAL[1] - 6 * FT
for n, x in enumerate([-KEY_BAR_X, KEY_BAR_X], start=1):
    add(f"Key Bar {n}", "Solena Max Bar 28 RGB",
        ["Luminaire_LED_Bar", "Bars", "Key Bars"],
        v3(x, KEY_BAR_Y, 0.15),
        (VOCAL[0] - x, VOCAL[1] - KEY_BAR_Y, VOCAL_HEAD - 0.15),
        v3(1.0, 0.09, 0.09), 30.0, 6)

def by_tag(tag):
    return [f["chan"] for f in fixtures if tag in f["tags"]]


# `target`/`label`/`channels`, the shape the other venues use — the
# target is the group's own number and the channels are strings so a
# run can be written "1-16".
_g = [("Towers", "Towers"), *[(f"Tower {t}", f"Tower {t}") for t in range(1, 5)],
      *[(f"Tower Row {n}", f"Tower Row {n}") for n in range(1, 7)],
      ("Trees", "Trees"), ("Tree Left", "Tree Left"), ("Tree Right", "Tree Right"),
      ("Bars", "Bars"), ("Wall Bars", "Wall Bars"), ("Key Bars", "Key Bars"),
      ("Movers", "Movers")]
groups = [{"target": str(i), "label": label, "channels": [str(c) for c in by_tag(tag)]}
          for i, (label, tag) in enumerate(_g, start=1)]
# The middle pair of towers is what is actually over the kit, so the
# drum special is those rather than a fixture of its own.
groups.append({"target": str(len(groups) + 1), "label": "Drum Towers",
               "channels": [str(c) for c in by_tag("Tower 2") + by_tag("Tower 3")]})
# Everything that lights a face from the front: the trees out in the
# house and the two bars crossing on the frontman. One group, because
# `Key` is one role and both of these are doing its job.
groups.append({"target": str(len(groups) + 1), "label": "Front Light",
               "channels": [str(c) for c in by_tag("Trees") + by_tag("Key Bars")]})

# The drum kit, centred on the 48 ft wall as described. Same shape the
# other venues give it — props carry a size, not a kind.
props = [
    {"name": "Drum Kit", "position": v3(0, DRUMS_Y, 0),
     "eulers": v3(0, 0, 0), "size": v3(1.926, 1.632, 1.206)},
    {"name": "Person - Drummer", "position": v3(0.17, DRUMS_Y + 0.45, 0),
     "eulers": v3(0, 0, 0), "size": v3(0.68, 0.3, 1.8)},
    *[{"name": name, "position": v3(x, y, 0), "eulers": v3(0, 0, yaw),
       "size": v3(0.678, 0.302, 1.83)} for name, x, y, yaw in BAND],
    # Further out past the bass player and turned in 45 degrees, so the
    # player faces across the stage rather than straight down the room.
    {"name": "Keyboard", "position": v3(AUDIENCE_LEFT * 14 * FT, BACK_LINE - 2 * FT, 0),
     "eulers": v3(0, 0, -45 * AUDIENCE_LEFT), "size": v3(1.35, 0.42, 0.95)},
    {"name": "Person - Keys", "position": v3(AUDIENCE_LEFT * 15.5 * FT, BACK_LINE - 3 * FT, 0),
     "eulers": v3(0, 0, -45 * AUDIENCE_LEFT), "size": v3(0.678, 0.302, 1.83)},
    {"name": "Mic 1", "position": v3(VOCAL[0], VOCAL[1] - 1.2 * FT, 0),
     "eulers": v3(0, 0, 0), "size": v3(0.59, 0.52, 1.6)},
]

cameras = {
    "presets": [
        {"name": "Wide", "eye": [0.0, Y_HOUSE_BACK + 4 * FT, 5.5 * FT],
         "look": [0.0, Y_BAND_WALL - 6 * FT, 4 * FT], "fov_deg": 65.0,
         "about": "The whole room from the back of the house."},
        {"name": "Singer", "eye": [0.0, -8 * FT, 5.2 * FT],
         "look": [0.0, Y_BAND_WALL - 6 * FT, 5 * FT], "fov_deg": 32.0,
         "about": "Close on the centre mic."},
        {"name": "Drums", "eye": [3 * FT, -6 * FT, 5.5 * FT],
         "look": [0.0, Y_BAND_WALL - 4 * FT, 3.5 * FT], "fov_deg": 35.0,
         "about": "Drum cam, past the singer's shoulder."},
        {"name": "Super wide", "eye": [-20 * FT, Y_HOUSE_BACK + 2 * FT, 9 * FT],
         "look": [0.0, 0.0, 4 * FT], "fov_deg": 70.0,
         "about": "The whole room from the back corner."},
        # Above the ceiling, framed to hold the whole 48 x 35 floor: at
        # 40 ft up a 56 degree view is about 42 ft of room, so nothing
        # falls off the edge. The ceiling hides itself while this is up.
        {"name": "Bird's eye", "eye": [0.0, (Y_BAND_WALL + Y_HOUSE_BACK) / 2, 40 * FT],
         "look": [0.0, (Y_BAND_WALL + Y_HOUSE_BACK) / 2, 0.0], "fov_deg": 56.0,
         "ortho": True, "about": "Plan view above the ceiling: the XY of every fixture."},
    ],
    "favourites": ["Wide", "Singer", "Drums", "Super wide", "Bird's eye"],
    "setups": [{"name": "two", "slots": ["Wide", "Singer"]},
               {"name": "four", "slots": ["Wide", "Singer", "Drums", "Super wide"]}],
}

# Where people stand, so the profile's focus roles have somewhere to
# point. Heads at 5' 1" — the same figure Riverside uses.
HEAD = 1.55
FRONT = Y_BAND_WALL - 9 * FT      # the downstage line, in front of the kit
BACK = Y_BAND_WALL - 4 * FT       # the back line, level with the drums
palettes = {
    "colors": [],
    "focus": [
        {"name": "Vocal Centre", "target": v3(0, FRONT, HEAD)},
        {"name": "Vocal Stage Left", "target": v3(8 * FT, FRONT, HEAD)},
        {"name": "Vocal Stage Right", "target": v3(-8 * FT, FRONT, HEAD)},
        {"name": "Upstage Centre", "target": v3(0, BACK, HEAD)},
        {"name": "Drums", "target": v3(0, BACK, 1.1)},
        {"name": "Stage Wide", "target": v3(0, (FRONT + BACK) / 2, HEAD)},
        {"name": "Band Wall", "target": v3(0, Y_BAND_WALL, HEIGHT / 2)},
        {"name": "Audience Front", "target": v3(0, -8 * FT, HEAD)},
        {"name": "Audience Back", "target": v3(0, -22 * FT, HEAD)},
    ],
}

# `Vocal`, `Stage` and `Audience` are required by the Ignition profile —
# `every_venue_implements_the_default_profile` fails the build without
# them, which is how this room got its focus points at all.
profile = {
    "profile": "Ignition",
    # The show targets roles and never a group by name, so what it looks
    # like here is entirely what these bindings say. Bound: everything
    # the room can honestly answer. Unbound and left that way: Movers,
    # Beams, Spot, Haze and House Lights, none of which exist in Room 138
    # -- the show names them and they are skipped with a warning, which
    # is the graceful-degradation rule rather than a fault.
    "groups": {
        # Front light on faces: the trees out in the house and the two
        # bars crossing on the frontman, which is what makes his face
        # read from both sides instead of flat from one.
        "Key": {"Group": "Front Light"},
        # The main colourable surface, and the biggest target in the show.
        "Wash": {"Group": "Towers"},
        # Behind the band: the bars washing up the wall.
        "Back": {"Group": "Wall Bars"},
        "Bars": {"Group": "Bars"},
        # Uplight and the floor package — the bars are floor-mounted, and
        # the wall pair is the uplight proper.
        "Floor": {"Group": "Wall Bars"},
        # The towers carry the warm blinder and face the room.
        "Audience": {"Group": "Towers"},
        "Drums": {"Group": "Drum Towers"},
        # The four wall movers answer both: they are what pans and tilts,
        # and being beams they are also the hard-edged package. Between
        # them these were the show's two largest unbound roles -- Movers
        # is named 71 times and Beams 21.
        "Movers": {"Group": "Movers"},
        "Beams": {"Group": "Movers"},
    },
    "focus": {"Vocal": "Vocal Centre", "Stage": "Stage Wide",
              "Audience": "Audience Front", "Band": "Upstage Centre",
              "Drums": "Drums", "Back Wall": "Band Wall",
              "House": "Audience Back"},
    "canvases": {}, "colors": [],
}

manifest = {
    "version": 1, "name": "Room 138 - CBU", "profile": "Ignition",
    "dmx": {"universes": {"1": {"sacn": {"priority": 100},
                                "artnet": {"net": 0, "subnet": 0, "universe": 0}}}},
}

OUT.mkdir(parents=True, exist_ok=True)
# `screens.json` is not optional the way groups/palettes/profile are —
# the loader reads it outright — and Room 138 has no video walls, so it
# is the empty list rather than a missing file.
screens = []

for name, data in [("room", room), ("fixtures", fixtures), ("patch", patch),
                   ("groups", groups), ("props", props), ("cameras", cameras),
                   ("screens", screens), ("palettes", palettes), ("profile", profile),
                   ("venue.ig-venue", manifest)]:
    path = OUT / (name if name.endswith(".ig-venue") else f"{name}.json")
    path.write_text(json.dumps(data, indent=2) + "\n")
print(f"{len(fixtures)} fixtures, {addr - 1} DMX channels, {len(room)} room pieces")
