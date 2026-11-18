// IGNITION PATCH: the volumetric shadow fetches, extracted so more than
// one pass can use them.
//
// These were private helpers inside `volumetric_fog.wgsl`. The froxel
// injection needs exactly the same sampling — a fog sample has no
// surface and therefore no normal, which is what makes these different
// from `bevy_pbr::shadows` — and two copies of a shadow lookup that must
// agree is how a beam ends up lit differently depending on which pass
// drew it.

#define_import_path bevy_pbr::volumetric_shadows

#import bevy_pbr::mesh_view_bindings::{clustered_lights, lights}
#import bevy_pbr::mesh_view_types::POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE
#import bevy_pbr::shadow_sampling::{
    sample_shadow_cubemap,
    sample_shadow_map,
    SPOT_SHADOW_TEXEL_SIZE,
}
#import bevy_render::maths::orthonormalize

fn fetch_point_shadow_without_normal(light_id: u32, frag_position: vec4<f32>, frag_coord_xy: vec2<f32>) -> f32 {
    let light = &clustered_lights.data[light_id];

    // because the shadow maps align with the axes and the frustum planes are at 45 degrees
    // we can get the worldspace depth by taking the largest absolute axis
    let surface_to_light = (*light).position_radius.xyz - frag_position.xyz;
    let surface_to_light_abs = abs(surface_to_light);
    let distance_to_light = max(surface_to_light_abs.x, max(surface_to_light_abs.y, surface_to_light_abs.z));

    // The normal bias here is already scaled by the texel size at 1 world unit from the light.
    // The texel size increases proportionally with distance from the light so multiplying by
    // distance to light scales the normal bias to the texel size at the fragment distance.
    let depth_offset = (*light).shadow_depth_bias * normalize(surface_to_light.xyz);
    let offset_position = frag_position.xyz + depth_offset;

    // similar largest-absolute-axis trick as above, but now with the offset fragment position
    let frag_ls = offset_position.xyz - (*light).position_radius.xyz;
    let abs_position_ls = abs(frag_ls);
    let major_axis_magnitude = max(abs_position_ls.x, max(abs_position_ls.y, abs_position_ls.z));

    // NOTE: These simplifications come from multiplying:
    // projection * vec4(0, 0, -major_axis_magnitude, 1.0)
    // and keeping only the terms that have any impact on the depth.
    // Projection-agnostic approach:
    let zw = -major_axis_magnitude * (*light).light_custom_data.xy + (*light).light_custom_data.zw;
    let depth = zw.x / zw.y;

    // Do the lookup, using HW PCF and comparison. Cubemaps assume a left-handed coordinate space,
    // so we have to flip the z-axis when sampling.
    let flip_z = vec3(1.0, 1.0, -1.0);
    return sample_shadow_cubemap(frag_ls * flip_z, distance_to_light, depth, light_id, frag_coord_xy);
}

fn fetch_spot_shadow_without_normal(light_id: u32, frag_position: vec4<f32>, frag_coord_xy: vec2<f32>) -> f32 {
    let light = &clustered_lights.data[light_id];

    let surface_to_light = (*light).position_radius.xyz - frag_position.xyz;

    // construct the light view matrix
    var spot_dir = vec3<f32>((*light).light_custom_data.x, 0.0, (*light).light_custom_data.y);
    // reconstruct spot dir from x/z and y-direction flag
    spot_dir.y = sqrt(max(0.0, 1.0 - spot_dir.x * spot_dir.x - spot_dir.z * spot_dir.z));
    if (((*light).flags & POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE) != 0u) {
        spot_dir.y = -spot_dir.y;
    }

    // view matrix z_axis is the reverse of transform.forward()
    let fwd = -spot_dir;
    let offset_position =
        -surface_to_light
        + ((*light).shadow_depth_bias * normalize(surface_to_light));

    let light_inv_rot = orthonormalize(fwd);

    // because the matrix is a pure rotation matrix, the inverse is just the transpose, and to calculate
    // the product of the transpose with a vector we can just post-multiply instead of pre-multiplying.
    // this allows us to keep the matrix construction code identical between CPU and GPU.
    let projected_position = offset_position * light_inv_rot;

    // divide xy by perspective matrix "f" and by -projected.z (projected.z is -projection matrix's w)
    // to get ndc coordinates
    let f_div_minus_z = 1.0 / ((*light).spot_light_tan_angle * -projected_position.z);
    let shadow_xy_ndc = projected_position.xy * f_div_minus_z;
    // convert to uv coordinates
    let shadow_uv = shadow_xy_ndc * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);

    // 0.1 must match POINT_LIGHT_NEAR_Z
    let depth = 0.1 / -projected_position.z;

    return sample_shadow_map(
        shadow_uv,
        depth,
        i32(light_id) + lights.spot_light_shadowmap_offset,
        frag_coord_xy,
        SPOT_SHADOW_TEXEL_SIZE
    );
}
