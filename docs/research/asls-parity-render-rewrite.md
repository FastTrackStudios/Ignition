# ASLS-parity beam rendering — scoping / handoff

Written 2026-08-24, after Slices 17–20 (`docs/research/lighting-console-
landscape.md`) iterated on the beam-cone approximation and the operator
was explicit it's not good enough: **"we don't want the approximation
we want the actual visualization like in ASLS, so we don't want our
version theirs looks way better."**

This doc scopes the real rewrite instead of continuing to tune the
approximation. Not started — this is the plan, to be picked up as its
own piece of work.

## What Ignition does today

`crates/ignition-viz/src/{mesh.rs,live_pipeline.rs,live_shader.wgsl}`:

- **CPU-baked geometry.** `mesh::build_glow_cone` bakes a beam's
  triangles (position + baked-in vertex colour) into a shared
  `Vec<Vertex>`/`Vec<u32>` every frame, for every lit fixture, on the
  CPU. No instancing — `mesh.rs`'s own header says so explicitly
  ("Object counts here are small (hundreds), so there is no
  instancing").
- **Shared HDR + single tonemap** (Slice 18): opaque geometry and beam
  glow both render into one `Rgba16Float` HDR target, resolved once,
  tonemapped once by a final fullscreen pass (`fs_tonemap`). Beams do
  **not** have their own pass or their own tonemap treatment.
  `DoubleSide`-equivalent rendering (`cull_mode: None`) plus this shared
  tonemap is why a single beam's near+far walls double-count into the
  same eventual compressed result.
  ("we don't want the approximation")
- **A single hand-baked intensity curve.** As of Slice 20,
  `glow_intensity_at(t) = 1.0 / (1.0 + 4.5*t²)` is baked into vertex
  colours at 6 rings along the beam's length at CPU build time — an
  approximation of a continuous per-fragment curve, not the real thing.
- **A hand-rolled WGSL point-light loop** (`fs_main`'s `for i in
  0..light_count`) stands in for real per-fixture lighting of the room.
- **`BeamThrow`** (Slice 17) computes a beam's real length via a
  floor-plane intersection against the venue's actual bounds — this
  part is *more* precise than ASLS's own approach (see below) and
  should be kept regardless of what else changes.

## What ASLS actually does (verified against their real source,
`github.com/ASLS-org/studio`, `src/plugins/visualizer/{moving_head.js,
shaders/beam.{vertex,fragment}.glsl,visualizer.js}` — not re-derived
from a screenshot)

- **One `THREE.InstancedMesh` for every beam**, `MAX_INSTANCES` capacity,
  one draw call for the whole rig's beams. Per-fixture state (colour,
  intensity, beam angle, world position, aim direction) rides as
  **per-instance buffer attributes** (`wpos`, `direction`, `color`,
  `intensity`, `angle` — all `THREE.InstancedBufferAttribute`s), updated
  in place each frame (`instanceMatrix.needsUpdate = true`) rather than
  rebuilding geometry.
- **One constant-radius `CylinderGeometry`, widened by the vertex
  shader** (`computeRadiusVertexScaleFactor` in
  `beam.vertex.glsl`) — not different CPU geometry per fixture. Fixed
  `BEAM_LENGTH = 100` (always absurdly overshooting any real room);
  "reaching the floor" is a per-fragment world-Z fade in the fragment
  shader (`floorFade`), not a raycast — **Ignition's own `BeamThrow`
  (real floor-plane intersection) is more precise here and should be
  kept, not replaced.**
- **Beam material**: `ShaderMaterial`, `transparent: true`, `depthWrite:
  false`, `side: THREE.DoubleSide`, `blending: THREE.AdditiveBlending`,
  **`toneMapped: false`**. Confirmed directly in `moving_head.js`
  (`new THREE.InstancedMesh(beamGeo, new THREE.ShaderMaterial({...})`).
  Notably: **ASLS also renders both sides additively with no depth
  write** — the same "near+far wall both contribute" property
  Ignition's beams have. They don't avoid it structurally; they avoid
  its *symptom* (blowing out) by (a) `toneMapped: false` — the beam
  shader's own attenuation curve is tuned to self-regulate into a
  reasonable range without leaning on the renderer's global tonemap to
  save it — and (b) the real continuous falloff curve below, not a
  handful of baked stops.
- **The real intensity formula** (`beam.fragment.glsl`):
  `attenuation = 2.0 / (1.0 + alignmentFactor * distance + radians(angle)
  * distance * distance)` — a genuine inverse-quadratic decay computed
  **per-fragment**, using the fragment's real local-space distance along
  the beam (a varying computed in the vertex shader from a `wpos`/
  `direction` instance attribute pair), not a value interpolated between
  a small number of baked vertex colours.
- **Real per-fixture `THREE.SpotLight`** lighting the room via Three.js's
  built-in lighting system (`physicallyCorrectLights = true`, light
  `decay: 1.0` — linear, not inverse-square). **No shadow mapping**
  anywhere (`castShadow` never set) — Ignition isn't missing shadows
  relative to ASLS; neither has them.
- **Renderer**: `antialias: true`, `toneMapping: THREE.ACESFilmicToneMapping`,
  `outputColorSpace: THREE.SRGBColorSpace` — already matched (Slice 2,
  Slice 18).

## The real gap, in priority order

1. **Glow isn't decoupled from the shared tonemap.** This is very
   likely the single biggest remaining visual gap — ASLS's beams
   self-regulate via their own curve *and* skip the global tonemap
   entirely; Ignition's beams go through the same single tonemap pass
   as the room's opaque geometry, so a bright, wide, double-counted
   beam competes for the same limited HDR headroom as everything else
   in the scene.
2. **No GPU instancing.** Not a visual-quality gap by itself (Ignition's
   fixture counts are in the hundreds, not thousands — CPU baking is
   fine at this scale per `mesh.rs`'s own reasoning) but it's the
   architecture that makes (3) practical to do per-fragment instead of
   per-baked-vertex.
3. **No real per-fragment local-space falloff.** Slice 20 baked the
   right *curve shape* into 6 discrete vertex rings at CPU build time —
   visually close, but still a piecewise-linear approximation of a
   continuous shader-side function, and it required no new vertex
   attributes (deliberately, to avoid touching `shot`'s regression path
   — see Slice 20's own "not ported" list). Doing this for real needs
   either a dedicated glow-only vertex format (extra position-along-beam
   attribute) or a separate glow shader path that doesn't share
   `mesh::Vertex` with every other static-geometry consumer.
4. **Hand-rolled point-light loop vs. real per-material lighting.** Lower
   priority — functionally the two approaches converge (both are cone-
   angle-gated, distance-attenuated per-light contributions); this is
   an implementation-detail gap, not a visual one, and reproducing
   Three.js's real light/material interaction in a hand-written WGSL
   forward-lighting pass is not obviously higher-value than what
   already exists.

## Proposed phasing

Each phase should land, get visually verified (a `--snapshot` A/B, same
pattern every prior slice used), and get its own commit — do not
attempt all four in one pass.

**Phase 1 — separate the glow pass from the shared tonemap.**
Render glow into its own HDR target (or a separate region/layer of the
existing one), tonemapped by its own pass (or the same `fs_tonemap`
shader invoked a second time with different exposure/curve tuning)
*before* being composited additively onto the already-tonemapped opaque
result — mirroring `toneMapped: false`'s actual effect: the beam's own
brightness curve decides how it looks, independent of how bright the
rest of the scene got. This requires either resolving a second MSAA
target or restructuring so glow's depth-test against opaque geometry
still works without sharing the same depth attachment across passes —
work through wgpu's multisampled-depth-as-a-sampled-texture path, or
render glow at single-sample resolution with a resolved (non-MSAA) depth
copy for testing. This is the phase most likely to fix the "solid
colour instead of soft light" complaint at its root, ahead of anything
in Phase 2/3.

**Phase 2 — GPU instancing for beam geometry.**
Move from `mesh.rs`'s per-frame CPU triangle bake to a real instanced
draw: one beam mesh (a cylinder, matching ASLS's own choice), one
per-instance buffer carrying `(world_pos, direction, color, intensity,
beam_angle)` per lit fixture, updated by `scene.rs` each frame instead
of rebuilt-from-scratch. `wgpu` equivalent: an
instance vertex buffer + `@builtin(instance_index)`-driven per-instance
uniform lookup, or a storage buffer of per-instance data read by index
(matches this project's existing `PointLight` storage-buffer pattern
already used for the point-light pass). Real payoff is enabling Phase 3
cleanly, not raw draw-call count at Ignition's current fixture counts.

**Phase 3 — real per-fragment local-space falloff.**
Once beams are instanced, port ASLS's actual vertex/fragment shader
math: a beam-local `distance` varying computed from the instance's own
`direction`/length, `attenuation = 2.0 / (1.0 + alignment*distance +
radians(angle)*distance²)` evaluated per-fragment in `fs_glow`
(replacing `glow_intensity_at`'s baked-at-6-rings approximation), and
real vertex-shader-side radius widening (replacing the CPU-computed
`near_radius`/`radius` ring geometry). Ignition's own `BeamThrow`
floor-plane intersection stays — feed it in as the per-instance beam
length instead of ASLS's fixed 100-unit overshoot-and-fade approach,
since it's already more precise.

**Phase 4 (optional, lowest priority) — reconsider the lighting model.**
Only revisit the point-light loop vs. a more Three.js-like per-material
system if Phase 1–3 don't close the gap on room illumination
specifically (as opposed to beam appearance, which they're aimed at).
Not recommended to start without first seeing how much Phase 1–3 alone
closes the perceived gap.

## Non-goals / explicitly not in scope

- Shadow mapping — ASLS doesn't have it either (confirmed: `castShadow`
  is never set anywhere in their source). Not a real gap.
- Matching ASLS's fixed 100-unit-beam-plus-shader-fade floor behaviour —
  Ignition's `BeamThrow` raycast-free floor-plane intersection is
  already more correct and should be kept through every phase above.
- A full Three.js-style scene graph / material system port. The goal is
  visual parity for beam rendering, not architectural parity with
  Three.js as a 3D engine.

## Acceptance criteria

- A `--snapshot` of a moderate live cue (a handful of fixtures, not the
  "all lights on" extreme) reads as genuinely soft/translucent light in
  air, with a visible bright core near the fixture and a smooth
  continuous falloff — not a flat-shaded or hard-banded shape.
- The "all lights on" extreme case (Norco's real "All" Eos group, ~70
  fixtures at once — `data/shows/all-lights-on.json`) no longer clips to
  a solid white/saturated blob from a house-view angle looking into the
  packed OH-mover cluster (the case that exposed the original bug in
  Slice 17).
- `cargo test --workspace` stays green; `shot`'s regression PNG stays
  byte-identical (the static, no-live-data path should be unaffected by
  any of this — same invariant every slice through Slice 20 held).
- Frame time at Norco's real fixture count (71 patched, up to ~70 lit
  simultaneously) doesn't regress — instancing should make Phase 2+
  strictly cheaper than the current per-frame CPU bake, not more
  expensive; verify this isn't accidentally violated by a naive first
  pass at Phase 1's dual-target approach.

## Effort estimate

Rough, not a commitment: Phase 1 is the highest-value, most
self-contained piece — a few hours of focused wgpu pipeline work plus
verification. Phase 2 is a bigger lift (touches `mesh.rs`'s core data
model, `scene.rs`'s per-frame fixture loop, and `live_pipeline.rs`'s
bind group setup) — likely a full session on its own. Phase 3 is
smaller once Phase 2 lands (mostly shader math, informed directly by
this doc's already-quoted ASLS formula). Total: several sessions'
worth of focused work, not a single sitting — this is why it's being
scoped and handed off rather than started inline.
