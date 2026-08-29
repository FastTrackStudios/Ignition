# Froxel volumetrics

## Why

Bevy's volumetric fog is, in its own words, "implemented as a
postprocessing effect … raymarching in screen space": every pixel walks
the ray itself, every frame, from scratch. There is no froxel grid, no
temporal accumulation and no native way to run it at reduced resolution.

Ignition has been paying for that in two currencies at once. The march
is expensive, so `haze.rs` renders it to a small buffer and stretches
the result — and the stretch is what puts blocks on a cone's apex, the
one artefact that has survived every other change (see
`docs/domain/beams-in-the-air.md` for the list of things measured and
found innocent: step count, ray dither, fog volume extent, shadow map
resolution, lamp radius). Raising the buffer fixes the picture and costs
the frame.

Godot and Unreal do not have this problem because neither raymarches per
pixel. Both build a **frustum-aligned voxel grid** — froxels — light it
once per froxel rather than once per pixel per step, and **accumulate it
across frames**, so a grid far coarser than the screen still resolves a
thin bright shaft. Temporal accumulation is the part that matters: it is
what lets four samples a frame look like sixty-four.

It is also where gobos belong. A froxel is a point in space with a light
list; sampling a projected texture there is natural, where Bevy's
per-pixel march computes spot attenuation analytically and has nowhere
to put one.

## Shape

Compute, as everyone else writes it.

There is a wrong turn recorded here because it was nearly taken. Bevy
declares *its* view bind group — the one the existing fog shader uses —
with `ShaderStages::FRAGMENT`, so a compute pipeline cannot bind that
group. From which it does not follow that the pass must be a fragment
pass: visibility belongs to the layout you create, not to the buffers,
and everything the injection needs is public with public GPU handles —
`ViewShadowBindings` (both shadow textures), `LightMeta`,
`GlobalClusterableObjectMeta`, `ViewClusterBindings`. Declaring our own
layout over the same buffers with `ShaderStages::COMPUTE` is the whole
of it, and we vendor the crate besides.

Four passes, three of them compute:

    inject     3D texture, RGB = in-scattered light, A = extinction
    reproject  blend with last frame's grid, offset by camera motion
    integrate  one invocation per (x, y), walking Z front to back
    apply      per pixel: sample the integrated grid at the pixel's depth

Real 3D storage textures, an integration pass that walks Z once per
column rather than once per pixel, and the published literature applying
directly rather than through a translation.

**The grid.** Frustum-aligned: X and Y are screen tiles, Z is depth
slices distributed exponentially so near froxels are small — a beam's
apex is near, which is exactly where the resolution is wanted. Start at
160x90x64 and make it a setting; that is 920k froxels against 2.4M
pixels at the current haze size, and each is lit *once* rather than
once per step.

**Reprojection.** Each frame jitters the sample position within the
froxel (a Halton sequence in Z, as Frostbite does) and blends with the
previous grid reprojected through the camera's motion. Godot names the
cost honestly: ghosting behind moving lights. A lighting visualiser
moves lights constantly, so the blend weight has to be conservative and
this needs judging by eye against `just beam-test` with the movers
running, not against a still.

**Integration.** One invocation per (x, y), walking Z and accumulating
scattered light and transmittance front to back, into a second 3D
texture the apply pass can read at any depth.

**Apply.** `volumetric_fog.wgsl` already does the hard parts: the
fullscreen pass, the blend state, the view bindings, the depth buffer.
Replace the raymarch with a single sample of the integrated grid at the
pixel's linear depth; the rest of the file stays.

## Where it lives

In `crates/bevy-pbr-vendored`, beside the existing implementation rather
than replacing it. Everything the injection pass needs — view uniforms,
clustered light lists, the shadow atlas — is already bound as
`@group(0)` `mesh_view_bindings` for the current shader, and rebuilding
that plumbing outside the crate would be most of the work for none of
the benefit. We already vendor it for two other hunks.

The existing path stays and stays the default until this one is better,
selected by `IGNITION_FROXEL=1`. A half-built volumetric that does not
render is worse than a grainy one that does.

## What it should let us delete

`haze.rs`'s whole reason for existing is that the march is too expensive
at full resolution: a separate camera, a reduced-size target, a
composite quad, a pixel budget, occluder twins on their own render
layer. A froxel grid *is* the reduced-resolution representation, done
properly, so most of that becomes unnecessary. That is the measure of
whether this worked — not just "the beams look right" but "the
workaround is gone".

## Order

1. Grid resources and the injection pass. Compare against the current
   path on `just beam-test` with no temporal accumulation at all; it
   will be noisy, and that is expected.
2. Integration and apply. First frames that replace the raymarch.
3. Temporal reprojection. The quality win, and the part that needs
   judging with the movers running.
4. Gobos: sample the fixture's projected texture per froxel.

## Where it got to

The path renders, and on the two things it was built for it wins
outright. At 5120x1440 on the benchmark cue, with effects and mover
effects running:

    screen-space march   18.01 ms   55.5 fps   (GPU-bound)
    froxels               6.45 ms  155.1 fps

The froxel pass itself is 0.22 ms at 160x90x64 and 1.5 ms at
320x180x192, against roughly 15 ms of GPU for the march it replaces.
Every quality tier now holds 120 fps — potato 173, low 163, medium 150,
high 136, ultra 126 — where the march managed 55 at medium.

And the beams are smooth. No beading, no blocks at the apex: the
artefact that survived step count, dither, fog extent, shadow
resolution and lamp radius is simply not there, because nothing is
being stretched.

### What it cost to get there

Four bugs, in the order they were found.

**The integrate pass multiplied by optical depth instead of dividing by
the extinction coefficient.** In-scattered light is per unit length, so
the slab integral is `S/o * (1 - exp(-t))`, not `S * t`. Short by a
factor of sigma — about fifty.

**The apply pass needs `TEXTURE_BINDING` on the depth texture,** which
only the march was asking for. A camera running froxels instead of the
march got a depth texture it could not sample.

**Shadow fetches returned zero — for a missing shader def, not because
of compute.** `sample_shadow_cubemap` is a chain of `#ifdef`s on the
filter method, and with none of them defined it falls through to a
default that upstream comments as "Set to 0 to make it obvious that
something is wrong". The froxel pipeline was compiling with no shader
defs at all, so every fetch came back fully shadowed, which took the
whole medium with it. Reading the method off the view key the way
`MeshPipeline::specialize` does is the entire fix; shadows now cost
about 4 fps and a beam ends where the scenery says it does.

**A beam's bright core is thinner than a froxel,** so one sample at the
froxel's centre renders a beam as a dashed line — which is exactly what
the grid held. Four samples per froxel made it continuous, which
confirmed it; a jittered sample per frame blended against a reprojected
history does the same thing for the price of one. That is what
`history_weight` and the Halton jitter are for, and it is why every
published implementation has them.

**A phase function the march never applied.** Wash-heavy cues came out
five to fifteen times dim, which read as beams fading out halfway down.
`volumetric_fog.wgsl` applies Henyey-Greenstein to *directional* lights
only; its clustered point and spot path has no phase term at all, and
every gain in the rig — `FOG_LIGHT_GAIN` above all — was calibrated
against that. A phase term belongs there on the physics, but adding it
means recalibrating the gain against it, in both paths at once.

**Aliasing across tiles, not just slices.** Sampling each froxel at its
centre in x and y leaves a near-vertical beam dashed down its own
length, which is exactly what `beam-alias.json` was built to show.
Offsetting the sample laterally by an R2 sequence as well as along the
ray is what finally made every beam solid.

### The ladder

Width is what a beam's *sharpness* costs — the grid is sampled
trilinearly, so a tile spanning sixteen screen pixels gives a beam a
sixteen-pixel edge — and depth is what its *continuity* costs, which
`froxel_samples` buys far more cheaply. So the tiers spend sideways:

    potato  240x135x64   x1    173 fps
    low     320x180x96   x1    158 fps
    medium  400x225x96   x2    124 fps
    high    480x270x72   x2    127 fps
    ultra   640x360x96   x2     90 fps

(5120x1440, benchmark cue, gobos and shadows on, against 55 fps for the
march at medium. High is not a typo: it trades depth slices for width,
which is the better buy.)

**The march is the default.** The grid was the default for a while, on
the strength of pictures that turn out to have been riding on the
field-order bug — with `dilution` reading a flag's bit pattern as a
denormal, the fog's falloff was effectively disabled and every beam was
boosted. Fixing the bug removed the boost and revealed that the beams
were never really there. A path that is three times cheaper and wrong
is not a default; `IGNITION_FROXEL=1` selects it while it is chased.

### Gobos in the air

Done, and cheaply, because the stencils were already in the scene.
`ignition_viz::gobo` hangs a `ClusteredDecal` off every emitter, sized
to the beam where it lands, with real art out of the GDTF archive —
that is what puts the pattern on the wall. The injection now walks the
same decal cluster list, so the pattern is in the shaft as well.

Two things had to be right. A decal is an orthographic box, so its own
uv is a parallel projection and the pattern would run down the beam as
a cylinder; the box has its near face at the gate and a depth of
`DECAL_DEPTH_FACTOR` times the reach, which is enough to rescale by —
at normalised depth `z` the beam's radius in the box's own units is
`(0.5 - z) * DECAL_DEPTH_FACTOR / 2`. That is QLC+'s
`coneTopRadius`/`coneBottomRadius` arithmetic, in a space we already
had. And a stencil belongs to *one* lamp, or two beams crossing in a
froxel stripe each other with the other's pattern: a decal is matched
to its lamp geometrically, by the lamp landing at `(0, 0, 0.5)` in the
box's own space, which costs one matrix multiply per candidate and no
new per-light data.

It cost nothing measurable: 131.5 fps with gobos against 130.8 without,
at medium on the benchmark cue.

This needed Bevy's *binding-array* group — where the decal buffer and
textures live — declared `COMPUTE` as well, the same one-line change as
`VIEW_STAGES`, and bound as group 1 of the injection pipeline.

### Borrowed from grandMA3

Its 3D is closed, but its render-quality dials say a lot about how it is
built, and two of them were worth taking.

**The haze is uneven and it drifts.** MA's haze tab is particle size,
layers, blend and animation speed — a room of moving, blotchy haze
rather than one density. `FogVolume` already carried a `density_texture`
and an offset to scroll it by, and the march has always sampled them;
the froxel injection had not. It does now, at one texture fetch per
froxel and about 3 fps, and a beam brightens and fades along its length
instead of reading as a clean bar. See `ignition_viz::haze_texture`.

The trap there is worth recording: the noise has to average *one*. A
first cut stored it as `R8Unorm` centred on 0.75, which dimmed the
benchmark cue by eighteen per cent — both renderers multiply the room's
density by this texture, so any other average silently rescales the haze
and every calibration above it. A multiplier that averages one has to be
able to exceed one, which is why it is a half-float texture.

**Dilution.** MA lets the operator choose the beam's falloff — none,
linear or correct — and scale how far it stays visible.
`IGNITION_HAZE_DILUTION` is the same idea as an exponent, two being the
physical inverse square, applied to the fog alone so surfaces stay
right. It defaults to physical; it exists because a real shaft is
visible well past where its own falloff says it should be.

Not taken: their layered animated particle haze, which is what you build
when you have no volumetric integrator, and the "line" beam tier, which
is a second renderer rather than a dial.

### The regression: one uniform, two layouts

**WGSL's uniform layout rounds a `vec3` up to sixteen bytes; `encase`
packs `UVec3` into twelve.** So every field after `dimensions` was read
four bytes out of step: the shader took `near` to be 60 and `far` to be
0.3, which put every froxel at the wrong depth. Beams landed behind
walls, the apply pass correctly excluded them, and the room rendered
with no beams in it. Nothing failed and nothing logged.

The struct used to carry `jitter: f32` immediately after `dimensions`,
filling the vector's fourth lane and holding the two declarations in
step by accident. Removing it — in the same change that made the jitter
per-froxel — is what let them drift. That is why the beams vanished in
that batch and why every subsequent theory about clusters, cones,
shadows, gobos and the fog volume came back clean: they *were* clean.

The fix is structural rather than a repair. Every member of the uniform
is now a `vec4` or a `mat4x4` — sixteen bytes wide, sixteen-byte
aligned — with the scalars living in the lanes of a vector. Two
declarations built that way cannot disagree about offsets whatever
either compiler believes about packing, and
`tests/froxel_uniform_layout.rs` fails the build if a scalar or a
`vec3` is ever added.

### How it was found

Guessing a froxel from a screenshot wasted a whole pass. What worked
was a **cone map**: every froxel writes its spot attenuation, the
integrate pass takes the column maximum, and the apply pass draws it
opaque. It shows the beams plainly, which proves the cluster walk, the
cone and the froxel geometry all work — and picking the brightest pixel
in a beam's body gives a froxel worth probing.

Sampling that map at the pixel's own depth rather than down the whole
column is what exposed the depth error: the beams were there, and they
were all *behind* the visible surface.

### What it costs, honestly

The earlier figures in this document — 126 fps at medium, three times
the march — were measured while the uniform was misread, which made the
pass do far less work than it should. With it fixed, at 5120x1440 on
the benchmark cue:

    march   medium   18.0 ms    55 fps
    froxel  medium   ~14 ms     ~72 fps

Roughly a fifth faster, not three times. And the froxel pass is no
longer the frame's bottleneck: shrinking the grid from 300x170x80 to
200x112x64 moves the frame by three frames a second, because at medium
quality the *scene* costs about 10 ms before any fog at all. Reaching
120 fps at this resolution is now a question about SSR, SSAO and TAA,
not about the fog.

What the grid still buys is the picture: no beading, gobos and shadows
in the air, and a cost set by the grid rather than by the pixel count.

### What is still wrong

The beams read softer than the march's, because a 480-wide grid against
a 2560-wide picture is a 5:1 stretch however well it is filtered. More
width fixes it and costs what the table says.

The biggest remaining performance idea is also MA's: **Single Beam
Dynamic Gobo**, where a fixture with many emitters becomes one beam
whose gobo texture encodes the per-cell colour. Our injection cost
scales with lights per cluster, and a bar is many spot lights in one
cluster, all walked per froxel. With gobo sampling now in the injection
the pieces are in place, but it changes how a multi-emitter fixture is
represented and is not a change to make in a hurry.

`haze.rs` is still there. The whole reason it exists — that the march is
too expensive at full resolution — is gone, so the separate camera, the
reduced target, the composite quad, the pixel budget and the occluder
twins on their own render layer are all now scaffolding around a
fallback. Removing it is the measure this document set for itself, and
it is the work left.

## On measuring this

Two instruments wasted more time than the bugs did.

The froxel node silently does nothing until its pipelines finish
compiling, so early frames show the scene alone, and a snapshot that
catches one reads as "the grid is empty".

And the frame is tone-mapped, so a value read back out of a screenshot
is not the value the shader wrote — a linear 0.05 reads as 0.38. Worse,
the apply pass only writes alpha where there is fog, so a scene showing
through gets measured as if it were the grid.

What worked, and what to reach for first next time:

- **Binary probes.** Write 1.0 or 0.0 for a threshold test. Survives
  the tonemapper, and three channels carry three thresholds at once. A
  single loose threshold proves almost nothing — `colour > 1e-3` and
  `atten > 1e-3` were both true where the product was 1e-12.
- **Bit painting.** For an exact number, paint a float's bits as an 8x8
  cell per bit and decode the PNG. A row per depth slice reads out a
  whole froxel column in one render, exactly, and that is what finally
  showed the dashed beam in the grid.
- Remember the integrate pass accumulates: a debug value written in the
  injection arrives at the apply pass summed down the column. Pass it
  through verbatim, or read what you get as a column sum.


## Upstream

Bevy knows about both halves of this and neither is being worked on.

[#18151](https://github.com/bevyengine/bevy/issues/18151), "Physically
based unified volumetrics system", proposes very nearly what is
described above — froxel sampling, density evolution in compute passes,
an eight-frame history with temporal reprojection. Opened March 2025, no
assignee, no linked PR, no branch, "Needs SME Triage". It is a design
document, not work in flight.

[#16701](https://github.com/bevyengine/bevy/issues/16701) asks for
volumetric fog at half or quarter resolution, because the author gets
86 fps at 3440x1440 with `step_count: 256` on a 7900XTX. That is
`haze.rs`, arrived at independently by someone with the same problem —
which is some comfort about the instinct and none at all about the
timeline. No PR, no assignee.

So this is ours to build, and worth writing in a shape that could be
offered upstream if it works.
