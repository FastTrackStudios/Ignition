#!/usr/bin/env python3
"""Converts QLC+'s generic fixture meshes (resources/meshes/fixtures/*.dae,
Apache-2.0 — see https://github.com/mcallegari/qlcplus) to the plain OBJ
format `crates/ignition-viz/src/obj_mesh.rs` loads: triangulated, position
and normal always at the same index, Y-up remapped to this project's Z-up
convention (local -Z = beam/hang direction — see
docs/domain/fixture-3d-models.md for why).

Usage: python3 convert_qlc_dae.py <src_dir_of_.dae_files> <out_dir>
Not a build step — run once when adding/updating a source .dae, commit the
resulting .obj under crates/ignition-viz/assets/qlc-meshes/.
"""
import xml.etree.ElementTree as ET
import sys, os

ns = {'c': 'http://www.collada.org/2005/11/COLLADASchema'}

def parse_source(mesh, source_id):
    src = mesh.find(f".//c:source[@id='{source_id}']", ns)
    floats = [float(x) for x in src.find('c:float_array', ns).text.split()]
    stride = int(src.find('c:technique_common/c:accessor', ns).get('stride'))
    return floats, stride

def remap(x, y, z):
    # DAE Y-up -> our Z-up, chosen so DAE's own +Y (the model's natural
    # "up"/lens direction) lands on our local -Z (the beam/hang direction
    # convention Augment3d and every other mesh builder in this crate use).
    return (x, z, -y)

def convert(path, out_path):
    tree = ET.parse(path)
    root = tree.getroot()
    geoms = root.findall('.//c:library_geometries/c:geometry', ns)

    all_verts = []   # (pos, normal)
    all_tris = []    # indices into all_verts, 0-based

    for g in geoms:
        mesh = g.find('c:mesh', ns)
        # positions source: referenced by <vertices><input semantic="POSITION">
        vertices_el = mesh.find('c:vertices', ns)
        pos_input = vertices_el.find("c:input[@semantic='POSITION']", ns)
        pos_source_id = pos_input.get('source')[1:]
        positions, pstride = parse_source(mesh, pos_source_id)
        assert pstride == 3

        polylist = mesh.find('c:polylist', ns)
        if polylist is None:
            polylist = mesh.find('c:triangles', ns)
        inputs = polylist.findall('c:input', ns)
        stride = max(int(i.get('offset')) for i in inputs) + 1
        vert_offset = next(int(i.get('offset')) for i in inputs if i.get('semantic') == 'VERTEX')
        norm_offset = None
        normals = None
        norm_input = next((i for i in inputs if i.get('semantic') == 'NORMAL'), None)
        if norm_input is not None:
            norm_offset = int(norm_input.get('offset'))
            normals, nstride = parse_source(mesh, norm_input.get('source')[1:])
            assert nstride == 3

        p = [int(v) for v in polylist.find('c:p', ns).text.split()]
        vcount_el = polylist.find('c:vcount', ns)
        if vcount_el is not None:
            vcounts = [int(v) for v in vcount_el.text.split()]
        else:
            count = int(polylist.get('count'))
            vcounts = [3] * count

        # local index -> global vertex index in all_verts, deduped per (pos,norm) pair
        cache = {}
        idx = 0
        poly_indices = []
        for vc in vcounts:
            face = []
            for _ in range(vc):
                pi = p[idx * stride + vert_offset]
                ni = p[idx * stride + norm_offset] if norm_offset is not None else None
                key = (pi, ni)
                if key not in cache:
                    px, py, pz = positions[pi*3:pi*3+3]
                    rx, ry, rz = remap(px, py, pz)
                    if ni is not None:
                        nx, ny, nz = normals[ni*3:ni*3+3]
                        rnx, rny, rnz = remap(nx, ny, nz)
                    else:
                        rnx, rny, rnz = 0.0, 0.0, 1.0
                    cache[key] = len(all_verts)
                    all_verts.append(((rx, ry, rz), (rnx, rny, rnz)))
                face.append(cache[key])
                idx += 1
            # fan-triangulate
            for k in range(1, len(face) - 1):
                all_tris.append((face[0], face[k], face[k+1]))

    with open(out_path, 'w') as f:
        for (px, py, pz), (nx, ny, nz) in all_verts:
            f.write(f"v {px:.6f} {py:.6f} {pz:.6f}\n")
            f.write(f"vn {nx:.6f} {ny:.6f} {nz:.6f}\n")
        for a, b, c in all_tris:
            f.write(f"f {a+1}//{a+1} {b+1}//{b+1} {c+1}//{c+1}\n")

    # bounds for sanity check
    xs = [v[0][0] for v in all_verts]; ys = [v[0][1] for v in all_verts]; zs = [v[0][2] for v in all_verts]
    print(f"{os.path.basename(path)}: {len(all_verts)} verts, {len(all_tris)} tris, "
          f"bounds x[{min(xs):.3f},{max(xs):.3f}] y[{min(ys):.3f},{max(ys):.3f}] z[{min(zs):.3f},{max(zs):.3f}]")

if __name__ == "__main__":
    src_dir, out_dir = sys.argv[1], sys.argv[2]
    os.makedirs(out_dir, exist_ok=True)
    for fn in os.listdir(src_dir):
        if fn.endswith('.dae'):
            convert(os.path.join(src_dir, fn), os.path.join(out_dir, fn.replace('.dae', '.obj')))
