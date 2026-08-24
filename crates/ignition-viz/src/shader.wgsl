struct Camera {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    camera_pos: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: Camera;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
}

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) world_pos: vec3<f32>,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_position = camera.view_proj * vec4<f32>(in.position, 1.0);
    out.world_normal = in.normal;
    out.color = in.color;
    out.world_pos = in.position;
    return out;
}

// Cheap hash for per-fragment grain — no texture sampling, just a
// deterministic pseudo-random value from world position so flat-shaded
// surfaces don't read as flat plastic.
fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Distance (0 at a line, `cell` at the centre of a cell) to the nearest
// grid line on both axes of a 2D projection, take the closer axis.
fn grid_dist(p: vec2<f32>, cell: f32) -> f32 {
    let g = abs(fract(p / cell) - 0.5) * cell;
    return min(g.x, g.y);
}

// Wood plank pattern: narrow seams every `plank_width` along one axis
// (the board edges), sparse staggered seams along the other (board ends).
// Returns (distance to nearest seam, a per-plank id for colour variation).
fn plank_dist(p: vec2<f32>, plank_width: f32, plank_length: f32) -> vec2<f32> {
    let board_index = floor(p.x / plank_width);
    // Stagger each board row's end-joints by a pseudo-random offset so
    // they don't all line up (real flooring doesn't either).
    let stagger = hash21(vec2<f32>(board_index, 0.0)) * plank_length;
    let across = abs(fract(p.x / plank_width) - 0.5) * plank_width;
    let along = abs(fract((p.y + stagger) / plank_length) - 0.5) * plank_length;
    let d = min(across, along * 0.3); // end-joints read fainter than edges
    return vec2<f32>(d, board_index);
}

// Ashlar stone-block pattern: rows of blocks, each row offset half a block
// width from the one below (running bond, like real coursed masonry).
// Returns (distance to nearest mortar joint, a per-block id for shading).
fn ashlar_dist(p: vec2<f32>, block_w: f32, block_h: f32) -> vec2<f32> {
    let row = floor(p.y / block_h);
    let row_parity = row - 2.0 * floor(row * 0.5);
    let x = p.x + row_parity * (block_w * 0.5);
    let across = abs(fract(x / block_w) - 0.5) * block_w;
    let along = abs(fract(p.y / block_h) - 0.5) * block_h;
    let block_id = row * 1000.0 + floor(x / block_w);
    return vec2<f32>(min(across, along), block_id);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let l = normalize(-camera.light_dir.xyz);
    // abs(), not max(..., 0.0): several object meshes (screens especially)
    // are single-sided quads that can face either way depending on the
    // source data's rotation convention, and a debug visualizer should
    // never render a correctly-placed object as unlit black just because
    // its winding faces the "wrong" way.
    let ndotl = abs(dot(n, l));
    let ambient = 0.45;
    let lit = ambient + (1.0 - ambient) * ndotl;

    let view_dir = normalize(camera.camera_pos.xyz - in.world_pos);
    let half = normalize(l + view_dir);
    let spec = pow(max(dot(n, half), 0.0), 24.0) * 0.15;

    var rgb = in.color * lit + vec3<f32>(spec, spec, spec);

    // Procedural tile/panel grid — only on near-axis-aligned flat surfaces
    // (floor, ceiling, walls, box props); curved fixture meshes have
    // continuously-varying normals that rarely sit this close to a single
    // world axis, so they fall through untouched. A floor tile is 2ft
    // (0.6096m) matching the real venue's floor grid this whole model is
    // measured against (see docs/domain/norco-field-measurements); walls
    // get a coarser 1.2m panel-seam spacing.
    let an = abs(n);
    if (an.z > 0.9 && in.world_pos.z > 2.0) {
        // Ceiling — black tile, dark grey grid lines, 2ft (0.6096m)
        // matching the venue's own tile unit
        // (docs/domain/norco-field-measurements). The base colour is
        // already near-black, so multiplying it darker at the seams
        // (like every other grid in this file) wouldn't show up —
        // brighten the seams instead, additively.
        let d = grid_dist(in.world_pos.xy, 0.6096);
        rgb += vec3<f32>(0.10, 0.10, 0.11) * (1.0 - smoothstep(0.0, 0.02, d));
    } else if ((an.x > 0.9 || an.y > 0.9) && in.color.r > in.color.b + 0.03) {
        // Columns — stonemason-style ashlar blocks, warm-toned (see
        // scene.rs's COLUMN_COLOR) so this branch only catches the
        // columns, not the cool-grey walls that would otherwise also hit
        // an.x/an.y > 0.9.
        let axes = select(in.world_pos.yz, in.world_pos.xz, an.x > 0.9);
        // Smaller blocks (0.25 x 0.2m) than the first pass so a column
        // this size (0.81 x 1.22m) shows several courses, not just one or
        // two; deeper, wider mortar joints and much stronger per-block
        // tint so it reads as coursed stone instead of a subtly-bumpy flat
        // panel.
        let ad = ashlar_dist(axes, 0.25, 0.2);
        rgb *= mix(0.45, 1.0, smoothstep(0.0, 0.02, ad.x));
        let block_tint = hash21(vec2<f32>(ad.y, 2.0));
        rgb *= 0.72 + block_tint * 0.5;
        // A second, finer hash per block for surface-roughness speckle —
        // real cut stone isn't flat within a block either.
        let speckle = hash21(axes * 19.0 + ad.y);
        rgb *= 0.94 + speckle * 0.12;
    } else if (an.z > 0.9 && (in.color.r + in.color.g + in.color.b) < 0.3) {
        // Stage floor — black-painted plywood: coarse 1.22m (4ft) sheet
        // seams, not the audience's narrow boards. Distinguished from the
        // audience floor by how dark the base colour already is (both are
        // flat, near-zero-slope surfaces at similar world height, so the
        // surface normal/position alone can't tell them apart — see
        // scene.rs for the two colour constants this threshold sits
        // between).
        let d = grid_dist(in.world_pos.xy, 1.2192);
        rgb *= mix(0.7, 1.0, smoothstep(0.0, 0.01, d));
        let grain_lines = plank_dist(in.world_pos.xy, 0.15, 2.0);
        rgb *= mix(0.92, 1.0, smoothstep(0.0, 0.004, grain_lines.x));
    } else if (an.z > 0.9) {
        // Audience floor — real wood planks. 0.15m board width, 2m boards
        // with staggered end-joints.
        let pd = plank_dist(in.world_pos.xy, 0.15, 2.0);
        rgb *= mix(0.78, 1.0, smoothstep(0.0, 0.008, pd.x));
        // Per-board colour variation — real boards aren't uniform.
        let board_tint = hash21(vec2<f32>(pd.y, 1.0));
        rgb *= 0.90 + board_tint * 0.20;
    } else if (an.x > 0.9) {
        let d = grid_dist(in.world_pos.yz, 1.2);
        rgb *= mix(0.9, 1.0, smoothstep(0.0, 0.015, d));
    } else if (an.y > 0.9) {
        let d = grid_dist(in.world_pos.xz, 1.2);
        rgb *= mix(0.9, 1.0, smoothstep(0.0, 0.015, d));
    }

    // Subtle per-fragment grain everywhere, independent of the grid above
    // — breaks up perfectly flat colour on curved surfaces too.
    let grain = hash21(in.world_pos.xy * 37.0 + in.world_pos.z * 53.0);
    rgb *= 0.97 + grain * 0.06;

    return vec4<f32>(rgb, 1.0);
}
