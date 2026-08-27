# Vendored `bevy_pbr` 0.19.1

A copy of the published crate, redirected by `[patch.crates-io]` in the
root `Cargo.toml` so every `bevy_pbr` in the tree — including the one
`bevy_internal` pulls in — resolves here. Nothing else about the Bevy
dependency changes.

Two hunks, both in the volumetric fog, both marked `IGNITION PATCH`.
Neither is a tuning knob: each is a place where upstream's behaviour is
reasonable for a game and wrong for a lighting visualiser.

## A light may scatter without casting a shadow

`src/render/light.rs`. Upstream sets the GPU `VOLUMETRIC` flag only when
`shadow_maps_enabled && volumetric`, and `volumetric_fog.wgsl` skips any
light without the flag — so "appears in the haze" and "casts a shadow"
are the same switch. A shadow map is a render pass per light per frame;
`budget.rs` affords eight. So of sixty-seven emitters marked
`VolumetricLight`, eight were in the air and fifty-nine were not, which
is neither what `r[viz.haze-is-volumetric]` says nor what a hazed room
looks like.

Nothing else had to change: the shader already defaults `shadow = 1.0`
for a light whose shadow bit is clear, so a shadowless light scatters
correctly and simply silhouettes nobody. The two volumetric counts are
no longer clamped to shadow-map array capacity either, for the same
reason — a light that wants no map should not compete for one.

Every fixture is now in the air, at no shadow cost. It costs frame rate
in the fog loop instead: about 80 fps to 50 in the operator's layout on
the reference machine, which is the trade an operator would actually
choose to make.

See `docs/domain/beams-in-the-air.md`.

## The inverse square may stop at the lamp's own radius

`src/volumetric_fog/volumetric_fog.wgsl`. Upstream takes the raw
distance from the light, so radiance runs to infinity at the light's
position. On a surface that is invisible — nothing is ever *at* a light.
In fog it is a singularity in the middle of the picture, and Bevy
already uploads the radius that bounds it in `position_radius.w`; the
fog is simply the one place that ignores it.

The clamp is the standard sphere-light form and is a no-op at radius
zero, which is the default (`spawn::DEFAULT_LENS_RADIUS_M`). It was
written to test whether a real lens diameter — a beam leaves a fixture a
hundred and fifty millimetres across, not at a point — would fix the
blocky cone apexes. **It does not**; the apexes are the haze buffer's
resolution, which is ours and not Bevy's. The hunk stays because the
singularity is real and the clamp is correct, and because GDTF's
`BeamRadius` makes a per-fixture lens cheap to wire up if the look is
ever wanted.

## Keeping it

Re-vendor on a Bevy bump: copy the published crate, drop
`Cargo.toml.orig` and `Cargo.lock`, restore the header and
`publish = false`, and re-apply the three marked hunks (`grep -rn
"IGNITION PATCH"`). They are small and local; if either lands upstream,
delete the patch entry and the directory.
