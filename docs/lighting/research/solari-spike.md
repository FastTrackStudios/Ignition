# Bevy Solari spike: raytraced lighting for the rig

*2026-08-26. Bevy 0.19.1, `bevy_solari` 0.19.1, wgpu 29, RTX 4080 on the
NVIDIA 610 driver (Vulkan). Code: `crates/ignition-viz/src/solari.rs`
behind the `solari` Cargo feature; off by default and the default build
is unchanged.*

## The question

Can Solari's real-time raytraced direct lighting take the 67 spot lights
of a venue like Norco — real lumens, as `spawn.rs` builds them — and
replace the shadow-map budget (a handful of shadowed movers) with
correct soft shadows from *every* fixture, at 5120×1440 and 120 fps, and
look better doing it?

## Answer: no, and not for lack of trying. Park it.

Three separate reasons, any one of which is enough on its own.

### 1. Solari has no spot lights

Solari's light list is built in `bevy_solari/src/scene/binder.rs` and
holds exactly two kinds of source: **emissive triangles** (any
`RaytracingMesh3d` whose `StandardMaterial::emissive` is non-zero) and
**directional lights**. `SpotLight` and `PointLight` are not read at all.
Worse, a camera with `SolariLighting` is marked `SkipDeferredLighting`
in the render world, so the PBR deferred pass that would otherwise have
lit the opaque scene with those 67 spots is skipped: under Solari a
lighting rig lights nothing, and the picture is black.

So the spike does what would be needed to make Solari see the rig at
all: each fixture spill gets a stand-in **emissive disc** in the
raytracing scene (5 cm radius, raytraced only, never rasterised), its
emissive driven every frame from the spill's `SpotLight` colour and
intensity (radiance = candela / disc area), with a black **snoot tube**
behind it, also raytraced-only, to give the Lambertian disc something
like the fixture's cone. That is the "emissive lens meshes as area
lights" path, and it is what Solari's own `many_lights` example does
(441 emissive spheres).

It does not make a lighting rig. A disc radiates over a hemisphere with
a cosine falloff; a snoot long enough to narrow a 1.3° beam fixture is
two metres of tube, so it is capped at 0.5 m and the cutoff sits around
6°. The consequence is that the 60,000 cd movers flood the room white
and the 1,600 cd pars vanish under them: the amber and red stage of Bye
Bye Bye cue 18 comes out as one flat grey room. There is no field angle,
no beam-to-field falloff, no gobo, no zoom — none of the profile optics
`r[viz.profile-optics]` describes has an equivalent in an emissive
triangle, and adding a spot light type to Solari's ReSTIR light sampling
is upstream work in `presample_light_tiles.wgsl` / `restir_di.wgsl`,
not something this crate can bolt on.

### 2. It is not fast enough, even with the lighting wrong

Same venue, same cue, same 5120×1440, 300 timed frames on the bench
harness (`--bench`), both binaries built from the same tree:

| scene | default | `--features solari` |
|---|---|---|
| Norco, benchmark cue 0 | **7.07 ms → 141 fps** (holds 120) | **21.1 ms → 47 fps** (GPU-bound) |
| Norco, Bye Bye Bye cue 18, house | **7.50 ms → 133 fps** (holds 120) | **20.2 ms → 49 fps** (GPU-bound) |

Where the Solari frame goes (GPU, ms, cue 18):

| pass | ms |
|---|---|
| `ssr` (see below) | 6.0 |
| `solari_lighting/diffuse_indirect_lighting` | 3.5 |
| `solari_lighting/direct_lighting` | 2.3 |
| `solari_lighting/specular_indirect_lighting` | 0.8 |
| `ssao` | 0.5 |
| `solari_lighting/world_cache` + presample | 0.1 |

Solari's own passes are about 6.7 ms at this resolution — by themselves
already most of the 8.33 ms a 120 Hz frame has, before the prepasses,
the haze camera, bloom and the rest. The 6 ms of `ssr` is a knock-on:
Solari sets `DefaultOpaqueRendererMethod::deferred()` for the whole app,
so the screen-space-reflection node that the default build only pays for
on the one deferred deck material now marches the entire screen. The
haze's shadow maps are still rendered as well, because Bevy's volumetric
fog reads the spot lights and their shadow maps and Solari has no fog of
its own — the shadow-map budget is not replaced, it is added to.

Nothing here scales down to 120 fps. With DLSS Ray Reconstruction
(NVIDIA-only, the `dlss` feature, needs the SDK) Solari renders at a
lower internal resolution and upscales, which is how its example gets
its numbers; that is a vendor SDK dependency the studio cannot carry.

### 3. It is noisy and needs a denoiser Bevy does not ship

Without DLSS-RR there is no denoiser at all. ReSTIR DI/GI with temporal
reuse leaves visible grain on every lit surface — the crops in the
scratchpad show speckled walls and floor against the raster's smooth
pools — and a static camera with 300 frames of history is the *best*
case: a strobe or a fast pan/tilt invalidates the temporal reservoir
every frame. The `SolariLighting::reset` flag exists precisely for
"sudden camera cuts", i.e. a cue.

## What it took to get a picture at all

Kept for whoever reopens this, since none of it is in the docs:

- **Features.** `SolariPlugins::required_wgpu_features()` undersells
  what the device needs. The scene bind group is a *storage* binding
  array (`STORAGE_RESOURCE_BINDING_ARRAY`), the shaders take their
  constants as `IMMEDIATES`, and the passes put GPU timestamps inside a
  compute pass (`TIMESTAMP_QUERY_INSIDE_PASSES`). Bevy's own device asks
  for every feature the adapter has, so it never notices; a host that
  makes the device itself does. wgpu 29 also refuses `EXPERIMENTAL_RAY_QUERY`
  unless `DeviceDescriptor::experimental_features` is the `unsafe`
  enabled token.
- **Limits.** WebGPU's default limits — what the studio's Blitz device
  has — put every binding-array and acceleration-structure limit at zero
  and eight storage buffers a stage where the lighting pass binds
  fourteen. The bench takes the adapter's limits under the feature (and
  deliberately not `INDIRECT_FIRST_INSTANCE`, so Bevy's GPU culling —
  measured slower here — stays off).
- **The studio cannot enable it.** Blitz makes the device through
  `wgpu_context` with `ExperimentalFeatures::default()` (disabled), so
  the studio's `solari` feature requests features the device creation
  will refuse. Enabling it there means another one-hunk vendored patch
  on top of `crates/anyrender-vello-vendored`.
- **Meshes.** Solari builds a BLAS only for a mesh with exactly
  position, normal, uv0 and tangent, indexed u32. Bevy's primitive
  builders give none of the tangents and u16 indices for small meshes;
  the OBJ props are unindexed triangle soup; the skinned band figures
  carry joint indices/weights and are silently left out (the figures in
  the Solari render are unlit for that reason). `solari.rs` fixes up
  the first three; the fourth would need a stripped copy of every
  skinned mesh.
- **Camera.** `SolariLighting` requires `Msaa::Off`, `Hdr`, and a main
  texture with `STORAGE_BINDING`; it pulls in depth, deferred and motion
  vector prepasses. The haze camera is left as it is.

## Recommendation

**Park.** Not as a still/export mode either: the lighting is wrong, not
merely slow, and the wrongness is structural (no spot light type). The
things that would change the answer are all upstream of this repo:

1. Solari gaining analytic spot/point lights in its light list — the
   one that matters; watch `bevy_solari` changelogs for it.
2. A denoiser that is not DLSS (Bevy has discussed a cross-vendor path
   but ships none in 0.19).
3. Fog that samples the raytraced scene, so the shadow maps can
   actually be dropped rather than duplicated.

Until (1) lands there is nothing to adopt. The shadow-map budget in
`budget.rs` remains the right tool for "which movers cut a silhouette".
