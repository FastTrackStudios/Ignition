// IGNITION PATCH: froxel volumetrics — the apply pass.
//
// A fullscreen triangle that reads the integrated grid at each pixel's
// own depth and blends the result over the frame. No raymarch: the
// walking was done once per column by the integrate pass, so what is
// left here is a single filtered sample.
//
// That is the whole saving. The screen-space march this replaces walks
// the clustered light list once per pixel per step; this reads a texture.
//
// See docs/domain/froxel-volumetrics.md.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import bevy_render::view::View

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

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> grid: FroxelGrid;
@group(0) @binding(2) var integrated_grid: texture_3d<f32>;
@group(0) @binding(3) var grid_sampler: sampler;
@group(0) @binding(4) var depth_texture: texture_depth_2d;

/// Where a view-space depth sits along the grid's exponential slices,
/// as a 0..1 coordinate. The inverse of the injection's `froxel_depth`.
fn depth_to_slice(depth_view: f32) -> f32 {
    return log(depth_view / grid.range.x) / log(grid.range.y / grid.range.x);
}

@fragment
fn apply(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(in.position.xy);
    let depth_ndc = textureLoad(depth_texture, texel, 0);

    // How far along the ray this pixel's surface is. With a reversed-Z
    // projection an empty pixel reads 0, which is infinitely far — and
    // "infinitely far" is exactly the far end of the grid.
    var depth_view: f32;
    if (depth_ndc <= 0.0) {
        depth_view = grid.range.y;
    } else {
        let view_pos = view.view_from_clip * vec4(0.0, 0.0, depth_ndc, 1.0);
        depth_view = -view_pos.z / view_pos.w;
    }

    // Clamped rather than discarded: a surface nearer than the grid
    // starts has no fog in front of it, and one beyond the grid's end
    // gets everything the grid accumulated, which is the correct answer
    // in both directions.
    let slice = clamp(depth_to_slice(depth_view), 0.0, 1.0);

    // Filtered in all three axes: the whole reason a grid this coarse
    // reads smoothly is that it is interpolated rather than stepped.
    let sample = textureSampleLevel(
        integrated_grid,
        grid_sampler,
        vec3(in.uv, slice),
        0.0,
    );

    // `sample.a` is transmittance — what survives the fog — so the
    // fraction the fog itself contributes is one minus it. The pipeline
    // blends `src + dst * (1 - src_alpha)`, which is exactly this.
    return vec4(sample.rgb, 1.0 - sample.a);
}
