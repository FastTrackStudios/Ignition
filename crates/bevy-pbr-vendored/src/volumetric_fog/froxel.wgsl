// IGNITION PATCH: froxel volumetrics — the injection pass.
//
// One invocation per froxel of a frustum-aligned voxel grid: X and Y are
// screen tiles, Z is depth slices. Each froxel is lit *once* — the same
// clustered light walk, spot attenuation and shadow fetch the
// screen-space march runs, but once per froxel rather than once per
// pixel per step — and the result is written as in-scattered light and
// extinction for the integrate pass to accumulate.
//
// Group 0 is Bevy's own view bind group, unchanged, which is what makes
// this the *same* lighting rather than a second implementation of it.
// It is bound to a compute pipeline only because
// `render/mesh_view_bindings.rs` now declares those bindings visible to
// compute; upstream they are fragment-only.
//
// See docs/domain/froxel-volumetrics.md.

#import bevy_pbr::mesh_view_bindings::{clustered_lights, view}
#import bevy_pbr::mesh_view_types::{
    POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE,
    POINT_LIGHT_FLAGS_VOLUMETRIC_BIT,
}
#import bevy_pbr::volumetric_shadows::{fetch_point_shadow_without_normal, fetch_spot_shadow_without_normal}
#import bevy_pbr::clustered_forward as clustering
#import bevy_pbr::lighting::getDistanceAttenuation
#ifdef CLUSTERED_DECALS_ARE_USABLE
#import bevy_pbr::mesh_view_bindings::{clustered_decals, clustered_decal_textures, clustered_decal_sampler}
#endif

struct FroxelGrid {
    // Every member is sixteen bytes wide and sixteen-byte aligned, so
    // the Rust declaration and this one cannot disagree about offsets.
    // They did once — WGSL's uniform layout rounds a `vec3` up to
    // sixteen bytes while `encase` packs `UVec3` into twelve — and the
    // shader then read `near` as `far` and `far` as `scattering`, which
    // put every froxel at the wrong depth and rendered a room with no
    // beams in it. Scalars live in the lanes of a vector now.
    dimensions: vec4<u32>,
    range: vec4<f32>,
    medium: vec4<f32>,
    flags: vec4<f32>,
    density_offset: vec4<f32>,
    prev_clip_from_world: mat4x4<f32>,
    uvw_from_world: mat4x4<f32>,
}

@group(2) @binding(0) var<uniform> grid: FroxelGrid;
@group(2) @binding(1) var scattering_grid: texture_storage_3d<rgba16float, write>;
@group(2) @binding(2) var history_grid: texture_3d<f32>;
@group(2) @binding(3) var history_sampler: sampler;
@group(2) @binding(4) var density_texture: texture_3d<f32>;
@group(2) @binding(5) var density_sampler: sampler;

/// The haze at a point, as a multiple of the room's average.
///
/// Real haze is neither uniform nor still: it hangs in banks and it
/// drifts. `FogVolume` carries a density texture and an offset to
/// scroll it by for exactly this, and the march has always sampled
/// them — the froxel path simply had not. One texture fetch per
/// froxel, which is nothing beside the light walk it sits in.
fn density_at(world_position: vec3<f32>) -> f32 {
    let uvw = (grid.uvw_from_world * vec4(world_position, 1.0)).xyz;

    // Outside the fog volume there is no haze. The march gets this for
    // free by rasterising the volume's hull and marching only inside
    // it; a grid covers the whole frustum, so without this the room
    // washes out with haze from above the ceiling and behind the
    // walls — which is exactly what `beam-tests.json` showed.
    if (u32(grid.flags.z) != 0u
        && (any(uvw < vec3(0.0)) || any(uvw > vec3(1.0)))) {
        return 0.0;
    }

    if (u32(grid.flags.y) == 0u) {
        return 1.0;
    }
    return textureSampleLevel(
        density_texture,
        density_sampler,
        uvw + grid.density_offset.xyz,
        0.0,
    ).r;
}

/// Where a view-space depth sits along the grid's slices, 0..1 — the
/// inverse of `froxel_depth`, for finding a froxel in the history.
fn depth_to_slice(depth_view: f32) -> f32 {
    return log(depth_view / grid.range.x) / log(grid.range.y / grid.range.x);
}

/// The view-space depth at a froxel's centre.
///
/// Exponential, so the near froxels are the small ones. A cone has its
/// steepest gradient at its mouth, which is nearer the eye than the
/// wall behind it; linear slices would spend their precision on the
/// wall.
fn froxel_depth(z: f32) -> f32 {
    let t = (z + 0.5) / f32(grid.dimensions.z);
    return grid.range.x * pow(grid.range.y / grid.range.x, t);
}

/// The gobos covering this point, gathered once per froxel.
///
/// The stencils are already in the scene as clustered decals, hung off
/// each emitter and sized to the beam where it lands (see
/// `ignition_viz::gobo`) — the wall gets its pattern from exactly this
/// data. The fog has simply never read it, so a shaft stayed blank
/// while the floor under it carried the pattern. This is what QLC+'s
/// scattering shader does with `goboMask`, in the space we already
/// have.
///
/// Gathered rather than applied, because a stencil belongs to *one*
/// lamp: two beams crossing in the same froxel must not stripe each
/// other with the other's pattern.
struct FroxelGobos {
    count: u32,
    decals: array<u32, 4>,
    transmission: array<f32, 4>,
}

fn gather_gobos(world_position: vec3<f32>, ranges: clustering::ClusterableObjectIndexRanges) -> FroxelGobos {
    var gobos: FroxelGobos;
    gobos.count = 0u;
#ifdef CLUSTERED_DECALS_ARE_USABLE
    var i = ranges.first_decal_offset;
    while (i < ranges.last_clusterable_object_index_offset && gobos.count < 4u) {
        let decal_index = clustering::get_clusterable_object_id(i);
        let local = (clustered_decals.decals[decal_index].local_from_world
            * vec4(world_position, 1.0)).xyz;
        if (all(local >= vec3(-0.5)) && all(local <= vec3(0.5))) {
            // A decal is an orthographic box, so its own uv is a
            // parallel projection — the pattern would run down the beam
            // as a cylinder rather than a cone converging on the gate.
            // The box has its near face at the gate and a depth of
            // `DECAL_DEPTH_FACTOR` times the beam's reach, which is
            // enough to rescale by: at normalised depth `z` the beam's
            // radius in the box's own units is
            // `(0.5 - z) * DECAL_DEPTH_FACTOR / 2`. 0.75 is that
            // factor halved.
            let radius = max((0.5 - local.z) * 0.75, 1e-4);
            let uv = vec2(local.x, -local.y) / radius * 0.5 + 0.5;
            if (all(uv >= vec2(0.0)) && all(uv <= vec2(1.0))) {
                let texture_index =
                    clustered_decals.decals[decal_index].base_color_texture_index;
                let stencil = textureSampleLevel(
                    clustered_decal_textures[texture_index],
                    clustered_decal_sampler,
                    uv,
                    0.0,
                );
                // The decal paints black with alpha where the gobo is
                // *closed*, so what gets through is one minus it.
                gobos.decals[gobos.count] = decal_index;
                gobos.transmission[gobos.count] = 1.0 - stencil.a;
                gobos.count = gobos.count + 1u;
            }
        }
        i = i + 1u;
    }
#endif
    return gobos;
}

/// How much of *this lamp's* light survives the gobos here.
///
/// A decal is matched to its lamp geometrically: the box's near face is
/// the gate, so a lamp whose position lands at `(0, 0, 0.5)` in the
/// box's own space is the lamp that stencil belongs to. That costs one
/// matrix multiply per candidate and needs no new per-light data.
fn gobo_for_light(gobos: FroxelGobos, light_position: vec3<f32>) -> f32 {
    var transmission = 1.0;
#ifdef CLUSTERED_DECALS_ARE_USABLE
    for (var k = 0u; k < gobos.count; k = k + 1u) {
        let gate = (clustered_decals.decals[gobos.decals[k]].local_from_world
            * vec4(light_position, 1.0)).xyz;
        if (abs(gate.z - 0.5) < 0.05 && length(gate.xy) < 0.05) {
            transmission *= gobos.transmission[k];
        }
    }
#endif
    return transmission;
}


@compute @workgroup_size(4, 4, 4)
fn inject(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id >= grid.dimensions.xyz)) {
        return;
    }

    // A beam's bright core is thinner than a froxel is deep, so one
    // sample at the centre renders a beam as a dashed line. The
    // temporal blend fixes that over several frames; taking more than
    // one sample fixes it in the frame itself, which is what a still
    // needs and what the cheapness of this pass affords.
    let count = max(u32(grid.flags.x), 1u);
    var summed = vec3(0.0);
    var extinction = 0.0;
    var world = vec3(0.0);
    var depth = 0.0;
    for (var s = 0u; s < count; s = s + 1u) {
        let one = inject_sample(id, sample_offset(id, s, count));
        summed += one.scattered;
        extinction = one.extinction;
        world = one.position_world;
        depth = one.depth_view;
    }
    let scattered = summed / f32(count);
    let position_world = world;
    let depth_view = depth;

    // Blend with last frame's grid, reprojected through the camera's
    // motion. One jittered sample a frame is not enough to resolve a
    // beam whose core is thinner than a froxel; several frames of them
    // are. Where the reprojection lands outside the grid there is no
    // history to use, and this frame's sample stands alone.
    var blended = scattered;
    let prev_clip = grid.prev_clip_from_world * vec4(position_world, 1.0);
    if (grid.medium.w > 0.0 && prev_clip.w > 0.0) {
        let prev_ndc = prev_clip.xyz / prev_clip.w;
        let prev_uv = vec2(prev_ndc.x * 0.5 + 0.5, 0.5 - prev_ndc.y * 0.5);
        // `w` of a perspective clip position is the view-space depth,
        // so this is where the froxel sat in *last* frame's grid, not
        // where it sits in this one.
        let prev_slice = depth_to_slice(prev_clip.w);
        if (all(prev_uv >= vec2(0.0)) && all(prev_uv <= vec2(1.0))
            && prev_slice >= 0.0 && prev_slice <= 1.0) {
            let history = textureSampleLevel(
                history_grid,
                history_sampler,
                vec3(prev_uv, prev_slice),
                0.0,
            );
            blended = mix(scattered, history.rgb, grid.medium.w);
        }
    }

    textureStore(
        scattering_grid,
        vec3<i32>(id),
        vec4(blended, extinction),
    );
}

struct FroxelSample {
    scattered: vec3<f32>,
    extinction: f32,
    position_world: vec3<f32>,
    depth_view: f32,
}

/// Three decorrelated values in 0..1 for a froxel on a frame.
fn froxel_noise(id: vec3<u32>, frame: u32) -> vec3<f32> {
    var h = id.x * 73856093u ^ id.y * 19349663u ^ id.z * 83492791u ^ frame * 2654435761u;
    h = h ^ (h >> 13u);
    h = h * 1274126177u;
    let a = h ^ (h >> 16u);
    let b = (h * 2246822519u) ^ ((h * 2246822519u) >> 15u);
    let c = (h * 3266489917u) ^ ((h * 3266489917u) >> 17u);
    return vec3(
        f32(a & 0xffffffu) / f32(0xffffffu),
        f32(b & 0xffffffu) / f32(0xffffffu),
        f32(c & 0xffffffu) / f32(0xffffffu),
    );
}

/// Where inside the froxel a sample sits: stratified along the ray by
/// the sample index, and moved every frame so the history has
/// something new to average.
///
/// The offset is drawn **per froxel**, and that is the whole point. A
/// single per-frame offset added to every froxel — which is what this
/// was — moves the entire lit field together, and the history blend
/// turns that coherent motion into the room visibly swimming in a
/// small circle. Neighbouring froxels have to move in different
/// directions for the jitter to average out instead of showing up as
/// drift.
///
/// The lateral half matters as much as the depth half. A beam standing
/// near-vertical in front of the house crosses many tiles and only a
/// slice or two of depth in each, so sampling every tile at its centre
/// leaves the beam dashed down its own length — which is the artefact
/// `beam-alias.json` exists to show.
fn sample_offset(id: vec3<u32>, s: u32, count: u32) -> vec3<f32> {
    let n = froxel_noise(id, grid.dimensions.w + s * 977u);
    let z = (f32(s) + n.z) / f32(count) - 0.5;
    return vec3(n.xy - 0.5, z);
}

/// One sample inside a froxel, offset from its centre in tiles and
/// slices.
fn inject_sample(id: vec3<u32>, offset: vec3<f32>) -> FroxelSample {
    // The froxel's centre, in every space the lighting needs. NDC from
    // the tile, view depth from the slice, world by the inverse view.
    let uv = (vec2<f32>(id.xy) + 0.5 + offset.xy) / vec2<f32>(grid.dimensions.xy);
    let ndc_xy = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let depth_view = froxel_depth(f32(id.z) + offset.z);

    // A ray through the tile's centre, walked out to the slice's depth.
    let ray_ndc = vec4(ndc_xy, 1.0, 1.0);
    var ray_view = view.view_from_clip * ray_ndc;
    ray_view /= ray_view.w;
    let direction_view = normalize(ray_view.xyz);
    // `-z` is forward in view space, so this is the point at
    // `depth_view` metres along the ray rather than along the axis.
    let position_view = direction_view * (depth_view / max(-direction_view.z, 0.0001));
    let position_world = (view.world_from_view * vec4(position_view, 1.0)).xyz;

    // The ray this froxel sits on, pointing away from the eye — the
    // same `Rd_world` the screen-space march measures its phase
    // against. Getting this sign wrong turns a forward-scattering haze
    // into a backward-scattering one.
    let V = normalize(position_world - view.world_position);

    let thickness = froxel_depth(f32(id.z) + 1.0) - froxel_depth(f32(id.z));
    // Sampled once, and used for both what the air scatters back and
    // what it takes out: a thin patch is thin in both directions.
    let local_density = density_at(position_world);
    let density = (grid.range.z + grid.range.w) * grid.medium.x * local_density;
    var scattered = vec3(0.0);

    // The clustered light list for this froxel. The cluster grid is
    // frustum-aligned and so is this one, which is the happy part: a
    // froxel is already almost a cluster, and the lookup is the same
    // one the fragment path does.
    let frag_coord = (vec2<f32>(id.xy) + 0.5)
        * view.viewport.zw / vec2<f32>(grid.dimensions.xy);
    let cluster_index = clustering::view_fragment_cluster_index(frag_coord, -depth_view, false);
    var ranges = clustering::unpack_clusterable_object_index_ranges(cluster_index);

    // The gobos in the air, not just on the wall.
    let gobos = gather_gobos(position_world, ranges);

    // What follows is `volumetric_fog.wgsl`'s own per-light block,
    // transplanted rather than re-derived: the froxel path has to agree
    // with the march it replaces, and the only honest way to guarantee
    // that is to run the same arithmetic on the same inputs. The one
    // difference is that this is in-scattered light *per unit length* —
    // the march multiplies by its step here, and the integrate pass
    // multiplies by the froxel's own depth instead.
    for (var i = ranges.first_point_light_index_offset;
         i < ranges.first_reflection_probe_index_offset;
         i = i + 1u) {
        let light_id = clustering::get_clusterable_object_id(i);
        let light = &clustered_lights.data[light_id];
        if (((*light).flags & POINT_LIGHT_FLAGS_VOLUMETRIC_BIT) == 0u) {
            continue;
        }

        let light_to_frag = (*light).position_radius.xyz - position_world;
        let L = normalize(light_to_frag);
        // The lamp has a lens, so the inverse square stops at its
        // surface — the same clamp the march now makes.
        let lens_radius = (*light).position_radius.w;
        let distance_square = max(dot(light_to_frag, light_to_frag), lens_radius * lens_radius);
        var distance_atten = getDistanceAttenuation(
            distance_square,
            (*light).color_inverse_square_range.w,
        );
        // Dilution: the fog's falloff exponent, two being the physical
        // inverse square. `getDistanceAttenuation` has already divided
        // by the distance squared, so carrying the beam further is a
        // matter of multiplying part of that back.
        if (grid.flags.w < 2.0) {
            distance_atten *= pow(distance_square, (2.0 - grid.flags.w) * 0.5);
        }
        var local_light_attenuation = distance_atten;

        if (i < ranges.first_spot_light_index_offset) {
            // Shadowed like the march: a beam ends where the scenery
            // it meets says it does.
            if (((*light).flags & POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
                local_light_attenuation *= fetch_point_shadow_without_normal(
                    light_id,
                    vec4(position_world, 1.0),
                    vec2(0.0),
                );
            }
        } else {
            // Reconstruct the spot direction from x/z and the y-sign
            // flag, exactly as the fragment path does.
            var spot_dir = vec3<f32>((*light).light_custom_data.x, 0.0, (*light).light_custom_data.y);
            spot_dir.y = sqrt(max(0.0, 1.0 - spot_dir.x * spot_dir.x - spot_dir.z * spot_dir.z));
            if (((*light).flags & POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE) != 0u) {
                spot_dir.y = -spot_dir.y;
            }
            let cd = dot(-spot_dir, L);
            let spot_attenuation = saturate(cd * (*light).light_custom_data.z + (*light).light_custom_data.w);
            local_light_attenuation *= spot_attenuation * spot_attenuation;

            if (((*light).flags & POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
                local_light_attenuation *= fetch_spot_shadow_without_normal(
                    light_id,
                    vec4(position_world, 1.0),
                    vec2(0.0),
                );
            }
        }

        // No phase function here, because the march has none for
        // clustered lights: `volumetric_fog.wgsl` applies
        // Henyey-Greenstein to directional lights only, and every gain
        // in the rig — `FOG_LIGHT_GAIN` above all — was calibrated
        // against that. Multiplying spots by a phase term the march
        // never applied made the froxel picture five to fifteen times
        // dim, which read as beams fading out halfway down.
        //
        // A phase term belongs here on the physics, and a beam seen
        // towards the lamp really is brighter than one seen from
        // behind. Adding it means recalibrating the gain against it,
        // and doing that to both paths at once — not to this one alone.
        scattered += (*light).color_inverse_square_range.rgb
            * local_light_attenuation
            * gobo_for_light(gobos, (*light).position_radius.xyz)
            * grid.range.z
            * grid.medium.x
            * local_density
            * grid.medium.y
            * view.exposure;
    }


    return FroxelSample(scattered, density * thickness, position_world, depth_view);
}
