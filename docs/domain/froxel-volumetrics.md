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

Everyone else writes this with compute shaders. We cannot, and the
constraint improves the design.

Bevy declares its view bind group — the view uniform, the clustered
light lists, the shadow atlas, everything the injection needs — with
`ShaderStages::FRAGMENT`. A compute pipeline cannot bind it. We could
patch that visibility, since we vendor the crate, but it is a central
file and a permissive change to a layout every pipeline in the engine
shares is a poor thing to carry across Bevy bumps for one feature.

So: **fragment passes, and the grid is a 2D texture with its Z slices
tiled across it.** That is an old technique and it costs nothing here —
a froxel's address is arithmetic either way.

    inject     grid, RGB = in-scattered light, A = extinction, one
               fragment per froxel, over a tiled 2D target
    reproject  blend with last frame's grid, offset by camera motion
    apply      per pixel, walk Z up to the pixel's depth, accumulating

**There is no integrate pass**, and dropping it is the second thing the
constraint bought. A separate prefix-sum along Z exists to save the
apply pass from walking the grid; but walking it is now sixty-four
texture reads of a grid that is *already lit*, against the current
march's sixty-four iterations of the whole clustered light loop. The
expensive part was never the walking.

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

**Apply.** `volumetric_fog.wgsl` already does the hard parts: the
fullscreen pass, the blend state, the view bindings, the depth buffer.
Replace the *body* of the raymarch loop — the clustered light walk, the
shadow fetches, the phase function — with a read of the grid, and the
loop structure, the front-to-back accumulation and the rest of the file
stay as they are.

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
