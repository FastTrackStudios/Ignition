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
