// IGNITION PATCH: froxel volumetrics — the injection pass.
//
// One invocation per froxel of a frustum-aligned voxel grid: X and Y
// are screen tiles, Z is depth slices. Writes in-scattered light and
// extinction into a 3D storage texture, which the integrate pass then
// walks front to back and the apply pass samples at a pixel's depth.
//
// Compute, over our own bind group. Bevy's *own* view bind group is
// declared `ShaderStages::FRAGMENT` and so cannot be bound here, but
// visibility belongs to the layout, not to the buffers — the lights,
// the clusters and the shadow atlas are all reachable and are bound
// below with `ShaderStages::COMPUTE`.
//
// See docs/domain/froxel-volumetrics.md.

#import bevy_render::view::View

struct FroxelGrid {
    // Froxels across, up, and deep.
    dimensions: vec3<u32>,
    // Frames advance this so the sample point inside each froxel moves,
    // which is what the temporal pass turns into resolution.
    jitter: f32,
    // Where the grid starts and stops in view space, in metres. Near is
    // not the camera's near plane: a grid that begins at 0.1 m spends
    // most of its depth on air nobody is looking through.
    near: f32,
    far: f32,
    // The medium, matching `FogVolume`.
    scattering: f32,
    absorption: f32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> grid: FroxelGrid;
@group(0) @binding(2) var scattering_grid: texture_storage_3d<rgba16float, write>;

// The view-space depth at a froxel's centre.
//
// Exponential, so near froxels are small. A cone has its steepest
// gradient at its mouth, which is nearer the eye than the wall behind
// it; linear slices would spend their precision on the wall.
fn froxel_depth(z: u32) -> f32 {
    let slices = f32(grid.dimensions.z);
    let t = (f32(z) + 0.5 + grid.jitter) / slices;
    return grid.near * pow(grid.far / grid.near, t);
}

// The thickness of a froxel at `z`, for turning a density into an
// extinction over the froxel's own depth.
fn froxel_thickness(z: u32) -> f32 {
    return froxel_depth(z + 1u) - froxel_depth(z);
}

@compute @workgroup_size(4, 4, 4)
fn inject(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id >= grid.dimensions)) {
        return;
    }

    // SPIKE: a constant medium, to prove the grid, the passes and the
    // read before the clustered light walk goes on top. What lands here
    // in the next stage is the same loop `volumetric_fog.wgsl` runs —
    // cluster lookup, spot and point attenuation, shadow fetch, phase
    // function — evaluated once per froxel instead of once per pixel
    // per step, which is the whole point of the exercise.
    let thickness = froxel_thickness(id.z);
    let extinction = (grid.scattering + grid.absorption) * thickness;
    textureStore(
        scattering_grid,
        vec3<i32>(id),
        vec4(vec3(0.02), extinction),
    );
}
