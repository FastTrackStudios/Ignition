"""Build the Lab 138 room model (CBU Room 138) for the visualizer.

    blender -b --python tools/build_lab138_room.py -- --out <model dir>

Writes `<model dir>/lab138-room.glb` (plus the generated textures beside
it, for inspection) and `data/venues/room138-cbu/environment.json`, the
record of every object and practical light in it. Edit this script and
re-run; neither output is hand-made.

Everything here is modelled clean from measurements. The measurements
were taken off a phone LiDAR scan of the room (metric) and sixteen
photographs; neither is read by this script and neither is in the
repository. Each constant says where it came from and how sure it is.
The textures are procedural (numpy, below), with base colours taken from
photo patches under the house lights as ratios to the wall, so none of
the photographed coloured light is baked in.

Coordinates are the venue's: metres, x = stage right(-)/left(+),
y = downstage(-)/upstage(+), z = up, the band wall at y = +1.8288 and the
room centred on x = 0 (docs/domain/norco-venue-reference.md). The scan's
own frame maps onto it by `S()` below.
"""
import json
import math
import sys
from pathlib import Path

import bpy
import numpy as np
from mathutils import Matrix, Vector

REPO = Path(__file__).resolve().parent.parent
VENUE = REPO / "data" / "venues" / "room138-cbu"
argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
OUT = Path(argv[argv.index("--out") + 1]) if "--out" in argv else REPO / "target" / "lab138-model"
OUT.mkdir(parents=True, exist_ok=True)
(OUT / "textures").mkdir(exist_ok=True)

# ── Measured: the room ───────────────────────────────────────────────
# Wall planes are the peaks of the scan's wall-face histograms (5 cm
# bins): the two long walls 10.12 m apart, the two end walls 14.00 m.
# LiDAR scale is good to about 1 %, and the wall peaks are 5-10 cm wide,
# so each is +/- 0.1 m.
Y_BAND = 1.8288                 # the curtain wall (scan x = +5.15); kept where the rig has it
DEPTH = 10.12                   # band wall to house-back wall
Y_HOUSE = Y_BAND - DEPTH        # scan x = -4.97
X_SR, X_SL = -7.0, 7.0          # lounge end (scan y = +5.95), console end (scan y = -8.05)
# Flat painted soffits at both ends and the corridor: the scan's largest
# ceiling planes, 3.45-3.50 m. +/- 0.05.
SOFFIT_Z = 3.5
X_SOFFIT_SR, X_SOFFIT_SL = -5.15, 5.05   # where the tile ceiling starts (scan y = +4.1, -6.1)
# The tile ceiling is a four-sided pyramid over the 10.2 x 10.1 m between
# the soffits, rising from 3.5 m at its edges to a point. Apex height is
# the scan's highest ceiling vertex; +/- 0.15 (the scan thins out there).
APEX = (-0.05, -3.23, 5.45)
# The corridor off the house-back stage-right corner, to a double exit door.
X_CORRIDOR_END = -9.85          # scan y = +8.8
Y_CORRIDOR_IN = -6.27           # scan x = -2.95
# The curtain: track at 3.12 m, full width, one gap.
CURTAIN_TOP = 3.12
CURTAIN_GAP = (-0.30, 0.05)     # scan y = -0.75 .. -1.10
# The pendant rectangle of linear fixtures, hung below the tiles.
LINEAR = (-2.55, 2.80, -5.62, -0.37, 3.78)   # x0, x1, y0, y1, z


def S(xs, ys):
    """A point in the scan's frame, in the venue's."""
    return (-(ys + 1.05), xs - 3.3212)


def pyramid_z(x, y):
    """The tile ceiling's height over a floor point."""
    ax, ay, az = APEX
    fx = (x - ax) / ((X_SOFFIT_SL - ax) if x > ax else (ax - X_SOFFIT_SR))
    fy = (y - ay) / ((Y_BAND - ay) if y > ay else (ay - Y_HOUSE))
    return az - (az - SOFFIT_Z) * max(abs(fx), abs(fy))


# ── Materials ────────────────────────────────────────────────────────
# Base colours are LINEAR reflectance. Where a photo was the source, the
# patch was linearised and divided by the wall's in the same frame, then
# scaled by the wall's assumed reflectance (0.75, an off-white matte
# paint): the wall is the room's grey card. See docs/lighting/lab-138-room.md.
def lin_to_srgb(c):
    c = np.clip(np.asarray(c, dtype=np.float64), 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(c, 1 / 2.4) - 0.055)


MATERIALS = {
    # name: (linear rgb, roughness, metallic, where the numbers came from)
    "Wall Paint": ((0.77, 0.75, 0.66), 0.90, 0.0,
                   "photo 5783: wall under house light, chroma kept, luminance set to 0.75 (matte off-white paint)"),
    "Soffit Paint": ((0.68, 0.65, 0.58), 0.90, 0.0,
                     "photo 5784: the soffits read a shade darker than the walls, in their own shadow; +/- 0.1"),
    "Ceiling Tile": ((0.82, 0.82, 0.80), 0.95, 0.0,
                     "mineral-fibre tile, manufacturer light reflectance 0.80-0.85; procedural texture"),
    "Floor Wood": ((1.0, 1.0, 1.0), 0.42, 0.0,
                   "photo 5783: floor/wall luminance 0.25 -> 0.185; semi-gloss laminate; procedural plank texture"),
    "Base Trim": ((0.072, 0.038, 0.026), 0.50, 0.0, "photo 5783: trim/wall 0.06"),
    "Curtain Velvet": ((0.016, 0.026, 0.075), 1.00, 0.0,
                       "photos 5797 and 5790: curtain/wall-above-it 0.024-0.07 -> reflectance about 0.03, navy"),
    "Door Wood": ((0.45, 0.166, 0.032), 0.45, 0.0, "photo 5795: door/wall 0.29"),
    "Frame Black": ((0.02, 0.02, 0.02), 0.50, 0.0, "door frames, stands, speaker cabinets: judged"),
    "Piano Black": ((0.008, 0.008, 0.009), 0.06, 0.0, "polyester piano finish: near-mirror"),
    "Piano Keys": ((0.80, 0.80, 0.76), 0.30, 0.0, "judged"),
    "Diffuser Cells": ((1.0, 1.0, 1.0), 0.70, 0.0, "photo 5791 for the hue; procedural cell texture"),
    "Diffuser Frame": ((0.30, 0.11, 0.03), 0.45, 0.0, "photo 5782: frame/wall 0.28 under mixed light; +/- 0.1"),
    "Glass": ((0.60, 0.65, 0.65), 0.05, 0.0, "the booth panels' glass tops: judged"),
    "Table White": ((0.80, 0.80, 0.78), 0.55, 0.0, "photo 5782: folding table top"),
    "Chair Fabric": ((0.017, 0.020, 0.078), 0.95, 0.0, "photo 5783: chair/wall 0.03"),
    "Leather Tan": ((0.20, 0.09, 0.04), 0.50, 0.0, "photo 5788: couch; the patch was in shade, lifted to a tan leather"),
    "Woven Tan": ((0.45, 0.36, 0.22), 0.90, 0.0, "photo 5788: armchair and beanbag"),
    "Rug": ((1.0, 1.0, 1.0), 0.95, 0.0, "photo 5788: rug/wall 0.79 -> 0.55; procedural weave"),
    "Rug Dark": ((0.10, 0.09, 0.08), 0.95, 0.0, "the mat under the tables and the drum mat: judged from the scan"),
    "Metal Grey": ((0.35, 0.35, 0.36), 0.40, 0.8, "stools, table legs: judged"),
    "Console Grey": ((0.22, 0.23, 0.25), 0.45, 0.3, "console surface: judged"),
    "Walnut": ((0.16, 0.07, 0.03), 0.45, 0.0, "console and coffee-table wood: judged"),
    "Screen": ((0.01, 0.01, 0.012), 0.10, 0.0, "TV glass, off: nothing of what was on them is kept"),
    "Whiteboard": ((0.85, 0.85, 0.85), 0.15, 0.0, "blank"),
    "Keyboard Red": ((0.45, 0.02, 0.02), 0.40, 0.0, "the red stage keyboard"),
    "Drum Skin": ((0.70, 0.62, 0.42), 0.50, 0.0, "bongo shells"),
    "Plant Green": ((0.05, 0.14, 0.04), 0.60, 0.0, "judged"),
    "Fixture White": ((0.80, 0.80, 0.80), 0.50, 0.0, "the linear fixtures' and LED panels' bodies"),
    "Vent": ((0.55, 0.55, 0.55), 0.60, 0.0, "air grilles: white louvres, read as mid grey"),
    "Glow Linear": ((1.0, 1.0, 1.0), 0.50, 0.0, "the linear fixtures' lit lens, 4000 K"),
    "Glow Can": ((1.0, 1.0, 1.0), 0.50, 0.0, "a recessed can's lit aperture, 3000 K"),
    "Glow Exit": ((0.60, 0.02, 0.02), 0.50, 0.0, "exit signs: a blank red face, no lettering"),
}
# Emission in nits, by material. The lens of a 4500 lm, 1.2 m linear LED is
# several thousand nits; these read as lit at the stage exposure without
# blowing the whole frame to white through bloom.
EMISSION = {"Glow Linear": ((1.0, 0.93, 0.84), 2500.0), "Glow Can": ((1.0, 0.82, 0.62), 2500.0),
            "Glow Exit": ((1.0, 0.05, 0.03), 40.0)}
DOUBLE_SIDED = {"Curtain Velvet", "Plant Green", "Glass", "Frame Black"}

rng = np.random.default_rng(138)


def smooth_noise(h, w, cells_y, cells_x):
    """Tileable value noise: a random grid, wrapped and cubic-smoothed."""
    g = rng.random((cells_y, cells_x))
    ys = np.arange(h) / h * cells_y
    xs = np.arange(w) / w * cells_x
    y0, x0 = np.floor(ys).astype(int), np.floor(xs).astype(int)
    fy, fx = ys - y0, xs - x0
    fy, fx = fy * fy * (3 - 2 * fy), fx * fx * (3 - 2 * fx)
    y1, x1 = (y0 + 1) % cells_y, (x0 + 1) % cells_x
    a = g[y0][:, x0] * (1 - fx) + g[y0][:, x1] * fx
    b = g[y1][:, x0] * (1 - fx) + g[y1][:, x1] * fx
    return a * (1 - fy)[:, None] + b * fy[:, None]


def tex_floor():
    """Dark wood planks, 0.15 x 1.2 m, staggered: one tile is 2.4 x 2.4 m.

    Mean linear colour (0.26, 0.17, 0.115), luminance 0.185.
    """
    w, h, rows, per = 1024, 1024, 16, 2
    base = np.array([0.26, 0.17, 0.115])
    img = np.zeros((h, w, 3))
    ph = h // rows
    for r in range(rows):
        shift = int(rng.integers(0, w))
        for p in range(per):
            x0 = (shift + p * w // per) % w
            tone = 0.60 + 0.80 * rng.random()
            warm = 0.92 + 0.16 * rng.random()
            cols = [(x0 + i) % w for i in range(w // per)]
            img[r * ph:(r + 1) * ph, cols] = base * tone * np.array([warm, 1.0, 2.0 - warm])
            img[r * ph:(r + 1) * ph, [x0, (x0 + 1) % w]] *= 0.55      # butt joint
        img[r * ph:r * ph + 2] *= 0.55                                 # long seam
    grain = smooth_noise(h, w, 256, 16) * 0.5 + smooth_noise(h, w, 512, 8) * 0.5
    img *= (0.80 + 0.40 * grain)[:, :, None]
    return img * (base.mean() / img.mean(axis=(0, 1)).mean())


def tex_ceiling():
    """One 0.61 m ceiling tile: fissured white with the grid's shadow line."""
    n = 256
    v = 0.80 + 0.06 * smooth_noise(n, n, 64, 64) + 0.04 * smooth_noise(n, n, 16, 16)
    pits = rng.random((n, n)) > 0.985
    v[pits] *= 0.75
    v[:4] = v[-4:] = 0.58
    v[:, :4] = v[:, -4:] = 0.58
    return np.stack([v, v, v * 0.975], axis=2)


def tex_diffuser():
    """A skyline diffuser's face: 8 x 8 wooden cells of differing depth."""
    n, cells = 256, 8
    base = np.array([0.55, 0.36, 0.14])
    img = np.zeros((n, n, 3))
    c = n // cells
    for i in range(cells):
        for j in range(cells):
            depth = rng.random()
            img[i * c:(i + 1) * c, j * c:(j + 1) * c] = base * (0.35 + 0.65 * depth)
            img[i * c:i * c + 3, j * c:(j + 1) * c] = base * 0.9
            img[i * c:(i + 1) * c, j * c:j * c + 3] = base * 0.9
    return img


def tex_rug():
    """A flat woven rug: pale jute, mean linear (0.60, 0.50, 0.31)."""
    n = 256
    base = np.array([0.60, 0.50, 0.31])
    yy, xx = np.mgrid[0:n, 0:n]
    weave = 0.88 + 0.12 * (((yy // 4) + (xx // 4)) % 2) + 0.10 * (smooth_noise(n, n, 32, 32) - 0.5)
    return base * weave[:, :, None]


def save_texture(name, lin):
    """A linear float image as an sRGB PNG, and the Blender image for it."""
    h, w, _ = lin.shape
    rgba = np.concatenate([lin_to_srgb(lin), np.ones((h, w, 1))], axis=2)[::-1]
    image = bpy.data.images.new(name, w, h, alpha=False)
    image.pixels.foreach_set(rgba.astype(np.float32).ravel())
    image.filepath_raw = str(OUT / "textures" / f"{name}.png")
    image.file_format = "PNG"
    image.save()
    image.pack()
    return image


def make_material(name, image=None):
    color, rough, metal, _ = MATERIALS[name]
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    bsdf = m.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*color, 1.0)
    bsdf.inputs["Roughness"].default_value = rough
    bsdf.inputs["Metallic"].default_value = metal
    if image is not None:
        node = m.node_tree.nodes.new("ShaderNodeTexImage")
        node.image = image
        m.node_tree.links.new(node.outputs["Color"], bsdf.inputs["Base Color"])
    if name in EMISSION:
        tint, nits = EMISSION[name]
        bsdf.inputs["Emission Color"].default_value = (*tint, 1.0)
        bsdf.inputs["Emission Strength"].default_value = nits
    if name == "Glass":
        bsdf.inputs["Alpha"].default_value = 0.18
        m.surface_render_method = "BLENDED"
    m.use_backface_culling = name not in DOUBLE_SIDED
    return m


# ── Mesh building ────────────────────────────────────────────────────
class Builder:
    """Faces for one object, in the object's own frame (origin on the floor)."""

    def __init__(self):
        self.verts, self.faces, self.uvs, self.mats = [], [], [], []

    def face(self, pts, mat, uvs=None, toward=None):
        pts = [Vector(p) for p in pts]
        if toward is not None:
            n = (pts[1] - pts[0]).cross(pts[2] - pts[0])
            centre = sum(pts, Vector()) / len(pts)
            if n.dot(Vector(toward) - centre) < 0:
                pts.reverse()
                uvs = list(reversed(uvs)) if uvs else None
        if uvs is None:
            # Planar, in metres, along the face's own two axes.
            n = (pts[1] - pts[0]).cross(pts[2] - pts[0]).normalized()
            u = Vector((0, 0, 1)).cross(n)
            u = u.normalized() if u.length > 1e-4 else Vector((1, 0, 0))
            v = n.cross(u)
            uvs = [(p.dot(u), p.dot(v)) for p in pts]
        base = len(self.verts)
        self.verts.extend(tuple(p) for p in pts)
        self.faces.append(tuple(range(base, base + len(pts))))
        self.uvs.append(uvs)
        self.mats.append(mat)

    def box(self, centre, size, mat, m=None, mats=None):
        """A box; `mats` overrides the material of named sides (+x -x +y -y +z -z)."""
        cx, cy, cz = centre
        hx, hy, hz = size[0] / 2, size[1] / 2, size[2] / 2
        m = m or Matrix.Identity(4)
        c = [m @ Vector((cx + sx * hx, cy + sy * hy, cz + sz * hz))
             for sx in (-1, 1) for sy in (-1, 1) for sz in (-1, 1)]
        sides = {"-x": (0, 1, 3, 2), "+x": (4, 6, 7, 5), "-y": (0, 4, 5, 1),
                 "+y": (2, 3, 7, 6), "-z": (0, 2, 6, 4), "+z": (1, 5, 7, 3)}
        for side, idx in sides.items():
            self.face([c[i] for i in idx], (mats or {}).get(side, mat))

    def prism(self, outline, z0, z1, mat, m=None):
        """An outline in plan (counter-clockwise), extruded from z0 to z1."""
        m = m or Matrix.Identity(4)
        lo = [m @ Vector((x, y, z0)) for x, y in outline]
        hi = [m @ Vector((x, y, z1)) for x, y in outline]
        self.face(hi, mat)
        self.face(list(reversed(lo)), mat)
        for i in range(len(outline)):
            j = (i + 1) % len(outline)
            self.face([lo[i], lo[j], hi[j], hi[i]], mat)

    def cyl(self, centre, radius, height, mat, n=10, top=None, m=None):
        """An upright cylinder (or cone frustum, with `top` radius), base at `centre`."""
        cx, cy, cz = centre
        top = radius if top is None else top
        ring = lambda r, z: [(cx + r * math.cos(2 * math.pi * i / n),
                              cy + r * math.sin(2 * math.pi * i / n), z) for i in range(n)]
        m = m or Matrix.Identity(4)
        lo = [m @ Vector(p) for p in ring(radius, cz)]
        hi = [m @ Vector(p) for p in ring(top, cz + height)]
        self.face(hi, mat)
        self.face(list(reversed(lo)), mat)
        for i in range(n):
            j = (i + 1) % n
            self.face([lo[i], lo[j], hi[j], hi[i]], mat)

    def strut(self, a, b, thick, mat):
        """A square bar from a to b."""
        a, b = Vector(a), Vector(b)
        axis = b - a
        rot = Vector((0, 0, 1)).rotation_difference(axis.normalized()).to_matrix().to_4x4()
        m = Matrix.Translation(a) @ rot
        self.box((0, 0, axis.length / 2), (thick, thick, axis.length), mat, m=m)


materials = {}
objects = []          # the environment.json records
lights = []
triangles = 0


def emit(name, kind, builder, position=(0, 0, 0), yaw=0.0, movable=True, group=None,
         smooth=False, note=None):
    """Turn a Builder into a named Blender object and an environment record."""
    global triangles
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(builder.verts, [], builder.faces)
    used = []
    for mat in builder.mats:
        if mat not in used:
            used.append(mat)
    for mat in used:
        mesh.materials.append(materials[mat])
    uv = mesh.uv_layers.new(name="UVMap")
    for poly, face_uvs, mat in zip(mesh.polygons, builder.uvs, builder.mats):
        poly.material_index = used.index(mat)
        poly.use_smooth = smooth
        for loop, coord in zip(poly.loop_indices, face_uvs):
            uv.data[loop].uv = coord
        triangles += len(poly.vertices) - 2
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    obj.location = position
    obj.rotation_euler = (0, 0, math.radians(yaw))
    bpy.context.scene.collection.objects.link(obj)
    v = np.array(builder.verts)
    size = v.max(axis=0) - v.min(axis=0)
    record = {
        "name": name, "kind": kind,
        "position": {"x": round(position[0], 3), "y": round(position[1], 3), "z": round(position[2], 3)},
        "eulers": {"x": 0, "y": 0, "z": round(yaw, 2)},
        "size": {"x": round(float(size[0]), 3), "y": round(float(size[1]), 3), "z": round(float(size[2]), 3)},
        "visible": True, "movable": movable,
    }
    if group:
        record["group"] = group
    if note:
        record["note"] = note
    objects.append(record)
    return obj


def rot_z(deg):
    return Matrix.Rotation(math.radians(deg), 4, "Z")


def rot_x(deg):
    return Matrix.Rotation(math.radians(deg), 4, "X")


# ── Architecture ─────────────────────────────────────────────────────
def build_architecture():
    inside = (0.0, -3.0, 1.5)
    corridor = ((X_SR + X_CORRIDOR_END) / 2, (Y_HOUSE + Y_CORRIDOR_IN) / 2, 1.5)

    floor = Builder()
    uv = lambda pts: [(p[0] / 2.4, p[1] / 2.4) for p in pts]
    for x0, x1, y0, y1 in [(X_SR, X_SL, Y_HOUSE, Y_BAND), (X_CORRIDOR_END, X_SR, Y_HOUSE, Y_CORRIDOR_IN)]:
        pts = [(x0, y0, 0), (x1, y0, 0), (x1, y1, 0), (x0, y1, 0)]
        floor.face(pts, "Floor Wood", uvs=uv(pts))
    emit("Floor", "architecture", floor, movable=False)

    walls = Builder()
    plan = [((X_SR, Y_BAND), (X_SL, Y_BAND), inside), ((X_SL, Y_BAND), (X_SL, Y_HOUSE), inside),
            ((X_SL, Y_HOUSE), (X_CORRIDOR_END, Y_HOUSE), inside),
            ((X_CORRIDOR_END, Y_HOUSE), (X_CORRIDOR_END, Y_CORRIDOR_IN), corridor),
            ((X_CORRIDOR_END, Y_CORRIDOR_IN), (X_SR, Y_CORRIDOR_IN), corridor),
            ((X_SR, Y_CORRIDOR_IN), (X_SR, Y_BAND), inside)]
    for (ax, ay), (bx, by), toward in plan:
        walls.face([(ax, ay, 0), (bx, by, 0), (bx, by, SOFFIT_Z), (ax, ay, SOFFIT_Z)],
                   "Wall Paint", toward=toward)
    emit("Walls", "architecture", walls, movable=False)

    trim = Builder()
    for (ax, ay), (bx, by), toward in plan[1:]:           # not the band wall: the curtain covers it
        a, b = Vector((ax, ay, 0)), Vector((bx, by, 0))
        along = (b - a).normalized()
        into = Vector((0, 0, 1)).cross(along)
        if into.dot(Vector(toward) - a) < 0:
            into = -into
        mid = (a + b) / 2 + into * 0.008
        yaw = math.degrees(math.atan2(along.y, along.x))
        trim.box((0, 0, 0.05), ((b - a).length, 0.016, 0.10), "Base Trim",
                 m=Matrix.Translation(mid) @ rot_z(yaw))
    emit("Base Trim", "architecture", trim, movable=False)

    ceiling = Builder()
    for x0, x1, y0, y1 in [(X_SOFFIT_SL, X_SL, Y_HOUSE, Y_BAND), (X_SR, X_SOFFIT_SR, Y_HOUSE, Y_BAND),
                           (X_CORRIDOR_END, X_SR, Y_HOUSE, Y_CORRIDOR_IN)]:
        ceiling.face([(x0, y0, SOFFIT_Z), (x1, y0, SOFFIT_Z), (x1, y1, SOFFIT_Z), (x0, y1, SOFFIT_Z)],
                     "Soffit Paint", toward=(0, 0, 0))
    # The tiles: 0.61 m, laid on the diagonal, so the UVs are the plan
    # rotated 45 degrees and scaled to one tile per unit.
    r = 1 / (math.sqrt(2) * 0.61)
    tile_uv = lambda p: ((p[0] + p[1]) * r, (p[0] - p[1]) * r)
    corners = [(X_SOFFIT_SR, Y_HOUSE, SOFFIT_Z), (X_SOFFIT_SL, Y_HOUSE, SOFFIT_Z),
               (X_SOFFIT_SL, Y_BAND, SOFFIT_Z), (X_SOFFIT_SR, Y_BAND, SOFFIT_Z)]
    for i in range(4):
        pts = [corners[i], corners[(i + 1) % 4], APEX]
        ceiling.face(pts, "Ceiling Tile", uvs=[tile_uv(p) for p in pts], toward=(0, -3, 0))
    emit("Ceiling", "architecture", ceiling, movable=False)

    # The curtain: two drapes of pinch-pleated velvet, 12 cm off the wall.
    for name, x0, x1 in [("Curtain - Stage Right", X_SR + 0.05, CURTAIN_GAP[0]),
                         ("Curtain - Stage Left", CURTAIN_GAP[1], X_SL - 0.15)]:
        drape = Builder()
        pleat, steps = 0.17, 10
        n = int((x1 - x0) / pleat) * steps
        prof = []
        for i in range(n + 1):
            t = i / steps
            # A rounded fold with a flattened back, 9 cm deep.
            s = math.sin(2 * math.pi * t)
            prof.append((x0 + (x1 - x0) * i / n, -0.045 * (s + 0.25 * math.sin(4 * math.pi * t))))
        for i in range(n):
            (xa, da), (xb, db) = prof[i], prof[i + 1]
            pts = [(xa, da, 0.02), (xb, db, 0.02), (xb, db, CURTAIN_TOP), (xa, da, CURTAIN_TOP)]
            drape.face(pts, "Curtain Velvet", uvs=[(xa, 0), (xb, 0), (xb, CURTAIN_TOP), (xa, CURTAIN_TOP)])
        emit(name, "architecture", drape, position=(0, Y_BAND - 0.12, 0), movable=True, smooth=True)
    track = Builder()
    track.box((0, 0, CURTAIN_TOP + 0.03), (X_SL - X_SR - 0.1, 0.05, 0.06), "Fixture White")
    emit("Curtain Track", "architecture", track, position=(0, Y_BAND - 0.12, 0), movable=False)


def door(name, position, yaw, width, height, leaves):
    b = Builder()
    f = 0.06
    b.box((0, 0.02, height / 2), (width + 2 * f, 0.04, height), "Frame Black")
    b.box((0, 0.02, height + f / 2), (width + 2 * f, 0.04, f), "Frame Black")
    leaf = width / leaves
    for i in range(leaves):
        x = -width / 2 + leaf * (i + 0.5)
        b.box((x, 0.045, height / 2), (leaf - 0.012, 0.03, height - 0.01), "Door Wood")
        side = 1 if (leaves == 1 or i == 0) else -1
        b.box((x + side * (leaf / 2 - 0.1), 0.075, 1.0), (0.03, 0.03, 0.14), "Metal Grey")
    emit(name, "architecture", b, position=position, yaw=yaw, movable=False)


def exit_sign(name, position, yaw):
    b = Builder()
    b.box((0, 0.03, 0), (0.33, 0.05, 0.20), "Fixture White", mats={"+y": "Glow Exit"})
    emit(name, "practical", b, position=position, yaw=yaw, movable=False)


def build_doors():
    # House-back wall (faces +y). Positions off the scan's wall elevation, +/- 0.05.
    door("Door - Double", (2.65, Y_HOUSE, 0), 0, 1.90, 2.15, 2)
    door("Door - Single", (6.05, Y_HOUSE, 0), 0, 0.92, 2.15, 1)
    door("Door - Corridor Exit", (X_CORRIDOR_END, (Y_HOUSE + Y_CORRIDOR_IN) / 2, 0), -90, 1.80, 2.10, 2)
    exit_sign("Exit Sign - Single Door", (6.05, Y_HOUSE, 3.05), 0)
    exit_sign("Exit Sign - Corridor", (X_CORRIDOR_END, (Y_HOUSE + Y_CORRIDOR_IN) / 2, 2.45), -90)


# ── Practical lights ─────────────────────────────────────────────────
# The visualizer's camera has no white balance: white is D65. The
# photographs were balanced on the house lights, which is also what an eye
# in the room does, so the practicals are tinted relative to this white
# rather than to daylight. At D65 the 4000 K linear light would render
# orange (linear 1.0, 0.64, 0.37) and every comparison would be of the
# colour cast.
WHITE_BALANCE_K = 4000


def kelvin_rgb(k):
    """A source's tint as a camera balanced for WHITE_BALANCE_K sees it, linear."""
    source, white = blackbody(k), blackbody(WHITE_BALANCE_K)
    tint = [s / w for s, w in zip(source, white)]
    return [round(c / max(tint), 4) for c in tint]


def blackbody(k):
    """Blackbody colour, linear sRGB (Tanner Helland's fit)."""
    t = k / 100.0
    r = 255.0 if t <= 66 else 329.698727446 * (t - 60) ** -0.1332047592
    g = 99.4708025861 * math.log(t) - 161.1195681661 if t <= 66 else 288.1221695283 * (t - 60) ** -0.0755148492
    b = 255.0 if t >= 66 else (0.0 if t <= 19 else 138.5177312231 * math.log(t - 10) - 305.0447927307)
    srgb = np.clip(np.array([r, g, b]) / 255.0, 0, 1)
    linear = np.where(srgb <= 0.04045, srgb / 12.92, ((srgb + 0.055) / 1.055) ** 2.4)
    return [float(c) for c in linear / linear.max()]


def light(name, group, kind, position, lumens, color, aim=None, cone_deg=None, range_m=14.0,
          shadows=False, note=None, radius=0.05):
    rec = {"name": name, "group": group, "kind": kind,
           "position": {"x": round(position[0], 3), "y": round(position[1], 3), "z": round(position[2], 3)},
           "lumens": lumens, "color": color, "range": range_m, "radius": radius, "shadows": shadows}
    if aim is not None:
        rec["aim"] = {"x": round(aim[0], 3), "y": round(aim[1], 3), "z": round(aim[2], 3)}
        rec["cone_deg"] = cone_deg
    if note:
        rec["note"] = note
    lights.append(rec)


# Recessed cans, from the scan's ceiling (bright discs in the soffits), +/- 0.1.
CANS = [S(-3.15, -7.2), S(0.12, -7.15), S(3.45, -7.1),          # console soffit
        S(-3.25, 4.8), S(0.0, 5.0), S(3.3, 4.95),               # lounge soffit
        S(-4.0, 6.3), S(-3.9, 7.6)]                             # corridor
SOFFIT_VENTS = [S(4.2, -7.1), S(3.9, 4.95), S(-3.9, 8.3)]
TILE_VENTS = [S(-1.6, -4.7), S(-3.5, -2.8), S(-3.5, 1.4), S(-1.6, 2.3), S(1.8, 2.4)]


def build_practicals():
    x0, x1, y0, y1, z = LINEAR
    body, glow = Builder(), Builder()
    runs = [((x0 + x1) / 2, y0, x1 - x0, 0), ((x0 + x1) / 2, y1, x1 - x0, 0),
            (x0, (y0 + y1) / 2, y1 - y0, 90), (x1, (y0 + y1) / 2, y1 - y0, 90)]
    for cx, cy, length, yaw in runs:
        m = Matrix.Translation((cx, cy, 0)) @ rot_z(yaw)
        body.box((0, 0, z + 0.04), (length + 0.1, 0.10, 0.08), "Fixture White", m=m)
        glow.box((0, 0, z - 0.004), (length, 0.085, 0.006), "Glow Linear", m=m)
    # Pendant stems up to the tiles, at the corners and mid-runs.
    for sx, sy in [(x0, y0), (x1, y0), (x1, y1), (x0, y1), ((x0 + x1) / 2, y0), ((x0 + x1) / 2, y1),
                   (x0, (y0 + y1) / 2), (x1, (y0 + y1) / 2)]:
        body.strut((sx, sy, z + 0.08), (sx, sy, pyramid_z(sx, sy)), 0.012, "Fixture White")
    emit("Linear Fixtures", "practical", body, movable=False)
    emit("Linear Fixtures - Lit", "practical", glow, movable=False, group="house")

    # The linear rectangle is 21.2 m of direct/indirect LED: 17 four-foot
    # sections at a nominal 4500 lm is 76 klm, which is 540 lm/m2 installed
    # over the 141 m2 room -- about 350 lux on the tables once the ceiling
    # and walls have taken their share. 4000 K: the photos' linear light is
    # neutral beside the visibly warmer cans.
    perimeter = 2 * ((x1 - x0) + (y1 - y0))
    count = 16
    each = round(76000 / count)
    for i in range(count):
        d = perimeter * (i + 0.5) / count
        if d < x1 - x0:
            p = (x0 + d, y0)
        elif d < (x1 - x0) + (y1 - y0):
            p = (x1, y0 + d - (x1 - x0))
        elif d < 2 * (x1 - x0) + (y1 - y0):
            p = (x1 - (d - (x1 - x0) - (y1 - y0)), y1)
        else:
            p = (x0, y1 - (d - 2 * (x1 - x0) - (y1 - y0)))
        light(f"Linear {i + 1}", "house", "point", (p[0], p[1], z - 0.12), each, kelvin_rgb(4000))

    # The visualizer lights directly: nothing bounces. In the room the floor
    # and walls return about a fifth of this light upward, which is the only
    # thing that lights the soffits' undersides and the underside of a
    # table. These stand in for that: shadowless points three metres UNDER
    # the floor, which the floor's upward face ignores and everything that
    # faces down receives evenly. In the room they made a hot spot on
    # whatever stood nearest. Not fixtures; they switch with the house.
    for i, (bx, by) in enumerate([(x, y) for x in (-5.0, 0.0, 5.0) for y in (-6.0, -1.0)], start=1):
        light(f"Bounce {i}", "house", "point", (bx, by, -3.0), 9000, [1.0, 0.86, 0.72], range_m=14.0,
              note="fill standing in for bounced light, which the renderer does not compute")

    cans, cans_lit = Builder(), Builder()
    for i, (cx, cy) in enumerate(CANS, start=1):
        cans.cyl((cx, cy, SOFFIT_Z - 0.012), 0.085, 0.012, "Frame Black", n=12)
        cans_lit.cyl((cx, cy, SOFFIT_Z - 0.016), 0.065, 0.004, "Glow Can", n=12)
        # A 6-inch LED downlight: 900 lm, 3000 K, a 60-degree flood.
        light(f"Can {i}", "house", "spot", (cx, cy, SOFFIT_Z - 0.05), 900, kelvin_rgb(3000),
              aim=(cx, cy, 0.0), cone_deg=60.0, range_m=6.0)
    emit("Recessed Cans", "practical", cans, movable=False)
    emit("Recessed Cans - Lit", "practical", cans_lit, movable=False, group="house")

    vents = Builder()
    for cx, cy in SOFFIT_VENTS:
        vents.box((cx, cy, SOFFIT_Z - 0.008), (0.55, 0.55, 0.016), "Vent")
    for cx, cy in TILE_VENTS:
        # On the slope, in the tiles' diamond: tipped to the ceiling's own normal.
        e = 0.05
        normal = Vector(((pyramid_z(cx - e, cy) - pyramid_z(cx + e, cy)) / (2 * e),
                         (pyramid_z(cx, cy - e) - pyramid_z(cx, cy + e)) / (2 * e), 1.0)).normalized()
        tip = Vector((0, 0, 1)).rotation_difference(normal).to_matrix().to_4x4()
        vents.box((0, 0, 0), (0.55, 0.55, 0.016), "Vent",
                  m=Matrix.Translation((cx, cy, pyramid_z(cx, cy) - 0.02)) @ tip @ rot_z(45))
    emit("Air Vents", "architecture", vents, movable=False)

    proj = Builder()
    px, py = S(0.1, -6.0)
    proj.box((0, 0, -0.30), (0.36, 0.42, 0.14), "Fixture White")
    proj.strut((0, 0, -0.23), (0, 0, 0), 0.03, "Frame Black")
    emit("Projector", "gear", proj, position=(px, py, pyramid_z(px, py)), movable=True)


# ── As photographed: the stage lights that were in the room ──────────
def panel_light(name, position, aim, height, color, lumens, note):
    """An LED panel with barn doors on a light stand, aimed at `aim`."""
    b = Builder()
    for k in range(3):
        a = math.radians(120 * k + 30)
        b.strut((0, 0, 0.42), (0.38 * math.cos(a), 0.38 * math.sin(a), 0.02), 0.018, "Frame Black")
    b.strut((0, 0, 0.02), (0, 0, height - 0.16), 0.022, "Frame Black")
    head = Vector((0, 0, height))
    d = (Vector(aim) - Vector(position) - head).normalized()
    m = Matrix.Translation(head) @ Vector((0, 1, 0)).rotation_difference(d).to_matrix().to_4x4()
    b.box((0, 0, 0), (0.36, 0.08, 0.26), "Fixture White", m=m)
    for sx, sz, w, h in [(-0.24, 0, 0.14, 0.26), (0.24, 0, 0.14, 0.26), (0, 0.19, 0.36, 0.14)]:
        b.box((sx, 0.10, sz), (w, 0.004, h), "Frame Black", m=m)
    b.strut((-0.19, 0, height - 0.16), (0.19, 0, height - 0.16), 0.014, "Fixture White")
    for sx in (-0.19, 0.19):
        b.strut((sx, 0, height - 0.16), (sx, 0, height), 0.014, "Fixture White")
    emit(name, "found-light", b, position=position, note=note)
    p = Vector(position) + head + d * 0.06
    light(name, "found", "spot", tuple(p), lumens, color, aim=aim, cone_deg=110.0, range_m=10.0,
          shadows=True, note=note)


def uplight(name, position, color, lumens, note):
    b = Builder()
    b.cyl((0, 0, 0), 0.075, 0.10, "Frame Black", n=10)
    emit(name, "found-light", b, position=position, note=note)
    x, y, _ = position
    light(name, "found", "spot", (x, y, 0.11), lumens, color, aim=(x, y + 0.35, 2.5), cone_deg=40.0,
          range_m=6.0, note=note)


BLUE, MAGENTA, TEAL = [0.02, 0.10, 1.0], [0.75, 0.10, 1.0], [0.0, 0.85, 0.65]
# The panels' output is not known (no model is legible). The renderer takes
# `lumens x colour` as the power of each channel, so this is the flux of a
# white source with the same blue: set by eye against the photographs.
PANEL_LM = 30000


def build_found_lights():
    seen = "photographed; position from the scan's curtain elevation, +/- 0.2 m"
    panel_light("Panel - Curtain Blue", (-3.75, Y_BAND - 0.67, 0), (-3.75, Y_BAND - 0.12, 2.2), 1.50,
                BLUE, PANEL_LM, seen)
    panel_light("Panel - Curtain Magenta", (1.25, Y_BAND - 0.67, 0), (1.25, Y_BAND - 0.12, 2.2), 1.50,
                MAGENTA, PANEL_LM, seen)
    lx, ly = S(-2.75, 5.45)
    panel_light("Panel - Lounge Magenta", (lx + 0.1, ly, 0), (X_SR, ly + 0.25, 2.3), 1.45, MAGENTA, PANEL_LM,
                "photographed beside the plant, washing the stage-right end wall; +/- 0.2 m")
    for i, xs in enumerate([-3.5, -1.9, 2.9], start=1):
        uplight(f"Uplight - Curtain {i}", (xs, Y_BAND - 0.32, 0), TEAL, 300,
                "small floor uplight at the curtain: its teal lens is in the scan; +/- 0.2 m")
    # The blue wash on the console-end wall and soffit. Its source is hidden
    # behind the console's screen in every photo; this is a guess at it.
    wx, wy = S(-0.6, -7.05)
    b = Builder()
    b.box((0, 0, 0.84), (0.5, 0.12, 0.12), "Frame Black")
    b.strut((0, 0, 0), (0, 0, 0.78), 0.03, "Frame Black")
    emit("Wash - Console Wall", "found-light", b, position=(wx, wy, 0), yaw=90,
         note="source not visible in any photo; placed behind the console screen")
    light("Wash - Console Wall", "found", "spot", (wx, wy, 0.92), 3 * PANEL_LM, BLUE, aim=(X_SL, wy, 3.0),
          cone_deg=110.0, range_m=8.0, shadows=True,
          note="source not visible in any photo; placed behind the console screen")


# ── Furniture and gear ───────────────────────────────────────────────
def piano():
    b = Builder()
    outline = [(-0.75, -0.92), (0.75, -0.92), (0.75, -0.35), (0.66, 0.0), (0.36, 0.25), (0.18, 0.55),
               (0.05, 0.80), (-0.25, 0.92), (-0.60, 0.86), (-0.75, 0.60)]
    b.prism(outline, 0.62, 0.98, "Piano Black")
    # The lid, propped open on the bass side's hinge.
    hinge = Matrix.Translation((-0.75, 0, 1.0)) @ Matrix.Rotation(math.radians(-38), 4, "Y")
    b.prism([(x + 0.75, y) for x, y in outline], 0.0, 0.025, "Piano Black", m=hinge)
    b.strut((0.45, -0.1, 0.98), (0.30, -0.1, 1.62), 0.02, "Piano Black")
    b.box((0, -1.0, 0.70), (1.24, 0.16, 0.025), "Piano Keys")
    b.box((0, -1.0, 0.66), (1.50, 0.20, 0.06), "Piano Black")
    b.box((0, -0.80, 1.10), (0.9, 0.02, 0.24), "Piano Black")
    for x, y in [(-0.62, -0.72), (0.62, -0.72), (-0.25, 0.70)]:
        b.box((x, y, 0.31), (0.10, 0.10, 0.62), "Piano Black")
    b.box((0, -0.80, 0.14), (0.30, 0.10, 0.28), "Piano Black")
    return b


def bench():
    b = Builder()
    b.box((0, 0, 0.47), (0.70, 0.36, 0.08), "Frame Black")
    for x in (-0.3, 0.3):
        for y in (-0.14, 0.14):
            b.box((x, y, 0.215), (0.04, 0.04, 0.43), "Frame Black")
    return b


def stage_keyboard(mat):
    b = Builder()
    b.box((0, 0, 0.90), (1.30, 0.32, 0.09), mat)
    b.box((0, -0.04, 0.948), (1.20, 0.14, 0.012), "Piano Keys")
    for y in (-0.12, 0.12):
        b.strut((-0.42, y, 0.0), (0.42, y, 0.85), 0.03, "Frame Black")
        b.strut((0.42, y, 0.0), (-0.42, y, 0.85), 0.03, "Frame Black")
    return b


def bongos():
    b = Builder()
    b.cyl((-0.13, 0, 0.80), 0.10, 0.17, "Drum Skin", n=10)
    b.cyl((0.14, 0, 0.80), 0.12, 0.17, "Drum Skin", n=10)
    b.strut((0, 0, 0.45), (0, 0, 0.80), 0.025, "Metal Grey")
    for k in range(3):
        a = math.radians(120 * k)
        b.strut((0, 0, 0.48), (0.3 * math.cos(a), 0.3 * math.sin(a), 0.01), 0.018, "Metal Grey")
    return b


def amp(w=0.55, d=0.26, h=0.45):
    b = Builder()
    b.box((0, 0, h / 2), (w, d, h), "Frame Black", mats={"-y": "Woven Tan"})
    return b


def pa_stack():
    b = Builder()
    b.box((0, 0, 0.30), (0.50, 0.58, 0.60), "Frame Black")
    b.strut((0, 0, 0.60), (0, 0, 1.75), 0.035, "Frame Black")
    b.box((0, 0, 2.05), (0.36, 0.34, 0.60), "Frame Black")
    return b


def console():
    """The mixing desk: a sloped surface between walnut cheeks. Local x is its length."""
    b = Builder()
    length = 3.65
    side = [(-0.50, 0.0), (0.50, 0.0), (0.50, 1.10), (0.36, 1.12), (0.22, 0.98), (-0.50, 0.76)]
    # The side profile (y, z), extruded along the desk's length: the prism's
    # own (x, y, z) become the desk's (y, z, x).
    m = Matrix(((0, 0, 1, 0), (1, 0, 0, 0), (0, 1, 0, 0), (0, 0, 0, 1)))
    b.prism(side, -length / 2 + 0.06, length / 2 - 0.06, "Console Grey", m=m)
    for x in (-length / 2 + 0.03, length / 2 - 0.03):
        b.box((x, 0.0, 0.57), (0.06, 1.06, 1.14), "Walnut")
    return b


def tv_on_stand(width=1.45, centre=1.45):
    b = Builder()
    height = width * 9 / 16
    b.box((0, 0, centre), (width, 0.05, height), "Frame Black", mats={"-y": "Screen"})
    b.strut((0, 0.05, 0.05), (0, 0.05, centre), 0.06, "Frame Black")
    b.box((0, 0.05, 0.03), (0.9, 0.55, 0.05), "Frame Black")
    return b


def studio_monitor():
    b = Builder()
    b.box((0, 0, 1.42), (0.45, 0.42, 0.84), "Frame Black")
    m = Matrix.Translation((0, -0.215, 0)) @ rot_x(90)
    b.cyl((0, 1.22, 0), 0.17, 0.006, "Console Grey", n=12, m=m)
    b.cyl((0, 1.56, 0), 0.07, 0.006, "Console Grey", n=10, m=m)
    b.box((0, 0, 0.50), (0.34, 0.34, 1.0), "Frame Black")
    return b


def rack():
    b = Builder()
    b.box((0, 0, 0.42), (0.70, 1.60, 0.84), "Frame Black")
    b.box((0, 0, 0.87), (0.62, 1.20, 0.06), "Console Grey")
    return b


def diffuser_panel():
    """A rolling booth panel: 1.3 m of skyline diffuser under a glass top."""
    b = Builder()
    w, lower, top = 1.30, 1.36, 2.02
    b.box((0, 0, 0.12 + (lower - 0.12) / 2), (w - 0.08, 0.09, lower - 0.12), "Diffuser Frame")
    for y in (-0.047, 0.047):
        pts = [(-w / 2 + 0.04, y, 0.14), (w / 2 - 0.04, y, 0.14), (w / 2 - 0.04, y, lower - 0.02),
               (-w / 2 + 0.04, y, lower - 0.02)]
        b.face(pts, "Diffuser Cells", uvs=[(p[0] / 0.52, p[2] / 0.52) for p in pts], toward=(0, y * 10, 0.7))
    for x in (-w / 2 + 0.02, w / 2 - 0.02):
        b.box((x, 0, top / 2 + 0.05), (0.04, 0.07, top - 0.10), "Diffuser Frame")
    b.box((0, 0, top - 0.02), (w, 0.07, 0.04), "Diffuser Frame")
    b.box((0, 0, lower), (w, 0.07, 0.04), "Diffuser Frame")
    b.box((0, 0, (lower + top) / 2), (w - 0.08, 0.008, top - lower - 0.06), "Glass")
    for x in (-w / 2 + 0.12, w / 2 - 0.12):
        b.box((x, 0, 0.06), (0.07, 0.50, 0.05), "Diffuser Frame")
    return b


def couch():
    b = Builder()
    b.box((0, 0.0, 0.22), (2.0, 0.86, 0.26), "Leather Tan")
    b.box((0, -0.05, 0.40), (1.72, 0.66, 0.12), "Leather Tan")
    b.box((0, 0.34, 0.55), (2.0, 0.18, 0.46), "Leather Tan")
    for x in (-0.93, 0.93):
        b.box((x, 0, 0.42), (0.14, 0.86, 0.34), "Leather Tan")
    for x in (-0.9, 0.9):
        for y in (-0.36, 0.36):
            b.box((x, y, 0.045), (0.05, 0.05, 0.09), "Walnut")
    return b


def armchair():
    b = Builder()
    b.box((0, 0, 0.27), (0.76, 0.76, 0.34), "Woven Tan")
    b.box((0, 0.30, 0.60), (0.76, 0.16, 0.40), "Woven Tan")
    for x in (-0.33, 0.33):
        b.box((x, 0, 0.50), (0.10, 0.76, 0.16), "Woven Tan")
    return b


def coffee_table():
    b = Builder()
    b.box((0, 0, 0.40), (1.0, 0.45, 0.05), "Walnut")
    b.box((0, 0, 0.15), (0.9, 0.38, 0.03), "Walnut")
    for x in (-0.46, 0.46):
        for y in (-0.19, 0.19):
            b.box((x, y, 0.19), (0.05, 0.05, 0.38), "Walnut")
    return b


def beanbag():
    b = Builder()
    b.cyl((0, 0, 0.0), 0.55, 0.30, "Woven Tan", n=10, top=0.48)
    b.cyl((0, 0, 0.30), 0.48, 0.22, "Woven Tan", n=10, top=0.22)
    return b


def plant():
    b = Builder()
    b.cyl((0, 0, 0), 0.17, 0.35, "Woven Tan", n=10, top=0.20)
    for k in range(6):
        a = math.radians(60 * k + 15)
        lean = 0.35 + 0.1 * (k % 3)
        root = Vector((0, 0, 0.35))
        tip = Vector((lean * math.cos(a) * 1.6, lean * math.sin(a) * 1.6, 1.25 + 0.15 * (k % 2)))
        side = Vector((-math.sin(a), math.cos(a), 0)) * 0.13
        mid = (root + tip) / 2 + Vector((0, 0, 0.1))
        b.face([root, mid - side, tip, mid + side], "Plant Green")
    return b


def stool():
    b = Builder()
    b.cyl((0, 0, 0.72), 0.17, 0.03, "Metal Grey", n=12)
    for k in range(4):
        a = math.radians(90 * k + 45)
        b.strut((0.12 * math.cos(a), 0.12 * math.sin(a), 0.72), (0.22 * math.cos(a), 0.22 * math.sin(a), 0.0),
                0.02, "Metal Grey")
    return b


def folding_table():
    b = Builder()
    b.box((0, 0, 0.72), (2.44, 0.76, 0.045), "Table White")
    for x in (-0.95, 0.95):
        for y in (-0.30, 0.30):
            b.box((x, y, 0.35), (0.03, 0.03, 0.70), "Metal Grey")
        b.box((x, 0, 0.12), (0.03, 0.60, 0.03), "Metal Grey")
    return b


def chair():
    """A stacking chair, facing -y."""
    b = Builder()
    b.box((0, 0, 0.45), (0.48, 0.46, 0.06), "Chair Fabric")
    b.box((0, 0.22, 0.68), (0.46, 0.05, 0.34), "Chair Fabric")
    for x in (-0.21, 0.21):
        b.box((x, -0.19, 0.21), (0.025, 0.025, 0.42), "Frame Black")
        b.box((x, 0.22, 0.43), (0.025, 0.025, 0.86), "Frame Black")
    return b


def office_chair():
    b = Builder()
    b.box((0, 0, 0.48), (0.50, 0.48, 0.07), "Frame Black")
    b.box((0, 0.23, 0.80), (0.48, 0.05, 0.56), "Frame Black")
    b.cyl((0, 0, 0.08), 0.03, 0.38, "Metal Grey", n=8)
    for k in range(5):
        a = math.radians(72 * k)
        b.strut((0, 0, 0.09), (0.30 * math.cos(a), 0.30 * math.sin(a), 0.04), 0.03, "Frame Black")
    return b


def whiteboard(width=3.6):
    b = Builder()
    b.box((0, -0.02, 1.60), (width, 0.03, 1.20), "Metal Grey", mats={"-y": "Whiteboard"})
    return b


def flat(w, d, mat, t=0.012, textured=False):
    b = Builder()
    if textured:
        pts = [(-w / 2, -d / 2, t), (w / 2, -d / 2, t), (w / 2, d / 2, t), (-w / 2, d / 2, t)]
        b.face(pts, mat, uvs=[(p[0] / 0.4, p[1] / 0.4) for p in pts])
    else:
        b.box((0, 0, t / 2), (w, d, t), mat)
    return b


def place(name, kind, builder, scan_xy=None, at=None, yaw=0.0, note=None):
    x, y = S(*scan_xy) if scan_xy else at
    emit(name, kind, builder, position=(x, y, 0), yaw=yaw, note=note)


def build_set_dressing():
    photo = "placed from the photographs, not the scan: +/- 0.5 m"
    # Stage side. The grand's tail points into the upstage stage-right corner.
    place("Grand Piano", "gear", piano(), at=(-5.57, 0.45), yaw=45)
    place("Piano Bench", "gear", bench(), at=(-4.42, -0.70), yaw=45)
    place("Keyboard - Red", "gear", stage_keyboard("Keyboard Red"), at=(-3.45, 0.25), yaw=-15, note=photo)
    place("Bongos", "gear", bongos(), at=(-2.75, -0.35), note=photo)
    place("Drum Mat", "gear", flat(2.30, 1.90, "Rug Dark"), scan_xy=(4.05, -0.35))
    place("Rug - Stage", "furniture", flat(1.60, 0.90, "Rug", textured=True), scan_xy=(2.55, -0.2))
    for i, ys in enumerate([3.55, 1.55, -2.3, -3.25], start=1):
        place(f"Amp {i}", "gear", amp(), scan_xy=(4.62, ys))
    place("PA - Stage Right", "gear", pa_stack(), scan_xy=(1.5, 3.75))
    place("PA - Stage Left", "gear", pa_stack(), scan_xy=(1.5, -4.2))

    # The booth of rolling diffuser panels, stage left. The two rows nearest
    # the room are in the photos; the two against the curtain and the end
    # wall are in the scan only, as low lines.
    for i, (xs, ys, yaw) in enumerate([(2.78, -4.4, 90), (4.12, -4.4, 90), (2.2, -5.72, 0), (2.2, -7.0, 0)], 1):
        place(f"Diffuser Panel {i}", "furniture", diffuser_panel(), scan_xy=(xs, ys), yaw=yaw)
    for i, (xs, ys, yaw) in enumerate([(4.8, -5.2, 0), (4.8, -6.55, 0), (2.95, -7.28, 90), (4.2, -7.28, 90)], 5):
        place(f"Diffuser Panel {i}", "furniture", diffuser_panel(), scan_xy=(xs, ys), yaw=yaw,
              note="in the scan only (a low line); the photos do not show this side")

    # Console end.
    place("Mixing Console", "gear", console(), scan_xy=(-0.92, -5.72), yaw=-90)
    place("Console Chair", "furniture", office_chair(), scan_xy=(-0.9, -4.7), yaw=90)
    place("Console Screen", "gear", tv_on_stand(1.95, 1.60), scan_xy=(-0.9, -6.62), yaw=-90)
    for i, xs in enumerate([-2.25, 0.4], start=1):
        place(f"Studio Monitor {i}", "gear", studio_monitor(), scan_xy=(xs, -6.75), yaw=-90)
    place("Side Rack", "gear", rack(), scan_xy=(-3.85, -5.1), yaw=90)

    # The tables in the middle of the room, on their mat.
    place("Rug - Tables", "furniture", flat(4.30, 3.00, "Rug Dark"), scan_xy=(-1.5, -1.15))
    for i, ys in enumerate([0.6, -0.6, -1.85, -3.05], start=1):
        place(f"Table {i}", "furniture", folding_table(), scan_xy=(-1.48, ys), yaw=90)
        for j, xs in enumerate([-2.2, -0.8], start=1):
            # Seated facing the curtain, behind each table.
            place(f"Chair {i}.{j}", "furniture", chair(), scan_xy=(xs, ys + 0.55), yaw=90,
                  note="chairs move daily; these stand for them")
    for i, ys in enumerate([-0.2, -0.8, -1.4, -2.0], start=1):
        place(f"Chair - Wall {i}", "furniture", chair(), scan_xy=(-4.65, ys), yaw=180)

    # The lounge, stage right.
    place("Rug - Lounge", "furniture", flat(2.45, 3.30, "Rug", textured=True), scan_xy=(-1.35, 3.87))
    place("Couch", "furniture", couch(), scan_xy=(-1.32, 5.45), yaw=90)
    place("Coffee Table", "furniture", coffee_table(), scan_xy=(-1.25, 4.32), yaw=90)
    place("Beanbag", "furniture", beanbag(), scan_xy=(0.2, 4.55))
    place("Plant", "furniture", plant(), scan_xy=(-2.6, 5.7))
    place("Armchair", "furniture", armchair(), scan_xy=(-4.55, 3.5), yaw=180)
    for i, ys in enumerate([2.6, 2.15, 1.3], start=1):
        place(f"Stool {i}", "furniture", stool(), scan_xy=(-4.7, ys))
    place("TV - Lounge", "gear", tv_on_stand(1.45, 1.40), scan_xy=(-4.6, 4.8), yaw=180)
    place("TV - Whiteboard", "gear", tv_on_stand(1.45, 1.40), scan_xy=(1.75, 5.35), yaw=90)
    bx, by = S(1.5, 5.95)
    emit("Whiteboard", "furniture", whiteboard(), position=(bx, by, 0), yaw=90, movable=False)


# ── Run ──────────────────────────────────────────────────────────────
bpy.ops.wm.read_factory_settings(use_empty=True)
textures = {"Floor Wood": save_texture("floor-wood", tex_floor()),
            "Ceiling Tile": save_texture("ceiling-tile", tex_ceiling()),
            "Diffuser Cells": save_texture("diffuser-cells", tex_diffuser()),
            "Rug": save_texture("rug-weave", tex_rug())}
for material_name in MATERIALS:
    materials[material_name] = make_material(material_name, textures.get(material_name))

build_architecture()
build_doors()
build_practicals()
build_found_lights()
build_set_dressing()

glb = OUT / "lab138-room.glb"
bpy.ops.export_scene.gltf(filepath=str(glb), export_format="GLB", export_yup=True, export_apply=True,
                          export_image_format="JPEG", export_jpeg_quality=85, export_materials="EXPORT",
                          export_cameras=False, export_lights=False, export_extras=False)

environment = {
    "version": 1,
    "name": "Lab 138",
    "about": "Generated by tools/build_lab138_room.py -- edit the script, or these records; not the GLB.",
    # Found beside this file in `model/`, or in $IGNITION_ENVIRONMENT_DIR.
    "model": "lab138-room.glb",
    "measured": {
        "width_m": X_SL - X_SR, "depth_m": DEPTH, "soffit_m": SOFFIT_Z, "apex_m": APEX[2],
        "corridor_m": {"length": round(X_SR - X_CORRIDOR_END, 2), "width": round(Y_CORRIDOR_IN - Y_HOUSE, 2)},
        "curtain_top_m": CURTAIN_TOP,
    },
    "materials": [{"name": n, "base_color_linear": list(c), "roughness": r, "metallic": m, "source": src}
                  for n, (c, r, m, src) in MATERIALS.items()],
    "light_groups": [
        {"name": "house", "label": "House lights", "on": False},
        {"name": "found", "label": "Stage lights as photographed", "on": False},
    ],
    "objects": objects,
    "lights": lights,
}
(VENUE / "environment.json").write_text(json.dumps(environment, indent=1) + "\n")
print(f"lab138: {len(objects)} objects, {len(lights)} lights, {triangles} triangles, "
      f"{glb.stat().st_size / 1024:.0f} KiB -> {glb}")
