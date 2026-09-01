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
            v3(x, TOWER_Y, z), (0, -1, -0.12), v3(0.18, 0.18, 0.12), 45.0, 4)
room = [r for r in room if r is not None]

# ── 2 trees in the house, 4 parcans each, facing the stage.
# A T-bar stand about 8 ft tall with the four pars side by side along
# the crossbar, as photographed — not stacked up the pole.
TREE_H = 8 * FT
TREE_Y = -14 * FT
TREE_SPAN = [-1.5 * FT, -0.5 * FT, 0.5 * FT, 1.5 * FT]
for t, x in enumerate([-16 * FT, 16 * FT], start=1):
    side = "Right" if x < 0 else "Left"
    room.append({"name": f"Column - Tree {t}", "position": v3(x, TREE_Y, TREE_H / 2),
                 "eulers": v3(0, 0, 0), "size": v3(0.09, 0.09, TREE_H)})
    room.append({"name": f"Column - Tree {t} Bar", "position": v3(x, TREE_Y, TREE_H),
                 "eulers": v3(0, 0, 0), "size": v3(4 * FT, 0.06, 0.06)})
    for n, dx in enumerate(TREE_SPAN, start=1):
        add(f"Tree {t} Par {n}", "36 LED Par Can",
            ["Luminaire_LED_Wash", "Trees", f"Tree {t}", f"Tree {side}"],
            v3(x + dx, TREE_Y, TREE_H), (-x * 0.22, 1, -0.34), v3(0.2, 0.2, 0.24), 25.0, 7)

# ── 4 floor bars: two uplighting the wall behind the band, two on the
#    house floor throwing back at the stage.
for n, x in enumerate([-9 * FT, 9 * FT], start=1):
    add(f"Wall Bar {n}", "Solena Max Bar 28 RGB",
        ["Luminaire_LED_Bar", "Bars", "Wall Bars"],
        v3(x, Y_BAND_WALL - 0.4 * FT, 0.15), (0, 0.15, 1), v3(1.0, 0.09, 0.09), 30.0, 6)
for n, x in enumerate([-9 * FT, 9 * FT], start=1):
    add(f"Floor Bar {n}", "Solena Max Bar 28 RGB",
        ["Luminaire_LED_Bar", "Bars", "Floor Bars"],
        v3(x, -6 * FT, 0.15), (0, 1, 0.55), v3(1.0, 0.09, 0.09), 30.0, 6)

def by_tag(tag):
    return [f["chan"] for f in fixtures if tag in f["tags"]]


# `target`/`label`/`channels`, the shape the other venues use — the
# target is the group's own number and the channels are strings so a
# run can be written "1-16".
_g = [("Towers", "Towers"), *[(f"Tower {t}", f"Tower {t}") for t in range(1, 5)],
      *[(f"Tower Row {n}", f"Tower Row {n}") for n in range(1, 7)],
      ("Trees", "Trees"), ("Tree Left", "Tree Left"), ("Tree Right", "Tree Right"),
      ("Bars", "Bars"), ("Wall Bars", "Wall Bars"), ("Floor Bars", "Floor Bars")]
groups = [{"target": str(i), "label": label, "channels": [str(c) for c in by_tag(tag)]}
          for i, (label, tag) in enumerate(_g, start=1)]

# The drum kit, centred on the 48 ft wall as described. Same shape the
# other venues give it — props carry a size, not a kind.
props = [
    {"name": "Drum Kit", "position": v3(0, Y_BAND_WALL - 4 * FT, 0),
     "eulers": v3(0, 0, 0), "size": v3(1.926, 1.632, 1.206)},
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
    "groups": {"Key": {"Group": "Tower Row 3"}, "Wash": {"Group": "Towers"},
               "Back": {"Group": "Wall Bars"}, "Bars": {"Group": "Bars"},
               "Floor": {"Group": "Trees"}},
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
