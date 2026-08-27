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

struct FroxelGrid {
    dimensions: vec3<u32>,
    jitter: f32,
    near: f32,
    far: f32,
    scattering: f32,
    absorption: f32,
}

@group(1) @binding(0) var<uniform> grid: FroxelGrid;
@group(1) @binding(1) var scattering_grid: texture_storage_3d<rgba16float, write>;

/// The view-space depth at a froxel's centre.
///
/// Exponential, so the near froxels are the small ones. A cone has its
/// steepest gradient at its mouth, which is nearer the eye than the wall
/// behind it; linear slices would spend their precision on the wall.
fn froxel_depth(z: f32) -> f32 {
    let t = (z + 0.5 + grid.jitter) / f32(grid.dimensions.z);
    return grid.near * pow(grid.far / grid.near, t);
}

/// Henyey-Greenstein, as the screen-space march uses. Haze is forward
/// scattering: a shaft seen towards the lamp is many times the same
/// shaft seen from behind it, and that asymmetry is most of why a beam
/// reads as a beam.
fn henyey_greenstein(neg_ldotv: f32, asymmetry: f32) -> f32 {
    let denom = 1.0 + asymmetry * asymmetry - 2.0 * asymmetry * neg_ldotv;
    return (1.0 - asymmetry * asymmetry) / (4.0 * 3.14159265 * pow(max(denom, 0.0001), 1.5));
}

@compute @workgroup_size(4, 4, 4)
fn inject(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id >= grid.dimensions)) {
        return;
    }

    // The froxel's centre, in every space the lighting needs. NDC from
    // the tile, view depth from the slice, world by the inverse view.
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(grid.dimensions.xy);
    let ndc_xy = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let depth_view = froxel_depth(f32(id.z));

    // A ray through the tile's centre, walked out to the slice's depth.
    let ray_ndc = vec4(ndc_xy, 1.0, 1.0);
    var ray_view = view.view_from_clip * ray_ndc;
    ray_view /= ray_view.w;
    let direction_view = normalize(ray_view.xyz);
    // `-z` is forward in view space, so this is the point at
    // `depth_view` metres along the ray rather than along the axis.
    let position_view = direction_view * (depth_view / max(-direction_view.z, 0.0001));
    let position_world = (view.world_from_view * vec4(position_view, 1.0)).xyz;

    // Towards the eye, which is what the phase function is measured
    // against.
    let V = normalize(view.world_position - position_world);

    let thickness = froxel_depth(f32(id.z) + 1.0) - depth_view;
    let density = grid.scattering + grid.absorption;
    var scattered = vec3(0.0);

    // The clustered light list for this froxel. The cluster grid is
    // frustum-aligned and so is this one, which is the happy part: a
    // froxel is already almost a cluster, and the lookup is the same
    // one the fragment path does.
    // `view_fragment_cluster_index` wants a pixel coordinate, so the
    // froxel's tile is scaled back up to where it sits on screen.
    let frag_coord = (vec2<f32>(id.xy) + 0.5)
        * view.viewport.zw / vec2<f32>(grid.dimensions.xy);
    let cluster_index = clustering::view_fragment_cluster_index(frag_coord, -depth_view, false);
    var ranges = clustering::unpack_clusterable_object_index_ranges(cluster_index);

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
        // surface — the same clamp the screen-space march now makes.
        let lens_radius = (*light).position_radius.w;
        let distance_square = max(dot(light_to_frag, light_to_frag), lens_radius * lens_radius);
        var attenuation = getDistanceAttenuation(
            distance_square,
            (*light).color_inverse_square_range.w,
        );

        if (i < ranges.first_spot_light_index_offset) {
            if (((*light).flags & POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
                attenuation *= fetch_point_shadow_without_normal(
                    light_id,
                    vec4(position_world, 1.0),
                    vec2(0.0),
                );
            }
        } else {
            var spot_dir = vec3<f32>((*light).light_custom_data.x, 0.0, (*light).light_custom_data.y);
            spot_dir.y = sqrt(max(0.0, 1.0 - spot_dir.x * spot_dir.x - spot_dir.z * spot_dir.z));
            if (((*light).flags & POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE) != 0u) {
                spot_dir.y = -spot_dir.y;
            }
            let cd = dot(-spot_dir, L);
            let cone = saturate(cd * (*light).light_custom_data.z + (*light).light_custom_data.w);
            attenuation *= cone * cone;
            if (((*light).flags & POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
                attenuation *= fetch_spot_shadow_without_normal(
                    light_id,
                    vec4(position_world, 1.0),
                    vec2(0.0),
                );
            }
        }

        let phase = henyey_greenstein(dot(L, -V), 0.3);
        scattered += (*light).color_inverse_square_range.rgb
            * attenuation
            * phase
            * grid.scattering;
    }

    textureStore(
        scattering_grid,
        vec3<i32>(id),
        vec4(scattered, density * thickness),
    );
}
