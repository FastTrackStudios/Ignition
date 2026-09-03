#!/usr/bin/env python3
# r[impl viz.gdtf-generated] - generator
"""Generate GDTF 1.2 profiles for cheap fixtures from a spec JSON plus a
base .gdtf whose 3D geometry we borrow.

    python3 tools/make_gdtf.py --spec data/fixtures/x.json \
        --base data/gdtf/UKing@ZQ02341...gdtf [--out data/gdtf/generated]
    python3 tools/make_gdtf.py --all [--specs data/fixtures] [--out dir]

`--all` walks every spec and picks its base from `tools/gdtf_bases.json`
(console_name -> base filename in data/gdtf).

The generated FixtureType's `Name` is the spec's `console_name`, verbatim,
so the visualizer matches fixtures.json entries on it.  Everything the
visualizer reads (DMX modes, wheels, emitters, colour space) is built from
the spec; only Models/Geometries (and the model files) come from the base.

Python 3 standard library only.
"""

import argparse
import copy
import json
import re
import sys
import uuid
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DEFAULT_OUT = REPO / "data" / "gdtf" / "generated"
DEFAULT_SPECS = REPO / "data" / "fixtures"
DEFAULT_BASES = REPO / "data" / "gdtf"
BASES_TABLE = REPO / "tools" / "gdtf_bases.json"

WHITE = "0.312700,0.329000,100.000000"

# ---------------------------------------------------------------------------
# Attribute vocabulary: spec name -> GDTF attribute.
#
# Each entry: (gdtf_name, feature "Group.Feature", activation_group,
#              physical_unit, pretty, side)
# side: "beam" (Beam geometry), "pan"/"tilt" (Yoke/Head), "base" (root).
# ---------------------------------------------------------------------------
ATTRS = {
    "Dimmer":     ("Dimmer",         "Dimmer.Dimmer",    None,       "None",         "Dim",     "beam"),
    "Pan":        ("Pan",            "Position.PanTilt", "PanTilt",  "Angle",        "P",       "pan"),
    "Tilt":       ("Tilt",           "Position.PanTilt", "PanTilt",  "Angle",        "T",       "tilt"),
    "Red":        ("ColorAdd_R",     "Color.RGB",        "ColorRGB", "ColorComponent", "R",     "beam"),
    "Green":      ("ColorAdd_G",     "Color.RGB",        "ColorRGB", "ColorComponent", "G",     "beam"),
    "Blue":       ("ColorAdd_B",     "Color.RGB",        "ColorRGB", "ColorComponent", "B",     "beam"),
    "White":      ("ColorAdd_W",     "Color.RGB",        "ColorRGB", "ColorComponent", "W",     "beam"),
    "Amber":      ("ColorAdd_RY",    "Color.RGB",        "ColorRGB", "ColorComponent", "A",     "beam"),
    "UV":         ("ColorAdd_UV",    "Color.RGB",        "ColorRGB", "ColorComponent", "UV",    "beam"),
    "Lime":       ("ColorAdd_GY",    "Color.RGB",        "ColorRGB", "ColorComponent", "L",     "beam"),
    "Cyan":       ("ColorAdd_C",     "Color.RGB",        "ColorRGB", "ColorComponent", "C",     "beam"),
    "Magenta":    ("ColorAdd_M",     "Color.RGB",        "ColorRGB", "ColorComponent", "M",     "beam"),
    "Yellow":     ("ColorAdd_Y",     "Color.RGB",        "ColorRGB", "ColorComponent", "Y",     "beam"),
    "Strobe":     ("Shutter1",       "Beam.Beam",        None,       "None",         "Sh1",     "beam"),
    "Shutter":    ("Shutter1",       "Beam.Beam",        None,       "None",         "Sh1",     "beam"),
    "Color":      ("Color1",         "Color.Color",      "ColorRGB", "None",         "C1",      "beam"),
    "Gobo":       ("Gobo1",          "Gobo.Gobo",        "Gobo1",    "None",         "G1",      "beam"),
    "GoboRotate": ("Gobo1PosRotate", "Gobo.Gobo",        "Gobo1",    "AngularSpeed", "G1 Rot",  "beam"),
    "Prism":      ("Prism1",         "Beam.Beam",        "Prism",    "None",         "Prism1",  "beam"),
    "Focus":      ("Focus1",         "Focus.Focus",      None,       "None",         "Focus1",  "beam"),
    "Zoom":       ("Zoom",           "Beam.Beam",        None,       "Angle",        "Zoom",    "beam"),
    "Speed":      ("PositionMSpeed", "Control.Control",  None,       "None",         "Pos MSpeed", "base"),
    "Macro":      ("Effects2",       "Beam.Beam",        None,       "None",         "FX2",     "base"),
    "Program":    ("Effects1",       "Beam.Beam",        None,       "None",         "FX1",     "base"),
    "Sound":      ("Sound",          "Control.Control",  None,       "None",         "Sound",   "base"),
    "Fog":        ("Fog1",           "Beam.Beam",        None,       "None",         "Fog1",    "base"),
    "Fan":        ("Fan1",           "Control.Control",  None,       "None",         "Fan1",    "base"),
    "Reset":      ("Control1",       "Control.Control",  None,       "None",         "Ctrl1",   "base"),
    "ColorMacro": ("ColorMacro1",    "Color.Color",      None,       "None",         "CMacro1", "beam"),
    "WarmWhite":  ("ColorAdd_WW",    "Color.RGB",        "ColorRGB", "ColorComponent", "WW",    "beam"),
    "CoolWhite":  ("ColorAdd_CW",    "Color.RGB",        "ColorRGB", "ColorComponent", "CW",    "beam"),
    "Control":    ("Control1",       "Control.Control",  None,       "None",         "Ctrl1",   "base"),
    "Haze":       ("Haze1",          "Beam.Beam",        None,       "None",         "Haze1",   "base"),
    "Pump":       ("Fog1",           "Beam.Beam",        None,       "None",         "Fog1",    "base"),
    "Lamp":       ("LampControl",    "Control.Control",  None,       "None",         "Lamp",    "base"),
    "Rotation":   ("Rotation",       "Control.Control",  None,       "AngularSpeed", "Rot",     "base"),
}
# Fuzzy fallbacks for attribute names outside the vocabulary ("Aux Macro",
# "Dimmer (per LED section)", "Section Control"): first matching word wins.
FUZZY = [
    ("warm white", "WarmWhite"), ("cool white", "CoolWhite"), ("color macro", "ColorMacro"),
    ("colour macro", "ColorMacro"), ("gobo rot", "GoboRotate"), ("pan fine", "PanFine"),
    ("tilt fine", "TiltFine"), ("curve", "Control"), ("reserved", "Control"), ("mode", "Control"),
    ("fade", "Control"), ("motor", "Rotation"), ("dimmer", "Dimmer"), ("intensity", "Dimmer"), ("macro", "Macro"),
    ("program", "Program"), ("speed", "Speed"), ("strobe", "Strobe"), ("shutter", "Shutter"),
    ("control", "Control"), ("function", "Control"), ("reset", "Reset"), ("sound", "Sound"),
    ("haze", "Haze"), ("fog", "Fog"), ("pump", "Pump"), ("fan", "Fan"), ("lamp", "Lamp"),
    ("rotat", "Rotation"), ("spin", "Rotation"), ("uv", "UV"), ("amber", "Amber"),
    ("white", "White"), ("red", "Red"), ("green", "Green"), ("blue", "Blue"), ("lime", "Lime"),
    ("pan", "Pan"), ("tilt", "Tilt"), ("zoom", "Zoom"), ("focus", "Focus"), ("prism", "Prism"),
    ("gobo", "Gobo"), ("colo", "Color"),
]
# Per-emitter letters as they appear in "R/G/B per section" placeholders.
LETTER_ATTR = {"R": "Red", "G": "Green", "B": "Blue", "W": "White", "A": "Amber", "UV": "UV",
               "WW": "WarmWhite", "CW": "CoolWhite", "L": "Lime"}
WORD_ATTR = {"red": "Red", "green": "Green", "blue": "Blue", "white": "White", "amber": "Amber",
             "uv": "UV", "warm white": "WarmWhite", "cool white": "CoolWhite", "lime": "Lime"}
# Sub-attributes emitted inside a channel's functions, with their MainAttribute.
SUB_ATTRS = {
    "Shutter1Strobe": ("Beam.Beam", None, "Frequency", "Strobe1", "Shutter1"),
}
FINE = {"PanFine": "Pan", "TiltFine": "Tilt"}
FEATURE_GROUP_PRETTY = {
    "Dimmer": "Dimmer", "Position": "Position", "Color": "Color", "Beam": "Beam",
    "Gobo": "Gobo", "Focus": "Focus", "Control": "Control",
}

# Typical LED emitters: spec letter -> (name, dominant wavelength nm, xy or None)
EMITTERS = {
    "R":  ("Red",   625.0, (0.6915, 0.3083)),
    "G":  ("Green", 525.0, (0.1700, 0.7500)),
    "B":  ("Blue",  465.0, (0.1355, 0.0399)),
    "W":  ("White", 0.0,   (0.3127, 0.3290)),
    "A":  ("Amber", 590.0, (0.5752, 0.4242)),
    "UV": ("UV",    395.0, None),  # spec: omit Color for non-visible emitters
    "L":  ("Lime",  567.0, (0.4400, 0.5500)),
    "C":  ("Cyan",  490.0, (0.0454, 0.2950)),
    "M":  ("Magenta", 0.0, (0.3900, 0.1899)),
    "Y":  ("Yellow", 575.0, (0.4800, 0.5100)),
}
EMITTER_ALIASES = {"RED": "R", "GREEN": "G", "BLUE": "B", "WHITE": "W", "AMBER": "A",
                   "LIME": "L", "CYAN": "C", "MAGENTA": "M", "YELLOW": "Y", "WW": "W", "CW": "W",
                   "WARM WHITE": "W", "COOL WHITE": "W"}

# Named colours for wheel slots -> CIE xyY (Y relative to open white).
NAMED_COLORS = {
    "open": (0.3127, 0.3290, 100.0), "white": (0.3127, 0.3290, 100.0),
    "red": (0.6394, 0.3302, 16.37), "yellow": (0.4500, 0.5108, 99.5),
    "green": (0.3000, 0.6000, 94.75), "magenta": (0.3900, 0.1899, 38.76),
    "blue": (0.1481, 0.0603, 17.72), "orange": (0.6025, 0.3832, 30.5),
    "cyan": (0.2419, 0.3705, 89.75), "pink": (0.4100, 0.2600, 45.0),
    "purple": (0.2700, 0.1200, 20.0), "violet": (0.2200, 0.0900, 15.0),
    "uv": (0.1800, 0.0500, 5.0), "amber": (0.5752, 0.4242, 60.0),
    "light blue": (0.2000, 0.2500, 60.0), "lightblue": (0.2000, 0.2500, 60.0),
    "dark blue": (0.1481, 0.0603, 10.0), "warm white": (0.4500, 0.4100, 90.0),
    "cool white": (0.2900, 0.3000, 100.0), "lime": (0.4400, 0.5500, 80.0),
    "turquoise": (0.2000, 0.4000, 70.0), "teal": (0.2000, 0.4000, 60.0),
    "rose": (0.4500, 0.2800, 50.0), "lavender": (0.3000, 0.2000, 45.0),
    "salmon": (0.5000, 0.3400, 50.0), "gold": (0.5000, 0.4500, 70.0),
    "peach": (0.4500, 0.3800, 65.0), "sky": (0.2300, 0.2700, 70.0),
}


class SpecError(Exception):
    pass


def fmt(x):
    return "%.6f" % float(x)


def cie(xyY):
    x, y, Y = xyY
    return "%s,%s,%s" % (fmt(x), fmt(y), fmt(Y))


def named_color(name):
    n = name.strip().lower()
    n = re.sub(r"[_\-]+", " ", n)
    if n in NAMED_COLORS:
        return NAMED_COLORS[n]
    # "Open/White", "Light Blue 2", "Red + Yellow (split)": first known word run
    for key in sorted(NAMED_COLORS, key=len, reverse=True):
        if re.search(r"\b" + re.escape(key) + r"\b", n):
            return NAMED_COLORS[key]
    return NAMED_COLORS["white"]


def number(x, default=None):
    """First number in a value the spec may have written as prose
    ('1452 lux @ 1 m')."""
    if x is None or isinstance(x, bool):
        return default
    if isinstance(x, (int, float)):
        return float(x)
    m = re.search(r"-?\d+(?:\.\d+)?", str(x))
    return float(m.group(0)) if m else default


def resolve_attr(name):
    """Spec attribute -> vocabulary key, or None when nothing matches."""
    n = str(name).strip()
    if n in ATTRS or n in FINE:
        return n
    bare = re.sub(r"\(.*?\)", " ", n)
    bare = re.sub(r"\s+\d+$", "", bare).strip()   # "Warm White 3" -> "Warm White"
    if bare in ATTRS or bare in FINE:
        return bare
    low = bare.lower()
    for needle, key in FUZZY:
        if needle in low:
            return key
    return None


def expand_channels(mode_name, channels, all_modes, warn):
    """Flatten a spec mode into [(number, spec_attr, ranges)], expanding the
    compressed forms the research specs use:

      {"channel": "1-24", "attribute": "Dimmer (per LED section)"}
      {"channel": "1-24", "attribute": "Red/Green/Blue per section"}
      {"channel": "1-15", "attribute": "as 15ch"}
    """
    out = []
    for ch in channels:
        raw = str(ch["channel"]).strip()
        attr = str(ch["attribute"]).strip()
        ranges = ch.get("ranges") or []
        m = re.fullmatch(r"(\d+)\s*[-–]\s*(\d+)", raw)
        if not m:
            key = resolve_attr(attr)
            if key is None:
                warn("mode %r channel %s: attribute %r is outside the vocabulary; emitted as user-defined"
                     % (mode_name, raw, attr))
                key = ("*", attr)
            out.append((int(raw), key, ranges))
            continue
        lo, hi = int(m.group(1)), int(m.group(2))
        count = hi - lo + 1
        ref = re.fullmatch(r"(?:as|same as|identical to)\s+(.+)", attr, re.I)
        if ref and ref.group(1).strip() in all_modes:
            other = ref.group(1).strip()
            copied = expand_channels(other, all_modes[other], {k: v for k, v in all_modes.items() if k != mode_name}, warn)
            for n, key, r in copied:
                if lo <= n <= hi:
                    out.append((n, key, r))
            continue
        head = re.split(r"\bper\b|\bfor each\b|\beach\b", attr, flags=re.I)[0]
        parts = [p.strip() for p in re.split(r"[/,+&]", head) if p.strip()]
        keys = []
        for p in parts:
            k = LETTER_ATTR.get(p.upper()) or WORD_ATTR.get(p.lower()) or resolve_attr(p)
            if k is None:
                keys = []
                break
            keys.append(k)
        if not keys:
            warn("mode %r channels %s: cannot expand %r; skipped" % (mode_name, raw, attr))
            continue
        if count % len(keys):
            warn("mode %r channels %s: %d channels do not split into %s; skipped"
                 % (mode_name, raw, count, keys))
            continue
        for i in range(count):
            out.append((lo + i, keys[i % len(keys)], ranges if len(keys) == 1 else []))
    return out


def safe_name(s):
    # GDTF Name type: no '.', ',', '"' etc.  Keep it readable.
    return re.sub(r'[.",]', " ", str(s)).strip() or "unnamed"


def fixture_type_id(console_name):
    return str(uuid.uuid5(uuid.NAMESPACE_URL, "ignition:gdtf:" + console_name)).upper()


# ---------------------------------------------------------------------------
# Base geometry
# ---------------------------------------------------------------------------
GEOMETRY_TAGS = {"Geometry", "Axis", "FilterBeam", "FilterColor", "FilterGobo",
                 "FilterShaper", "Beam", "MediaServerLayer", "MediaServerCamera",
                 "MediaServerMaster", "Display", "GeometryReference", "Laser",
                 "WiringObject", "Inventory", "Structure", "Support", "Magnet"}


IDENTITY = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]


def parse_matrix(text):
    """A GDTF Position `{r0}{r1}{r2}{r3}` into 4 rows of 4 floats (row-major,
    translation in the last column of the first three rows)."""
    if not text:
        return [row[:] for row in IDENTITY]
    rows = [[float(v) for v in row.split(",")] for row in re.findall(r"\{([^}]*)\}", text)]
    if len(rows) != 4 or any(len(r) != 4 for r in rows):
        return [row[:] for row in IDENTITY]
    return rows


def format_matrix(rows):
    return "".join("{%s}" % ",".join(fmt(v) for v in row) for row in rows)


def mat_mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def mat_apply(m, p):
    x, y, z = p
    return tuple(m[i][0] * x + m[i][1] * y + m[i][2] * z + m[i][3] for i in range(3))


def model_box(base, model):
    """The box a <Model> draws about its own pivot, in metres: (lo, hi)."""
    size = tuple(float(model.get(k, 0)) for k in ("Length", "Width", "Height"))
    raw = None
    file = (model.get("File") or "").strip()
    if file:
        raw = base.mesh_bounds(file)
    if raw is not None:
        lo, hi = raw
        extent = [hi[i] - lo[i] for i in range(3)]
        ratios = [size[i] / extent[i] if extent[i] > 1e-6 and size[i] > 1e-6 else None
                  for i in range(3)]
        fallback = next((r for r in ratios if r is not None), 1.0)
        scale = [r if r is not None else fallback for r in ratios]
        return (tuple(lo[i] * scale[i] for i in range(3)),
                tuple(hi[i] * scale[i] for i in range(3)))
    length, width, height = size
    if length <= 0 and width <= 0 and height <= 0:
        return None
    hangs = model.get("PrimitiveType", "") in ("Base", "Base1_1")
    z0, z1 = (-height, 0.0) if hangs else (-height / 2, height / 2)
    return (-length / 2, -width / 2, z0), (length / 2, width / 2, z1)


def glb_bounds(data):
    """Bounds of every mesh in a .glb, node transforms applied, remapped
    from glTF's Y-up to GDTF's Z-up the way the visualizer does it."""
    import struct
    if data[:4] != b"glTF":
        return None
    length = struct.unpack_from("<I", data, 12)[0]
    doc = json.loads(data[20:20 + length])
    accessors = doc.get("accessors", [])
    meshes = doc.get("meshes", [])
    nodes = doc.get("nodes", [])
    lo = [float("inf")] * 3
    hi = [float("-inf")] * 3

    def node_matrix(node):
        if "matrix" in node:
            m = node["matrix"]  # column-major
            return [[m[c * 4 + r] for c in range(4)] for r in range(4)]
        t = node.get("translation", [0, 0, 0])
        q = node.get("rotation", [0, 0, 0, 1])
        sc = node.get("scale", [1, 1, 1])
        x, y, z, w = q
        rot = [[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
               [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
               [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]]
        return [[rot[r][c] * sc[c] for c in range(3)] + [t[r]] for r in range(3)] + [[0, 0, 0, 1]]

    def visit(index, parent):
        node = nodes[index]
        mat = mat_mul(parent, node_matrix(node))
        if "mesh" in node:
            for prim in meshes[node["mesh"]].get("primitives", []):
                acc = accessors[prim.get("attributes", {}).get("POSITION", -1)] if prim.get("attributes", {}).get("POSITION") is not None else None
                if acc is None or "min" not in acc or "max" not in acc:
                    continue
                for x in (acc["min"][0], acc["max"][0]):
                    for y in (acc["min"][1], acc["max"][1]):
                        for z in (acc["min"][2], acc["max"][2]):
                            wx, wy, wz = mat_apply(mat, (x, y, z))
                            for i, v in enumerate((wx, -wz, wy)):
                                lo[i] = min(lo[i], v)
                                hi[i] = max(hi[i], v)
        for child in node.get("children", []):
            visit(child, mat)

    scene = doc.get("scenes", [{}])[doc.get("scene", 0)] if doc.get("scenes") else {}
    roots = scene.get("nodes") or list(range(len(nodes)))
    for r in roots:
        visit(r, IDENTITY)
    if lo[0] == float("inf"):
        return None
    return tuple(lo), tuple(hi)


def tds_bounds(data):
    """Bounds of every vertex list in a .3ds (Z-up already)."""
    import struct
    lo = [float("inf")] * 3
    hi = [float("-inf")] * 3

    def chunks(buf):
        at = 0
        while at + 6 <= len(buf):
            cid, size = struct.unpack_from("<HI", buf, at)
            if size < 6 or at + size > len(buf):
                return
            yield cid, buf[at + 6:at + size]
            at += size

    def walk(buf, depth):
        for cid, body in chunks(buf):
            if cid in (0x4D4D, 0x3D3D):
                walk(body, depth + 1)
            elif cid == 0x4000:
                # Named object: a NUL-terminated name, then sub-chunks.
                end = body.find(b"\0")
                walk(body[end + 1:], depth + 1)
            elif cid == 0x4100:
                walk(body, depth + 1)
            elif cid == 0x4110:
                n = struct.unpack_from("<H", body, 0)[0]
                for i in range(n):
                    v = struct.unpack_from("<fff", body, 2 + i * 12)
                    for k in range(3):
                        lo[k] = min(lo[k], v[k])
                        hi[k] = max(hi[k], v[k])

    if data[:2] != b"\x4d\x4d":
        return None
    walk(data, 0)
    if lo[0] == float("inf"):
        return None
    return tuple(lo), tuple(hi)


class Base:
    """The parts of a base .gdtf we borrow."""

    def __init__(self, path):
        self.path = Path(path)
        self.zip = zipfile.ZipFile(self.path)
        self.root = ET.fromstring(self.zip.read("description.xml"))
        fixture_type = self.root.find("FixtureType")
        if fixture_type is None:
            raise SpecError("base %s has no <FixtureType>" % path)
        self.fixture_type = fixture_type
        self.models = self.fixture_type.find("Models")
        self.geometries = self.fixture_type.find("Geometries")
        if self.models is None or self.geometries is None:
            raise SpecError("%s: base has no Models/Geometries to borrow" % path)
        self.root_geometry = next(iter(self.geometries), None)
        if self.root_geometry is None:
            raise SpecError("%s: base Geometries is empty" % path)
        self.parents = {}
        for parent in self.geometries.iter():
            for child in parent:
                self.parents[child] = parent
        self.beams = [b for b in self.geometries.iter("Beam")]

    def ancestors(self, node):
        """Geometry ancestors of `node`, nearest first, stopping at the root."""
        chain = []
        while node in self.parents and self.parents[node] is not self.geometries:
            node = self.parents[node]
            chain.append(node)
        return chain

    def moving_head_axes(self):
        """(yoke, head) geometry nodes for the first beam; None if the base
        is not shaped root > yoke > head > beam."""
        if not self.beams:
            return None
        chain = self.ancestors(self.beams[0])
        # chain = [head, yoke, ..., root]
        if len(chain) < 3:
            return None
        head, yoke = chain[0], chain[1]
        if yoke is self.root_geometry:
            return None
        return yoke, head

    def name(self):
        return self.fixture_type.get("Name", "")

    def mesh_bounds(self, file):
        """Raw bounds of a Model's 3D file, Z-up; the glb wins over the
        3ds as it does in the visualizer. None when neither decodes."""
        for name in ("models/gltf/%s.glb" % file, "models/3ds/%s.3ds" % file):
            if name not in self.zip.namelist():
                continue
            data = self.zip.read(name)
            bounds = glb_bounds(data) if name.endswith(".glb") else tds_bounds(data)
            if bounds:
                return bounds
        return None

    def model_files(self):
        return [n for n in self.zip.namelist()
                if n.startswith("models/") and not n.endswith("/")]

    def thumbnail(self):
        for n in self.zip.namelist():
            if n.startswith("thumbnail."):
                return n
        return None


# ---------------------------------------------------------------------------
# Generator
# ---------------------------------------------------------------------------
class Generator:
    def __init__(self, spec, base):
        self.spec = spec
        self.base = base
        self.used_attrs = {}       # gdtf name -> definition tuple
        self.used_sub_attrs = {}
        self.used_groups = set()
        self.used_features = set()
        self.color_wheel = None
        self.gobo_wheel = None
        self.warnings = []
        self.validate_spec()

    def warn(self, msg):
        self.warnings.append(msg)

    # -- spec sanity ----------------------------------------------------
    def validate_spec(self):
        s = self.spec
        for key in ("console_name", "modes"):
            if key not in s or not s[key]:
                raise SpecError("spec is missing %r" % key)
        if not s.get("model"):
            self.warn("spec has no model; using the console name")
            s["model"] = s["console_name"]
        if not s.get("manufacturer"):
            self.warn("spec has no manufacturer; using 'Unknown'")
            s["manufacturer"] = "Unknown"
        if not isinstance(s["modes"], dict):
            raise SpecError("spec 'modes' must be an object keyed by mode name")
        # Sidecar flags like "8ch_ranges_guess": true sit beside the modes.
        s["modes"] = {k: v for k, v in s["modes"].items() if isinstance(v, list)}
        if not s["modes"]:
            raise SpecError("spec has no modes")
        self.expanded = {}
        for mode_name, channels in s["modes"].items():
            if not isinstance(channels, list) or not channels:
                raise SpecError("mode %r has no channels" % mode_name)
            for ch in channels:
                if "channel" not in ch or "attribute" not in ch:
                    raise SpecError("mode %r: every channel needs 'channel' and 'attribute'" % mode_name)
            flat = expand_channels(mode_name, channels, s["modes"], self.warn)
            seen = set()
            for n, _, _ in flat:
                if n < 1 or n in seen:
                    raise SpecError("mode %r: channel %d duplicated or < 1" % (mode_name, n))
                seen.add(n)
            if not flat:
                raise SpecError("mode %r: no channel could be expanded" % mode_name)
            self.expanded[mode_name] = flat
        mv = s.get("movement") or {}
        if number(mv.get("pan_deg")) or number(mv.get("tilt_deg")):
            axes = self.base.moving_head_axes()
            if axes is None:
                raise SpecError(
                    "%s is a moving head but base %s has no Base > Yoke > Head > Beam geometry chain"
                    % (s["console_name"], self.base.path.name))

    # -- attribute bookkeeping ------------------------------------------
    def use(self, spec_attr):
        if isinstance(spec_attr, tuple):   # ("*", raw name): user-defined attribute
            name = re.sub(r"[^A-Za-z0-9]+", "", str(spec_attr[1]).title()) or "Custom"
            d = (name, "Control.Control", None, "None", name[:10], "base")
        else:
            d = ATTRS[spec_attr]
        self.used_attrs[d[0]] = d
        group, feature = d[1].split(".")
        self.used_features.add((group, feature))
        if d[2]:
            self.used_groups.add(d[2])
        return d

    def use_sub(self, name):
        d = SUB_ATTRS[name]
        self.used_sub_attrs[name] = d
        group, feature = d[0].split(".")
        self.used_features.add((group, feature))

    # -- top level --------------------------------------------------------
    def build(self):
        s = self.spec
        gdtf = ET.Element("GDTF", DataVersion="1.2")
        thumb = self.base.thumbnail()
        ft = ET.SubElement(gdtf, "FixtureType", {
            "Name": s["console_name"],
            "ShortName": safe_name(s.get("short_name") or s["model"]),
            "LongName": safe_name("%s %s" % (s["manufacturer"], s["model"])),
            "Manufacturer": safe_name(s["manufacturer"]),
            "Description": self.description(),
            "FixtureTypeID": fixture_type_id(s["console_name"]),
            "Thumbnail": "thumbnail" if thumb else "",
            "RefFT": "",
            "CanHaveChildren": "No",
        })
        # Order matters for readers that expect the spec's section order.
        attr_defs = ET.SubElement(ft, "AttributeDefinitions")
        wheels = ET.SubElement(ft, "Wheels")
        phys = ET.SubElement(ft, "PhysicalDescriptions")
        models = ET.SubElement(ft, "Models")
        geoms = ET.SubElement(ft, "Geometries")
        modes = ET.SubElement(ft, "DMXModes")
        ET.SubElement(ft, "Revisions")
        ET.SubElement(ft, "FTPresets")
        ET.SubElement(ft, "Protocols")

        self.build_wheels(wheels)
        self.build_models(models)
        self.build_geometries(geoms)
        for mode_name, channels in s["modes"].items():
            self.build_mode(modes, mode_name, channels)
        # Attribute definitions depend on what the modes used.
        self.build_attribute_definitions(attr_defs)
        self.build_physical(phys)
        return gdtf

    def description(self):
        s = self.spec
        parts = ["Generated by tools/make_gdtf.py for Ignition; geometry borrowed from %s."
                 % self.base.path.name]
        if s.get("notes"):
            parts.append(s["notes"])
        if s.get("sources"):
            parts.append("Sources: " + "; ".join(str(x) for x in s["sources"]))
        return " ".join(parts)

    # -- wheels -------------------------------------------------------------
    def build_wheels(self, wheels):
        w = self.spec.get("wheels") or {}
        if w.get("color"):
            self.color_wheel = ET.SubElement(wheels, "Wheel", Name="Color Wheel")
            for name in w["color"]:
                ET.SubElement(self.color_wheel, "Slot",
                              Name=safe_name(name), Color=cie(named_color(name)))
        if w.get("gobo"):
            self.gobo_wheel = ET.SubElement(wheels, "Wheel", Name="Gobo Wheel")
            for name in w["gobo"]:
                ET.SubElement(self.gobo_wheel, "Slot", Name=safe_name(name), Color=WHITE)

    def slot_index(self, wheel, meaning):
        """1-based WheelSlotIndex for a range meaning, matched by slot name."""
        if wheel is None:
            return None
        m = re.sub(r"[^a-z0-9]+", " ", meaning.lower()).strip()
        for i, slot in enumerate(wheel, start=1):
            n = re.sub(r"[^a-z0-9]+", " ", slot.get("Name").lower()).strip()
            if n and (n == m or m.startswith(n) or m.endswith(n)):
                return i
        return None

    # -- models / geometry --------------------------------------------------
    def build_models(self, models):
        for m in self.base.models:
            models.append(copy.deepcopy(m))
        self.geometry_scale = (1.0, 1.0, 1.0)
        phys = self.spec.get("physical") or {}
        dims = {k: number(phys.get(k)) for k in ("width_mm", "length_mm", "height_mm")}
        if not all(dims.values()):
            return
        width_m, length_m, height_m = (
            float(dims[k] or 0) / 1000.0 for k in ("width_mm", "length_mm", "height_mm"))
        same_product = self.spec["model"].lower() in (self.base.name().lower() +
                                                      self.base.fixture_type.get("LongName", "").lower())
        if same_product:
            return
        # Scale the base so the *assembled* fixture's box matches the spec.
        # The parts overlap (a head sits inside its yoke, a yoke inside
        # its base's drop), so the height is what the file's own
        # placements add up to, not the sum of the Models' heights — that
        # sum shrank every mini mover to two thirds of its real size. The
        # same factors go on every Model and, in `build_geometries`, on
        # every Position, so the parts stay where they meet and the
        # <Beam> stays on the lens.
        extents = self.base_extents()
        if not extents:
            return
        base_x, base_y, base_z = extents
        # A listing's width/length are not axis-tagged (GDTF Length is X,
        # Width is Y, and a bar's long side is whichever the author drew
        # it on), so the larger of the pair goes on the base's longer
        # horizontal axis.
        spec_long, spec_short = sorted([width_m, length_m], reverse=True)
        if base_x >= base_y:
            sx, sy = spec_long / base_x, spec_short / base_y
        else:
            sx, sy = spec_short / base_x, spec_long / base_y
        sz = height_m / base_z
        self.geometry_scale = (sx, sy, sz)
        for m in models:
            m.set("Length", fmt(float(m.get("Length", 0)) * sx))
            m.set("Width", fmt(float(m.get("Width", 0)) * sy))
            m.set("Height", fmt(float(m.get("Height", 0)) * sz))

    def base_extents(self):
        """(x, y, z) size of the base file's drawn parts, assembled with
        the file's own Position matrices and every joint at rest. A Model
        with a 3D file is that file's real bounds fitted to its
        Length/Width/Height about the pivot — the same fit the visualizer
        applies (`gdtf_geometry::fit_scale`); a standard primitive is a
        box that hangs below the pivot for a Base and is centred on it
        otherwise, which is how the spec's primitive meshes come out.
        <Beam> nodes draw nothing."""
        by_name = {m.get("Name"): m for m in self.base.models}
        lo = [float("inf")] * 3
        hi = [float("-inf")] * 3

        def walk(node, parent):
            mat = mat_mul(parent, parse_matrix(node.get("Position")))
            model = by_name.get(node.get("Model"))
            # A pigtail is the cable stub, which no listing's dimensions
            # include; a <Beam> draws nothing.
            is_housing = model is not None and node.tag != "Beam" and \
                model.get("PrimitiveType", "") != "Pigtail"
            if is_housing:
                box = model_box(self.base, model)
                if box is not None:
                    (x0, y0, z0), (x1, y1, z1) = box
                    for x in (x0, x1):
                        for y in (y0, y1):
                            for z in (z0, z1):
                                w = mat_apply(mat, (x, y, z))
                                for i, v in enumerate(w):
                                    lo[i] = min(lo[i], v)
                                    hi[i] = max(hi[i], v)
            for child in node:
                if child.tag in GEOMETRY_TAGS:
                    walk(child, mat)

        for top in self.base.geometries:
            walk(top, IDENTITY)
        if lo[0] == float("inf"):
            return None
        return tuple(max(hi[i] - lo[i], 1e-6) for i in range(3))

    def build_geometries(self, geoms):
        for g in self.base.geometries:
            geoms.append(copy.deepcopy(g))
        sx, sy, sz = self.geometry_scale
        if (sx, sy, sz) != (1.0, 1.0, 1.0):
            for node in geoms.iter():
                if node.tag in GEOMETRY_TAGS and node.get("Position"):
                    rows = parse_matrix(node.get("Position"))
                    rows[0][3] *= sx
                    rows[1][3] *= sy
                    rows[2][3] *= sz
                    node.set("Position", format_matrix(rows))
        optics = self.spec.get("optics") or {}
        phys = self.spec.get("physical") or {}
        for beam in geoms.iter("Beam"):
            beam_angle = number(optics.get("beam_angle_deg"))
            field_angle = number(optics.get("field_angle_deg"))
            if beam_angle:
                beam.set("BeamAngle", fmt(beam_angle))
            if field_angle:
                beam.set("FieldAngle", fmt(field_angle))
            elif beam_angle:
                # No published field angle: an LED par's 10% edge sits at
                # roughly twice its 50% beam angle, so that is what the
                # visualizer assumes for a profile with no FieldAngle, and
                # the file states the same assumption rather than a
                # different one.
                beam.set("FieldAngle", fmt(beam_angle * 2.0))
            if number(optics.get("lumens_or_lux")):
                beam.set("LuminousFlux", fmt(number(optics.get("lumens_or_lux"))))
            if number(phys.get("power_w")):
                beam.set("PowerConsumption", fmt(number(phys.get("power_w"))))
            beam.set("LampType", "LED" if "led" in str(optics.get("source", "LED")).lower() else "Discharge")
        # The generated file's beam geometries, by name, in document order.
        self.beam_names = [b.get("Name") for b in geoms.iter("Beam")]
        self.root_name = self.base.root_geometry.get("Name")
        axes = self.base.moving_head_axes()
        self.yoke_name = axes[0].get("Name") if axes else self.root_name
        self.head_name = axes[1].get("Name") if axes else self.root_name

    # -- DMX modes ------------------------------------------------------------
    def build_mode(self, modes, mode_name, channels):
        mode = ET.SubElement(modes, "DMXMode", Name=safe_name(mode_name), Geometry=self.root_name)
        dmx_channels = ET.SubElement(mode, "DMXChannels")
        ET.SubElement(mode, "Relations")
        ET.SubElement(mode, "FTMacros")

        flat = self.expanded[mode_name]
        by_number = {n: (key, ranges) for n, key, ranges in flat}
        attrs_present = {key for _, key, _ in flat}
        fine_for = {}   # coarse spec attr -> fine channel number
        for n, key, _ in flat:
            if key in FINE:
                if FINE[key] not in attrs_present:
                    raise SpecError("mode %r: %s without a %s channel" % (mode_name, key, FINE[key]))
                fine_for[FINE[key]] = n
        beam_cursor = {}  # gdtf attr -> how many times used (pixel bars)
        base_used = {}    # gdtf attr -> uses on the root geometry
        for number in sorted(by_number):
            spec_attr, ranges = by_number[number]
            if spec_attr in FINE:
                continue  # folded into the coarse channel's Offset
            d = self.use(spec_attr)
            gdtf_attr, side = d[0], d[5]
            offsets = [number]
            if spec_attr in fine_for:
                offsets.append(fine_for[spec_attr])
            nbytes = len(offsets)

            if side == "pan":
                geometry = self.yoke_name
            elif side == "tilt":
                geometry = self.head_name
            elif side == "base" or not self.beam_names:
                geometry = self.root_name
                k = base_used.get(gdtf_attr, 0)
                base_used[gdtf_attr] = k + 1
                if k:
                    # A second Macro/Control/Speed on the same fixture: a
                    # numbered sibling, so channel names stay unique.
                    m = re.fullmatch(r"(.*?)(\d+)", gdtf_attr)
                    gdtf_attr = "%s%d" % (m.group(1), int(m.group(2)) + k) if m else "%s%d" % (gdtf_attr, k + 1)
                    self.used_attrs[gdtf_attr] = (gdtf_attr,) + d[1:]
            else:
                k = beam_cursor.get(gdtf_attr, 0)
                beam_cursor[gdtf_attr] = k + 1
                if k and k % len(self.beam_names) == 0:
                    self.warn("mode %r: %s repeats more than the base's %d beams; wrapping"
                              % (mode_name, spec_attr, len(self.beam_names)))
                geometry = self.beam_names[k % len(self.beam_names)]

            functions = self.functions_for(spec_attr, (gdtf_attr,) + d[1:], ranges, nbytes)
            first_name = functions[0][1]
            dmx = ET.SubElement(dmx_channels, "DMXChannel", {
                "DMXBreak": "1",
                "Offset": ",".join(str(o) for o in offsets),
                "InitialFunction": "%s_%s.%s.%s" % (geometry, gdtf_attr, gdtf_attr, first_name),
                "Highlight": ("%d/%d" % ((1 << (8 * nbytes)) - 1, nbytes)
                              if gdtf_attr in ("Dimmer",) or gdtf_attr.startswith("ColorAdd_") else "None"),
                "Geometry": geometry,
            })
            logical = ET.SubElement(dmx, "LogicalChannel", {
                "Attribute": gdtf_attr,
                "Snap": "Yes" if gdtf_attr in ("Shutter1", "Color1", "Gobo1", "Prism1", "Control1") else "No",
                "Master": "Grand" if gdtf_attr == "Dimmer" else "None",
                "MDependant": "No",
                "DMXChangeTimeLimit": "0.000000",
            })
            default = self.default_for(spec_attr, functions, nbytes)
            for attr_name, name, dmx_from, phys_from, phys_to, wheel, sets in functions:
                cf = ET.SubElement(logical, "ChannelFunction", {
                    "Name": name,
                    "Attribute": attr_name,
                    "DMXFrom": "%d/%d" % (dmx_from, nbytes),
                    "Default": "%d/%d" % (default, nbytes),
                    "PhysicalFrom": fmt(phys_from),
                    "PhysicalTo": fmt(phys_to),
                    "RealFade": "0.000000",
                })
                if wheel is not None:
                    cf.set("Wheel", wheel.get("Name"))
                emitter = self.emitter_name_for(gdtf_attr)
                if emitter:
                    cf.set("Emitter", emitter)
                for set_name, set_from, slot in sets:
                    cs = ET.SubElement(cf, "ChannelSet", {
                        "Name": safe_name(set_name),
                        "DMXFrom": "%d/%d" % (set_from, nbytes),
                    })
                    if slot is not None:
                        cs.set("WheelSlotIndex", str(slot))

    def full_scale(self, spec_attr, nbytes):
        """(PhysicalFrom, PhysicalTo) for a channel with no ranges."""
        mv = self.spec.get("movement") or {}
        if spec_attr == "Pan":
            deg = number(mv.get("pan_deg"), 540.0)
            return -deg / 2, deg / 2
        if spec_attr == "Tilt":
            deg = number(mv.get("tilt_deg"), 270.0)
            return -deg / 2, deg / 2
        if spec_attr == "Zoom":
            o = self.spec.get("optics") or {}
            return number(o.get("beam_angle_deg"), 10.0), number(o.get("field_angle_deg"), 40.0)
        if spec_attr in ("GoboRotate", "Rotation"):
            return -360.0, 360.0
        return 0.0, 1.0

    def functions_for(self, spec_attr, d, ranges, nbytes):
        """[(attribute, name, dmx_from, phys_from, phys_to, wheel_elem, [(set_name, set_from, slot)])]"""
        gdtf_attr = d[0]
        scale = 1 << (8 * (nbytes - 1))  # spec ranges are 8-bit; widen for 16-bit
        pf, pt = self.full_scale(spec_attr, nbytes)
        if not ranges:
            name = gdtf_attr if isinstance(spec_attr, tuple) or spec_attr == "Strobe" else spec_attr
            return [(gdtf_attr, name, 0, pf, pt, None, [])]

        wheel = self.color_wheel if spec_attr == "Color" else self.gobo_wheel if spec_attr == "Gobo" else None
        ranges = sorted(ranges, key=lambda r: int(r["from"]))
        out = []
        used_names = set()
        for r in ranges:
            lo, hi = int(r["from"]), int(r["to"])
            meaning = str(r.get("meaning") or "%d-%d" % (lo, hi))
            name = safe_name(meaning)
            base_name = name
            k = 2
            while name in used_names:
                name = "%s %d" % (base_name, k)
                k += 1
            used_names.add(name)
            m = meaning.lower()
            attr = gdtf_attr
            f, t = pf, pt
            sets = []
            slot = self.slot_index(wheel, meaning)
            if spec_attr in ("Strobe", "Shutter"):
                if "strobe" in m or "pulse" in m or "flash" in m or "random" in m:
                    attr = "Shutter1Strobe"
                    self.use_sub(attr)
                    f, t = parse_hz(m, 1.0, 25.0)
                elif "closed" in m or "off" in m or "black" in m:
                    f = t = 0.0
                else:
                    f = t = 1.0
            elif spec_attr in ("Color", "Gobo"):
                if slot is not None:
                    sets.append((meaning, lo, slot))
                    f = t = 0.0
                elif wheel is not None and ("rotat" in m or "spin" in m or "scroll" in m):
                    attr = "Color1WheelSpin" if spec_attr == "Color" else "Gobo1WheelSpin"
                    self.used_attrs.setdefault(attr, (attr, d[1], d[2], "AngularSpeed", "Wheel Spin", "beam"))
                    f, t = (-360.0, 360.0)
                elif wheel is not None and "shake" in m:
                    attr = "Gobo1WheelShake" if spec_attr == "Gobo" else "Color1WheelShake"
                    self.used_attrs.setdefault(attr, (attr, d[1], d[2], "Frequency", "Wheel Shake", "beam"))
                    f, t = (1.0, 4.0)
                else:
                    f, t = 0.0, 1.0
            elif spec_attr in ("Pan", "Tilt"):
                pass  # physical is the full travel regardless of the range split
            else:
                # Generic: position within the byte scale.
                f, t = lo / 255.0, hi / 255.0
            out.append((attr, name, lo * scale, f, t, wheel, sets))
        return out

    def default_for(self, spec_attr, functions, nbytes):
        top = (1 << (8 * nbytes)) - 1
        if spec_attr in ("Pan", "Tilt"):
            return (top + 1) // 2
        return functions[0][2]

    def emitter_name_for(self, gdtf_attr):
        letter = {"ColorAdd_R": "R", "ColorAdd_G": "G", "ColorAdd_B": "B", "ColorAdd_W": "W",
                  "ColorAdd_RY": "A", "ColorAdd_UV": "UV", "ColorAdd_GY": "L", "ColorAdd_C": "C",
                  "ColorAdd_M": "M", "ColorAdd_Y": "Y"}.get(gdtf_attr)
        if letter and letter in self.emitter_letters():
            return EMITTERS[letter][0]
        return None

    def emitter_letters(self):
        optics = self.spec.get("optics") or {}
        out = []
        for e in optics.get("emitters") or []:
            key = str(e).strip().upper()
            key = EMITTER_ALIASES.get(key, key)
            if key in EMITTERS and key not in out:
                out.append(key)
        return out

    # -- attribute definitions / physical ---------------------------------
    def build_attribute_definitions(self, attr_defs):
        groups = ET.SubElement(attr_defs, "ActivationGroups")
        for g in sorted(self.used_groups):
            ET.SubElement(groups, "ActivationGroup", Name=g)
        fgs = ET.SubElement(attr_defs, "FeatureGroups")
        by_group = {}
        for group, feature in sorted(self.used_features):
            by_group.setdefault(group, []).append(feature)
        for group, features in by_group.items():
            fg = ET.SubElement(fgs, "FeatureGroup", Name=group, Pretty=FEATURE_GROUP_PRETTY.get(group) or group)
            for feature in features:
                ET.SubElement(fg, "Feature", Name=feature)
        attrs = ET.SubElement(attr_defs, "Attributes")
        for name, d in self.used_attrs.items():
            a = {"Name": name, "Pretty": d[4], "Feature": d[1], "PhysicalUnit": d[3]}
            if d[2]:
                a["ActivationGroup"] = d[2]
            if name.endswith(("WheelSpin", "WheelShake")):
                a["MainAttribute"] = name[:-len("WheelSpin")] if name.endswith("WheelSpin") else name[:-len("WheelShake")]
            ET.SubElement(attrs, "Attribute", a)
        for name, d in self.used_sub_attrs.items():
            a = {"Name": name, "Pretty": d[3], "Feature": d[0], "PhysicalUnit": d[2], "MainAttribute": d[4]}
            if d[1]:
                a["ActivationGroup"] = d[1]
            ET.SubElement(attrs, "Attribute", a)

    def build_physical(self, phys):
        ET.SubElement(phys, "ColorSpace", Mode="sRGB", Name="")
        ET.SubElement(phys, "AdditionalColorSpaces")
        ET.SubElement(phys, "Gamuts")
        ET.SubElement(phys, "Filters")
        emitters = ET.SubElement(phys, "Emitters")
        for letter in self.emitter_letters():
            name, wl, xy = EMITTERS[letter]
            e = {"Name": name, "DiodePart": "", "DominantWaveLength": fmt(wl)}
            if xy:
                e["Color"] = cie((xy[0], xy[1], 100.0))
            em = ET.SubElement(emitters, "Emitter", e)
            ET.SubElement(em, "Measurement", Physical="100.000000",
                          LuminousIntensity="1.000000", InterpolationTo="Linear")
        ET.SubElement(phys, "DMXProfiles")
        ET.SubElement(phys, "CRIs")
        ET.SubElement(phys, "Connectors")
        props = ET.SubElement(phys, "Properties")
        p = self.spec.get("physical") or {}
        if number(p.get("weight_kg")):
            ET.SubElement(props, "Weight", Value=fmt(number(p.get("weight_kg"))))
        ET.SubElement(props, "LegHeight", Value="0.000000")


def parse_hz(text, lo, hi):
    """'strobe 1-25 hz' -> (1.0, 25.0); default when nothing is given."""
    m = re.search(r"(\d+(?:\.\d+)?)\s*(?:-|to|–)\s*(\d+(?:\.\d+)?)\s*hz", text)
    if m:
        return float(m.group(1)), float(m.group(2))
    return lo, hi


# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------
def output_name(spec):
    """<Manufacturer>@<console name>@ignition.gdtf — the console name is the
    stable, short identity; spec `model` strings tend to be prose."""
    def clean(s):
        return re.sub(r"[^A-Za-z0-9_\-]+", "_", s.strip()).strip("_")
    manufacturer = re.split(r"[(/;,]", spec.get("manufacturer") or "Unknown")[0]
    return "%s@%s@ignition.gdtf" % (clean(manufacturer), clean(spec["console_name"]))


def indent(elem, level=0):
    pad = "\n" + "  " * level
    if len(elem):
        if not elem.text or not elem.text.strip():
            elem.text = pad + "  "
        children = list(elem)
        for child in children:
            indent(child, level + 1)
            if not child.tail or not child.tail.strip():
                child.tail = pad + "  "
        last = children[-1]
        if not last.tail or not last.tail.strip():
            last.tail = pad
    if level and (not elem.tail or not elem.tail.strip()):
        elem.tail = pad


def console_names(spec):
    """`console_name` may be one string or a list of aliases the venue
    uses for the same fixture; each alias gets its own file."""
    names = spec.get("console_name")
    if isinstance(names, str):
        names = [names]
    if not (isinstance(names, list) and names and all(isinstance(n, str) for n in names)):
        raise SpecError("console_name must be a string or a list of strings")
    # `console_names`: further aliases the venues patch, each its own file.
    extra = spec.get("console_names") or []
    if not (isinstance(extra, list) and all(isinstance(n, str) for n in extra)):
        raise SpecError("console_names must be a list of strings")
    for n in extra:
        if n not in names:
            names.append(n)
    return names


def generate(spec_path, base_path, out_dir):
    """Write one .gdtf per console-name alias; returns the paths."""
    with open(spec_path, encoding="utf-8") as f:
        spec = json.load(f)
    outs = []
    for name in console_names(spec):
        per_alias = dict(spec, console_name=name)
        outs.append(generate_one(spec_path, per_alias, base_path, out_dir))
    return outs


def generate_one(spec_path, spec, base_path, out_dir):
    base = Base(base_path)
    gen = Generator(spec, base)
    root = gen.build()
    for w in gen.warnings:
        print("warning: %s: %s" % (Path(spec_path).name, w), file=sys.stderr)
    indent(root)
    xml = b'<?xml version="1.0" encoding="UTF-8" standalone="no" ?>\n' + ET.tostring(root, encoding="utf-8")
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    out = out_dir / output_name(spec)
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("description.xml", xml)
        thumb = base.thumbnail()
        if thumb:
            z.writestr(thumb, base.zip.read(thumb))
        for name in base.model_files():
            z.writestr(name, base.zip.read(name))
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--spec", type=Path, help="spec JSON")
    ap.add_argument("--base", type=Path, help="base .gdtf whose geometry to borrow")
    ap.add_argument("--all", action="store_true", help="every spec in --specs via tools/gdtf_bases.json")
    ap.add_argument("--specs", type=Path, default=DEFAULT_SPECS)
    ap.add_argument("--bases", type=Path, default=DEFAULT_BASES, help="directory holding the base .gdtf files")
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = ap.parse_args(argv)

    jobs = []
    if args.all:
        table = json.loads(BASES_TABLE.read_text(encoding="utf-8"))
        for spec_path in sorted(args.specs.glob("*.json")):
            try:
                names = console_names(json.loads(spec_path.read_text(encoding="utf-8")))
            except (json.JSONDecodeError, SpecError) as e:
                print("skip %s: %s" % (spec_path, e), file=sys.stderr)
                continue
            base_name = next((table[n] for n in names if n in table), None)
            if not base_name:
                print("skip %s: console_name %r not in %s" % (spec_path.name, names, BASES_TABLE.name), file=sys.stderr)
                continue
            jobs.append((spec_path, args.bases / base_name))
    elif args.spec and args.base:
        jobs.append((args.spec, args.base))
    else:
        ap.error("give --spec and --base, or --all")

    failed = 0
    for spec_path, base_path in jobs:
        try:
            for out in generate(spec_path, base_path, args.out):
                print("wrote %s" % out)
        except (SpecError, KeyError, ValueError, OSError, zipfile.BadZipFile) as e:
            failed += 1
            print("FAILED %s: %s" % (spec_path, e), file=sys.stderr)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
