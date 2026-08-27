// IGNITION PATCH: froxel volumetrics — the integrate pass.
//
// One invocation per column of the grid, walking Z front to back and
// accumulating scattered light against transmittance. The result is a
// grid the apply pass can read at any depth without walking anything,
// which is why there is no per-pixel prefix sum later on.
//
// See docs/domain/froxel-volumetrics.md.

struct FroxelGrid {
    dimensions: vec3<u32>,
    jitter: f32,
    near: f32,
    far: f32,
    scattering: f32,
    absorption: f32,
}

@group(0) @binding(0) var<uniform> grid: FroxelGrid;
@group(0) @binding(1) var scattering_grid: texture_3d<f32>;
@group(0) @binding(2) var integrated_grid: texture_storage_3d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn integrate(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= grid.dimensions.x || id.y >= grid.dimensions.y) {
        return;
    }

    // Front to back: what reaches the eye from this froxel is what it
    // scatters, attenuated by everything already between it and the
    // camera. `transmittance` is that "everything so far", and it only
    // ever falls, so the accumulation is stable however deep the grid.
    var accumulated = vec3(0.0);
    var transmittance = 1.0;

    for (var z = 0u; z < grid.dimensions.z; z = z + 1u) {
        let froxel = vec3<i32>(vec3<u32>(id.xy, z));
        let sample = textureLoad(scattering_grid, froxel, 0);
        let scattered = sample.rgb;
        let extinction = sample.a;

        // The analytic integral of scattering through a slab of
        // constant extinction, rather than a rectangle rule over it.
        // At the depths a froxel covers this is the difference between
        // a beam that fades and a beam that steps.
        let slab = 1.0 - exp(-extinction);
        accumulated += scattered * slab * transmittance;
        transmittance *= exp(-extinction);

        textureStore(
            integrated_grid,
            froxel,
            vec4(accumulated, transmittance),
        );
    }
}
