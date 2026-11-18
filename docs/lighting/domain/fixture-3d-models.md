# Real fixture 3D models — where they come from, how they're scaled

**Status**: implemented 2026-08-24. Replaces the one-size box+cone every
fixture used to render as, regardless of type — a floor-mounted beam mover
and a 1m LED bar were rendering with the same fixed 0.9m marker.

## Where the models come from

Checked GDTF Share first (the industry's real per-manufacturer 3D model
repository) — not usable here: it requires an account, and coverage is
built by manufacturers/community submissions, so the Norco rig's actual
fixtures (Uking, Betopper, Rockville, Riukoe — Amazon-tier gear, not
professional stock) almost certainly have no submitted models there.

**Q Light Controller+** (Apache-2.0, see
[`lighting-console-landscape.md`](../research/lighting-console-landscape.md))
solves the same problem the same way any OSS console has to: **one generic
mesh per fixture *category***, not per manufacturer —
`resources/meshes/fixtures/{par,moving_head,scanner,hazer,smoke,strobe}.dae`,
plus generic primitives and truss pieces. This is directly reusable and is
what Norco's fixtures now render as.

## The pipeline

1. **Source**: QLC+'s six category `.dae` (Collada) files, Blender-authored,
   Y-up per QLC+'s own README convention.
2. **Conversion** (`tools/convert_qlc_dae.py` — run once when adding a
   source `.dae`, not a build-time step): parses
   `<library_geometries>`, triangulates any n-gon `<polylist>` via fan
   triangulation, and remaps every position/normal from COLLADA's Y-up to
   this project's Z-up convention. The remap is `(x, y, z) → (x, z, -y)` —
   chosen (and verified via the transform's determinant, +1, so it's a
   proper rotation with no accidental mirroring) so COLLADA's own "up"
   (the modeled fixture's lens/business-end direction) lands on **local
   −Z**, matching the hang/beam-direction convention every other mesh
   builder in this crate already uses (`mesh.rs`, and Augment3d's own
   convention — see `norco-venue-reference.md`).
3. **Output**: plain OBJ (`v`/`vn`/`f a//a b//b c//c`, always triangulated,
   position and normal always at the same index) under
   `crates/ignition-viz/assets/qlc-meshes/` — see `LICENSE-NOTICE.txt`
   there for the Apache-2.0 attribution.
4. **Loading**: `obj_mesh.rs` is a minimal parser for exactly that shape —
   not a general OBJ implementation, it would reject a file with quads or
   mismatched v/vn indices. Loaded once per fixture category via
   `OnceLock` (`fixture_profile.rs`), not once per fixture instance — 69
   fixtures at Norco share 3 parsed meshes (par/moving-head/hazer), not 69
   copies.
5. **Scaling**: `fixture_profile.rs` maps `(manufacturer, model)` — straight
   from the live Eos patch, see
   [`norco-patch-and-groups.md`](norco-patch-and-groups.md) — to a real
   target size researched from actual product listings, and computes a
   uniform scale factor from the mesh's own bounding box to that size.
   Current mapping:

   | Manufacturer / model | QLC+ mesh | Target size (m) | Source |
   |---|---|---|---|
   | Uking / Par | `par` | 0.20 | Open Fixture Library, U\`King Par Light B262: 180×180×100mm |
   | Betopper (LB150) | `moving_head` | 0.35 | no published spec; sized from the compact-150W-beam-mover class |
   | Riukoe / Lixada, Gobo | `moving_head` | 0.235 | Lixada's listing for the same 11ch shell: 14.5×17×23.5cm |
   | Chauvet / Hurricane Haze | `hazer` | 0.28 | Chauvet spec: 11×6×9 in |
   | Rockville / Rockstrip | *(procedural box, no QLC+ mesh fits)* | 1.02 × 0.067 × 0.065 | Rockville spec: 40.16×2.64×2.56 in |

   `scanner`/`smoke`/`strobe` are converted and available
   (`fixture_profile::generic_shape`) but unused by any fixture in Norco's
   patch today — ready for the next venue.

## What's still a placeholder

- **Props** (people, pillars): no dedicated shape exists yet, and a plain
  axis-aligned bounding box for a standing person reads as a "tall box"
  with no silhouette — worse than no geometry, because it's easy to
  mistake for a fixture marker at a glance. **Hidden entirely** in
  `scene.rs` (`Person`/`Pillar` name prefixes) rather than left in looking
  wrong, per the operator's explicit call 2026-08-24. Revisit once there's
  a real human/architectural model to draw instead — QLC+ doesn't have one
  (it's a lighting console, not a venue-modelling tool), so this needs a
  different source than the fixture pipeline above.
- **Rockstrip bars, hazers-without-a-dedicated-mesh-fit** use hand-built
  procedural shapes (`mesh.rs::add_bar`), not an imported mesh — fine for a
  bar (QLC+ has no linear-batten category), less ideal long-term for
  anything else that ends up needing one.
- **No instancing.** At Norco's scale (69 fixtures) this doesn't matter —
  118k vertices renders in a headless single frame with no trouble. It
  will matter once real-time windowing (the still-open item from
  `lighting-console-landscape.md` §7.1/§9) needs to redraw every frame; the
  fix is standard GPU instancing per fixture-mesh-category, not a redesign
  of this pipeline.
