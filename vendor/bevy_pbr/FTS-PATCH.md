# Vendored `bevy_pbr`: Bevy `main` at 948cc8270, plus two commits

`crates/bevy_pbr` of bevyengine/bevy at
`948cc8270204b46fd37353eb786144a677d0c0ce` (the workspace's Bevy pin,
root `Cargo.toml`), with the two commits in `patches/` applied. The root
manifest's `[patch."https://github.com/bevyengine/bevy"]` puts it in
place of the revision's own `bevy_pbr`, for every Bevy crate in the tree.

It stands in for a fork. There is no `FastTrackStudios/bevy` yet; when
there is, its branch is upstream at that revision plus `patches/*.patch`
(`git am`), Ignition depends on it by tag, and this directory and its
patch entry are deleted. `docs/lighting/research/bevy-pin.md` has the
whole list of what the fork carries and why the revision was chosen.

Both commits are in the volumetric fog, which the analytic beam renderer
replaces; they go when the fog does.

## 1. A point or spot light may be volumetric without a shadow map

`src/render/light.rs`. Upstream sets the GPU `VOLUMETRIC` flag only when
`shadow_maps_enabled && volumetric`, and the fog's shader skips a light
without the flag, so "is in the haze" and "casts a shadow" are one
switch. A shadow map is a render pass per light per frame and
`budget.rs` affords twelve; a rig has sixty-odd emitters, all of them in
the air. The flag no longer asks for a shadow map, and the two
volumetric counts are no longer capped by the shadow maps' capacity. The
shader already takes `shadow = 1.0` for a light with no map.

## 2. The inverse square stops at the light's radius

`src/volumetric_fog/volumetric_fog.wesl`. Upstream takes the raw
distance to a clustered light, so radiance runs to infinity at the
light's position: in fog, a singularity where a beam leaves its fixture.
The squared distance is clamped to the light's radius, which Bevy
already uploads in `position_radius.w`. A no-op at radius zero, the
default (`spawn::DEFAULT_LENS_RADIUS_M`).

## What was not carried from the 0.19.1 fork

The froxel volumetrics (`froxel.rs`, three shaders, the extracted
`volumetric_shadows.wgsl`) and the view bindings' compute visibility
that served them. They were opt-in (`IGNITION_FROXEL=1`), and the fog is
being replaced; `IGNITION_FROXEL` is now ignored with a warning.

## Moving the pin

Copy `crates/bevy_pbr/{src,README.md,LICENSE-*}` of the new revision
over this directory, apply `patches/*.patch` (`git apply --directory
vendor/bevy_pbr -p3`), and regenerate `Cargo.toml` from upstream's: its
path dependencies on the other Bevy crates become `git` + `rev`
dependencies at the new revision, and `[lints] workspace = true` and the
`wgsl-analyzer` metadata are dropped.
