// IGNITION PATCH: froxel volumetrics — the injection pass.
//
// One fragment per froxel. The grid is a frustum-aligned voxel volume
// stored as a 2D texture with its Z slices tiled left to right, top to
// bottom: `slice = tile_y * tiles_x + tile_x`, and a froxel's address is
// arithmetic either way. Tiled rather than a `texture_3d` because the
// pass that writes it is a fragment pass, and it is a fragment pass
// because Bevy declares its view bindings — the clustered light lists
// and the shadow atlas this needs — `ShaderStages::FRAGMENT`.
//
// See docs/domain/froxel-volumetrics.md.

#import bevy_pbr::mesh_view_bindings::view
#import bevy_render::view::View

struct FroxelGrid {
    // Froxels across, up, and deep.
    dimensions: vec3<u32>,
    // Slices tiled across the target, so a slice's origin in texels is
    // `vec2(slice % tiles_x, slice / tiles_x) * dimensions.xy`.
    tiles_x: u32,
    // Where the grid starts and stops in view space, in metres. Near is
    // not the camera's near plane: a froxel grid that begins at 0.1 m
    // spends most of its depth on air nobody is looking through.
    near: f32,
    far: f32,
    // Frames advance it so the sample point inside each froxel moves,
    // which is what the temporal pass averages into resolution.
    jitter: f32,
    _pad: f32,
}

@group(1) @binding(0) var<uniform> grid: FroxelGrid;

// Which froxel this fragment is, from its position in the tiled target.
fn froxel_of(frag_coord: vec2<f32>) -> vec3<u32> {
    let texel = vec2<u32>(frag_coord);
    let tile = texel / grid.dimensions.xy;
    let slice = tile.y * grid.tiles_x + tile.x;
    return vec3<u32>(texel % grid.dimensions.xy, slice);
}

// The view-space depth at a froxel's centre.
//
// Exponential, so the near froxels are small. A beam's apex is near the
// fixture and the fixture is usually well inside the room, but the
// steep gradient a cone has at its mouth is always nearer the camera
// than the wall behind it, and this is where the resolution wants to
// be. Linear slices spend their precision on the far half of the room,
// which is mostly wall.
fn froxel_depth(z: u32) -> f32 {
    let slices = f32(grid.dimensions.z);
    let t = (f32(z) + 0.5 + grid.jitter) / slices;
    return grid.near * pow(grid.far / grid.near, t);
}

struct FragmentOutput {
    // rgb: in-scattered light reaching the eye from this froxel.
    // a:   extinction over the froxel's own depth.
    @location(0) scattering: vec4<f32>,
}

@fragment
fn inject(@builtin(position) position: vec4<f32>) -> FragmentOutput {
    let froxel = froxel_of(position.xy);
    var out: FragmentOutput;

    // Outside the grid — the target is tiled, so its corner tiles can
    // be past the last slice. Nothing there.
    if (froxel.z >= grid.dimensions.z) {
        out.scattering = vec4(0.0);
        return out;
    }

    // SPIKE: a constant, to prove the target, the pass and the read.
    // The light loop replaces this once the plumbing renders.
    let depth = froxel_depth(froxel.z);
    out.scattering = vec4(vec3(0.02), 0.02);
    return out;
}
