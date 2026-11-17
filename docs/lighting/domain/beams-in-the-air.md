# Why only eight fixtures are in the air

`r[viz.haze-is-volumetric]` says every emitter lights the fog: "with the
hazers off nothing shows in the air at all, and as the haze builds every
fixture's light appears in it, pars and movers alike". `spawn.rs` marks
all sixty-seven spill lights `VolumetricLight` accordingly.

It is not true as built, and the reason is one line of Bevy.

## The rule

`bevy_light`'s own documentation for the component:

> Add this component to a light **with a shadow map
> (`shadow_maps_enabled: true`)** to make volumetric fog interact with it.

And `bevy_pbr/src/render/light.rs`, where the GPU flag is set:

```rust
if light.shadow_maps_enabled
    && light.volumetric
    && (index < point_light_volumetric_enabled_count || …)
{
    flags |= PointLightFlags::VOLUMETRIC;
}
```

`volumetric_fog.wgsl` then skips any light without that flag outright:

```wgsl
if (((*light).flags & POINT_LIGHT_FLAGS_VOLUMETRIC_BIT) == 0) { continue; }
```

So a light is in the fog only if it has a shadow map. `budget.rs` gives
eight of them, on purpose, because a shadow map is a render pass per
light per frame. Therefore **eight fixtures are in the air and
fifty-nine are not**, and `bench`'s "volumetric lights 67 / shadowed
spots 8" has been saying so all along.

The count is capped twice over: the number of volumetric spot lights is
`min(max_texture_array_layers - directional_shadow_enabled_count *
MAX_CASCADES_PER_LIGHT)`, so it is bounded by shadow-map array capacity
even if every light asked for one.

## What that explains

**A shaft is a shadow map.** Not lit by one — made of one. What the map
cannot resolve becomes structure in the beam, and that structure is
untouchable from the fog's side. It is why the beading in a steep beam
does not respond to the step count (128 and 256 are indistinguishable to
the eye), to the dither (0 and 0.3 the same), or to the haze camera's
resolution. None of those are the shadow map. Unreal documents the same
coupling: "the resolution of the volumetric light shaft is directly
related to the resolution of the shadow map of that light".

**Turning shadows off removes the beam entirely**, rather than merely
removing silhouettes from it. `IGNITION_SHADOWS=0` shows this in one
launch: light still lands on the wall, and there is nothing in the air.
That is the rule above, seen from the other side.

Raising the map to 4096 (`IGNITION_SHADOW_MAP=4096`) does not fix the
beading either, which says the artefact is in *how* the map is sampled —
depth precision and bias along the ray — rather than in how many texels
it has. That is the next thing to look at.

## What it would take

The gate is Bevy's, not physics'. The shader already defaults
`shadow = 1.0` for a light without a map, so a shadowless light would
scatter correctly if it were flagged. Removing `shadow_maps_enabled &&`
from that condition, and decoupling the volumetric count from shadow-map
array capacity, would put every par in the air **at no shadow cost at
all** — which is both what the spec says and what the operator asked
for.

That means patching `bevy_pbr`: a large crate to vendor, and a
commitment carried across every Bevy bump. A render-world fixup is not
an alternative — `ExtractedPointLight`'s fields are public, but the caps
above are applied inside `prepare_lights`, so setting
`shadow_maps_enabled` from outside only asks for the shadow maps we are
trying not to pay for.

The same patch would settle the beading question, because it makes the
decisive experiment possible for the first time: a beam from a light
with **no** shadow map has no shadow-map sampling in it at all. If those
beams are clean, the artefact is the map; if they are not, it never was.

## Reproducing

    IGNITION_SHADOWS=0     just profile-bench    # no shadow maps: no beams at all
    IGNITION_SHADOW_MAP=4096 just profile-bench  # four times the texels, same beading
    just beam-test                               # the steep-beam picture itself
