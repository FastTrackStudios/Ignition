// Live-mode-only shader — a copy of shader.wgsl (same base ambient +
// directional lighting, same procedural texturing) plus two additions that
// only matter when there's live DMX data to draw: a point-light pass that
// lets lit fixtures actually illuminate the walls/floor around them, and a
// separate `fs_glow` entry point for the additively-blended beam cones
// (`mesh.rs`'s `glow_vertices`). Kept as its own file instead of adding
// this to `shader.wgsl` so `renderer.rs`'s headless regression-screenshot
// path (`shot`) never has to know about either — its pipeline/bind-group
// layout is completely untouched by this file.

struct Camera {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    camera_pos: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: Camera;

// Visualizer settings — the live-mode equivalent of a console viz app's
// "ambient light"/"haze level" sliders (QLC+, grandMA3's 3D view, WYSIWYG
// etc. all expose something like this). Defaults live in `live_pipeline.rs`
// (ambient 0.0, haze 0.35) and are overridable from `bin/live.rs`'s
// `--ambient`/`--haze` flags — deliberately NOT the same 0.45 ambient
// `shader.wgsl` hardcodes, since that shader exists to give `shot` a flat,
// always-readable layout view, not a realistic stage look. `_pad1` keeps
// the struct's WGSL uniform alignment (16-byte rounded) matching its
// Rust-side `#[repr(C)]` twin.
struct Settings {
    ambient: f32,
    haze: f32,
    /// Seconds since the renderer started — drifts the beam haze's
    /// turbulence (`fs_glow`'s `fogging()`) each frame instead of leaving
    /// it frozen. 0.0 in `--snapshot` mode (a single static frame has no
    /// "next frame" for drift to matter).
    time: f32,
    _pad1: f32,
}

@group(0) @binding(1)
var<uniform> settings: Settings;

// One entry per live-lit fixture (`mesh.rs::PointLight`) — a real cone-
// angled spotlight, not an omnidirectional point light (see that type's
// own doc comment — confirmed against ASLS Studio's actual visualizer
// source, `docs/research/lighting-console-landscape.md`'s Slice 7).
// `position.w`/`color.a` are unused, kept as vec4 for storage-buffer
// alignment. `color` is already dimmer-scaled by the time it lands here
// (see `scene.rs`), so this shader does no separate intensity math beyond
// distance + cone falloff.
struct PointLight {
    position: vec4<f32>,
    color: vec4<f32>,
    // xyz = normalized aim direction, w = cos(cone half-angle) —
    // precomputed CPU-side (live_pipeline.rs) so this is a plain compare.
    direction_cos_angle: vec4<f32>,
}

@group(1) @binding(0)
var<storage, read> point_lights: array<PointLight>;

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

// ACES filmic tone mapping (Narkowicz 2015 fit) — the other big piece of
// ASLS Studio's renderer setup this project was missing (their
// `renderer.toneMapping = THREE.ACESFilmicToneMapping`, alongside
// `antialias: true` — see `SAMPLE_COUNT` in `live_pipeline.rs`). Applied
// to the final linear colour right before output; both this shader's
// colour targets are sRGB formats, so the hardware still does the
// linear->sRGB encode on write — this only replaces the naive clamp with
// a filmic shoulder/toe curve, avoiding the flat, blown-out highlights a
// straight clamp produces on anything bright (which a lit fixture's
// point-light spill and beam glow both routinely are).
fn aces_tonemap(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn grid_dist(p: vec2<f32>, cell: f32) -> f32 {
    let g = abs(fract(p / cell) - 0.5) * cell;
    return min(g.x, g.y);
}

fn plank_dist(p: vec2<f32>, plank_width: f32, plank_length: f32) -> vec2<f32> {
    let board_index = floor(p.x / plank_width);
    let stagger = hash21(vec2<f32>(board_index, 0.0)) * plank_length;
    let across = abs(fract(p.x / plank_width) - 0.5) * plank_width;
    let along = abs(fract((p.y + stagger) / plank_length) - 0.5) * plank_length;
    let d = min(across, along * 0.3);
    return vec2<f32>(d, board_index);
}

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
    let ndotl = abs(dot(n, l));
    // `settings.ambient` scales the room's *entire* non-fixture lighting
    // (this directional key light, standing in for house/work lights, and
    // its specular highlight below) — not just a fixed floor added under
    // it. At ambient 0 (the default) a surface with no fixture light
    // falling on it is genuinely black, the way an unlit venue actually
    // looks; ambient only exists as a dial back toward a fully-lit room
    // for whoever wants a brighter, easier-to-read view instead.
    let ambient = settings.ambient;
    let lit = ambient * mix(0.35, 1.0, ndotl);

    let view_dir = normalize(camera.camera_pos.xyz - in.world_pos);
    let half = normalize(l + view_dir);
    let spec = pow(max(dot(n, half), 0.0), 24.0) * 0.15 * ambient;

    var rgb = in.color * lit + vec3<f32>(spec, spec, spec);

    let an = abs(n);
    if (an.z > 0.9 && in.world_pos.z > 2.0) {
        let d = grid_dist(in.world_pos.xy, 0.6096);
        rgb += vec3<f32>(0.10, 0.10, 0.11) * (1.0 - smoothstep(0.0, 0.02, d));
    } else if ((an.x > 0.9 || an.y > 0.9) && in.color.r > in.color.b + 0.03) {
        let axes = select(in.world_pos.yz, in.world_pos.xz, an.x > 0.9);
        let ad = ashlar_dist(axes, 0.25, 0.2);
        rgb *= mix(0.45, 1.0, smoothstep(0.0, 0.02, ad.x));
        let block_tint = hash21(vec2<f32>(ad.y, 2.0));
        rgb *= 0.72 + block_tint * 0.5;
        let speckle = hash21(axes * 19.0 + ad.y);
        rgb *= 0.94 + speckle * 0.12;
    } else if (an.z > 0.9 && (in.color.r + in.color.g + in.color.b) < 0.3) {
        let d = grid_dist(in.world_pos.xy, 1.2192);
        rgb *= mix(0.7, 1.0, smoothstep(0.0, 0.01, d));
        let grain_lines = plank_dist(in.world_pos.xy, 0.15, 2.0);
        rgb *= mix(0.92, 1.0, smoothstep(0.0, 0.004, grain_lines.x));
    } else if (an.z > 0.9) {
        let pd = plank_dist(in.world_pos.xy, 0.15, 2.0);
        rgb *= mix(0.78, 1.0, smoothstep(0.0, 0.008, pd.x));
        let board_tint = hash21(vec2<f32>(pd.y, 1.0));
        rgb *= 0.90 + board_tint * 0.20;
    } else if (an.x > 0.9) {
        let d = grid_dist(in.world_pos.yz, 1.2);
        rgb *= mix(0.9, 1.0, smoothstep(0.0, 0.015, d));
    } else if (an.y > 0.9) {
        let d = grid_dist(in.world_pos.xz, 1.2);
        rgb *= mix(0.9, 1.0, smoothstep(0.0, 0.015, d));
    }

    let grain = hash21(in.world_pos.xy * 37.0 + in.world_pos.z * 53.0);
    rgb *= 0.97 + grain * 0.06;

    // Live point lights — every lit fixture illuminates nearby surfaces,
    // not just its own mesh. Cheap inverse-square-ish falloff (no shadows,
    // no cone/spot restriction yet — every light is omnidirectional here,
    // a reasonable first cut per this project's own research into what
    // QLC+/ASLS Studio actually do: neither goes further than this without
    // a much larger renderer).
    let light_count = arrayLength(&point_lights);
    for (var i = 0u; i < light_count; i++) {
        let light = point_lights[i];
        let to_light = light.position.xyz - in.world_pos;
        let dist = length(to_light);
        let light_dir = to_light / max(dist, 0.001);
        let light_ndotl = max(dot(n, light_dir), 0.0);
        let atten = 1.0 / (1.0 + dist * dist * 0.35);
        // Cone falloff — a real fixture only lights within roughly its own
        // beam angle, not every direction. `-light_dir` is the direction
        // FROM the light TO this fragment (light_dir points the other
        // way); soft-edged like the beam cone itself (smoothstep between
        // the cone edge and a bit inside it), not a hard cutoff.
        let light_aim = light.direction_cos_angle.xyz;
        let cos_outer = light.direction_cos_angle.w;
        let cos_to_frag = dot(light_aim, -light_dir);
        let cone = smoothstep(cos_outer, mix(cos_outer, 1.0, 0.3), cos_to_frag);
        rgb += light.color.rgb * light_ndotl * atten * cone;
    }

    // Raw linear HDR — no tonemap here. This target is `HDR_FORMAT`
    // (`live_pipeline.rs`), resolved and handed to a single final
    // `fs_tonemap` pass instead. Tonemapping per-pass here (as this used
    // to) applies a filmic *compression* to each fragment individually,
    // which is not the same operation as compressing the true combined
    // brightness once everything (this pass, the additive glow pass) has
    // actually summed together in the framebuffer — several already-
    // compressed-toward-1.0 fragments can still sum past white when
    // added on top of each other. See `fs_tonemap`'s own comment.
    return vec4<f32>(rgb, 1.0);
}

// The beam-cone pass: a direct port of ASLS Studio's own beam shader
// (`src/plugins/visualizer/shaders/beam.{vertex,fragment}.glsl`, GPL-3.0,
// same licence as this repo), read from their source rather than
// approximated. Every term below is theirs; see `fs_glow` for the
// line-by-line mapping. The geometry differs — Ignition bakes real
// world-space cones on the CPU where ASLS widens one instanced cylinder
// in the vertex shader — but the shading maths a fragment sees is the
// same, which is the part that decides how a beam looks.
//
// The one term that is *not* ASLS's is the haze coordinate: they sample
// their noise field in clip space, which makes the mottling screen-locked;
// this samples it in world space, so the haze sits in the room and beams
// sweep through it. The noise function itself is Ashima Arts'
// public-domain/MIT `webgl-noise`, mechanically translated GLSL -> WGSL,
// same licence terms as the original (ASLS vendors the identical file).

fn mod289_3(x: vec3<f32>) -> vec3<f32> {
    return x - floor(x * (1.0 / 289.0)) * 289.0;
}
fn mod289_4(x: vec4<f32>) -> vec4<f32> {
    return x - floor(x * (1.0 / 289.0)) * 289.0;
}
fn permute4(x: vec4<f32>) -> vec4<f32> {
    return mod289_4(((x * 34.0) + 10.0) * x);
}
fn taylor_inv_sqrt4(r: vec4<f32>) -> vec4<f32> {
    return 1.79284291400159 - 0.85373472095314 * r;
}

// Ashima Arts / webgl-noise 3D simplex noise (MIT) — see file header above.
fn snoise(v: vec3<f32>) -> f32 {
    let c = vec2<f32>(1.0 / 6.0, 1.0 / 3.0);
    let d = vec4<f32>(0.0, 0.5, 1.0, 2.0);

    var i = floor(v + dot(v, c.yyy));
    let x0 = v - i + dot(i, c.xxx);

    let g = step(x0.yzx, x0.xyz);
    let l = 1.0 - g;
    let i1 = min(g.xyz, l.zxy);
    let i2 = max(g.xyz, l.zxy);

    let x1 = x0 - i1 + c.xxx;
    let x2 = x0 - i2 + c.yyy;
    let x3 = x0 - d.yyy;

    i = mod289_3(i);
    let p = permute4(permute4(permute4(i.z + vec4<f32>(0.0, i1.z, i2.z, 1.0)) + i.y + vec4<f32>(0.0, i1.y, i2.y, 1.0)) + i.x + vec4<f32>(0.0, i1.x, i2.x, 1.0));

    let n_ = 0.142857142857;
    let ns = n_ * d.wyz - d.xzx;

    let j = p - 49.0 * floor(p * ns.z * ns.z);

    let x_ = floor(j * ns.z);
    let y_ = floor(j - 7.0 * x_);

    let x = x_ * ns.x + ns.yyyy;
    let y = y_ * ns.x + ns.yyyy;
    let h = 1.0 - abs(x) - abs(y);

    let b0 = vec4<f32>(x.xy, y.xy);
    let b1 = vec4<f32>(x.zw, y.zw);

    let s0 = floor(b0) * 2.0 + 1.0;
    let s1 = floor(b1) * 2.0 + 1.0;
    let sh = -step(h, vec4<f32>(0.0));

    let a0 = b0.xzyw + s0.xzyw * sh.xxyy;
    let a1 = b1.xzyw + s1.xzyw * sh.zzww;

    var p0 = vec3<f32>(a0.xy, h.x);
    var p1 = vec3<f32>(a0.zw, h.y);
    var p2 = vec3<f32>(a1.xy, h.z);
    var p3 = vec3<f32>(a1.zw, h.w);

    let norm = taylor_inv_sqrt4(vec4<f32>(dot(p0, p0), dot(p1, p1), dot(p2, p2), dot(p3, p3)));
    p0 *= norm.x;
    p1 *= norm.y;
    p2 *= norm.z;
    p3 *= norm.w;

    var m = max(0.5 - vec4<f32>(dot(x0, x0), dot(x1, x1), dot(x2, x2), dot(x3, x3)), vec4<f32>(0.0));
    m = m * m;
    return 105.0 * dot(m * m, vec4<f32>(dot(p0, x0), dot(p1, x1), dot(p2, x2), dot(p3, x3)));
}

// Four-octave turbulence, same weighting ASLS uses (`fogging` in their
// beam.fragment.glsl) — a spatially-varying density field, so a beam
// reads as mottled haze rather than a flat-shaded cone.
fn fogging(coord: vec3<f32>) -> f32 {
    var fog = abs(snoise(coord)) * 1.0;
    fog += abs(snoise(coord * 2.0)) * 0.5;
    fog += abs(snoise(coord * 4.0)) * 0.25;
    fog += abs(snoise(coord * 8.0)) * 0.125;
    return fog;
}

// `mesh.rs::GlowVertex`, in the same field order — the beam's own
// parameters ride with every vertex so the fragment shader can evaluate
// the falloff curve at the fragment's real place inside the beam.
struct GlowVertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) beam_local: vec3<f32>,
    @location(3) color: vec3<f32>,
    @location(4) direction: vec3<f32>,
    @location(5) origin: vec3<f32>,
    @location(6) angle_deg: f32,
}

struct GlowOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    // The pre-divide clip position, forwarded as an ordinary varying.
    // `@builtin(position)` is the framebuffer coordinate by the time a
    // fragment sees it, which is not what `anglePower` wants — ASLS's
    // `vWorldPosition` (their name; it holds `projectionMatrix *
    // viewMatrix * modelMatrix * position`) is this.
    @location(7) clip_pos: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) beam_local: vec3<f32>,
    @location(3) color: vec3<f32>,
    @location(4) direction: vec3<f32>,
    @location(5) origin: vec3<f32>,
    // Constant across a beam, so there is nothing to interpolate.
    @location(6) @interpolate(flat) angle_deg: f32,
}

@vertex
fn vs_glow(in: GlowVertexIn) -> GlowOut {
    var out: GlowOut;
    out.clip_position = camera.view_proj * vec4<f32>(in.position, 1.0);
    out.clip_pos = out.clip_position;
    out.world_pos = in.position;
    out.normal = in.normal;
    out.beam_local = in.beam_local;
    out.color = in.color;
    out.direction = in.direction;
    out.origin = in.origin;
    out.angle_deg = in.angle_deg;
    return out;
}

@fragment
fn fs_glow(in: GlowOut) -> @location(0) vec4<f32> {
    // --- ASLS's `alignmentFactor` ---
    // How across-the-beam the camera is: 0 looking straight down the
    // beam's axis, 1 looking square across it. Computed from the beam's
    // *origin*, not this fragment's position, because it is a property of
    // the beam as a whole (their `dirCamToLight` uses `beamPos`).
    let cam_to_beam = normalize(camera.camera_pos.xyz - in.origin);
    let alignment = 1.0 - abs(dot(normalize(in.direction), cam_to_beam));

    // --- ASLS's `attenuation` ---
    // `2.0 / (1.0 + alignmentFactor * distance + radians(angle) *
    // distance * distance)` — a real inverse-quadratic decay in the
    // metres this fragment sits down the beam, evaluated per-fragment
    // rather than interpolated between baked vertex colours (what this
    // used to do). The angle term is why a wide wash dies off over a
    // shorter distance than a tight spot: the same flux spread over a
    // fast-growing cone section.
    let dist = length(in.beam_local);
    let attenuation = 2.0 / (1.0 + alignment * dist + radians(in.angle_deg) * dist * dist);

    // --- ASLS's `anglePower` ---
    // `pow(dot(normalize(vWorldPosition.xyz), recomputeVertexNormal()),
    // 4.0 * alignmentFactor)` — a gradient *across* the beam: bright
    // through its middle, falling off toward its silhouette, and the
    // harder the more across-the-beam the view is. This is the term that
    // makes a beam read as a soft shaft of light rather than a
    // flat-shaded solid, and no per-vertex intensity curve can express
    // it — it varies across a beam, not along it.
    //
    // Both vectors are in **clip space**, which is not a typo in their
    // shader and matters: the first pass at this port substituted an
    // honest world-space N·V, which is a stronger, more physical falloff
    // and collapsed every beam's visible silhouette into a tapering
    // finger well before its real far end. Clip space compresses depth
    // nonlinearly and leaves the "view direction" nearly constant across
    // the frame, so their term is much flatter and the beam keeps its
    // cone shape out to where the distance falloff ends it. Their look
    // is the point, so this is now their maths.
    //
    // `recomputeVertexNormal` is a geometric normal from screen-space
    // derivatives. WGSL's `dpdy` runs down the framebuffer where GLSL's
    // `dFdy` runs up it, which flips the cross product — hence the
    // explicit negation, rather than leaving the beam's lit side up to
    // the rasteriser's Y convention. `max(..., 0.0)` keeps `pow` off a
    // negative base (undefined in both languages, and a NaN here would
    // poison the additive blend); it also one-sides the beam, so its far
    // wall contributes nothing and a single beam is never double-counted.
    let clip = in.clip_pos.xyz;
    let clip_normal = -normalize(cross(dpdx(clip), dpdy(clip)));
    let facing = max(dot(normalize(clip), clip_normal), 0.0);
    let angle_power = pow(facing, 4.0 * alignment);

    let intensity = attenuation * angle_power;

    // --- ASLS's `computeFog` ---
    // `max(fogging(...), intensity)`: haze modulates the beam but never
    // dims it below what the falloff curve alone says, so turbulence
    // mottles a beam without punching holes in it. Sampled in world space
    // (see this section's header) with a slow drift along the noise
    // field's third axis, so the haze churns without the beam moving.
    let fog_coord = vec3<f32>(in.world_pos.xy * 1.5, in.world_pos.z * 1.5 + settings.time * 0.15);
    let density = max(fogging(fog_coord), intensity);

    // `settings.haze` stands in for ASLS's per-instance `intensity`
    // attribute as the one global brightness trim — how much particulate
    // is in the air to scatter the beam into something visible at all. At
    // 0 a beam is inert, the way it would genuinely look in clean air.
    // `in.color` already carries the fixture's dimmer and colour
    // (`scene.rs`).
    //
    // Raw linear HDR: this pass renders into its own HDR target which is
    // composited on top of the *already tonemapped* opaque scene, never
    // through the scene's tonemap — ASLS's `toneMapped: false`. See
    // `fs_composite`.
    return vec4<f32>(in.color * intensity * density * settings.haze, 1.0);
}

// A fullscreen triangle (3 vertices, no vertex buffer — the classic
// oversized-triangle trick: covers the full clip-space quad without
// needing 4 vertices + an index buffer) that combines the two resolved
// HDR targets into the final image — see `fs_composite`.
struct FullscreenOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) idx: u32) -> FullscreenOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var out: FullscreenOut;
    let p = positions[idx];
    out.clip_position = vec4<f32>(p, 0.0, 1.0);
    // Clip space is Y-up, texture space is Y-down — flip.
    out.uv = p * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
    return out;
}

@group(0) @binding(0)
var hdr_texture: texture_2d<f32>;
@group(0) @binding(1)
var hdr_sampler: sampler;
@group(0) @binding(2)
var glow_texture: texture_2d<f32>;

// Tonemap the room, then add the beams on top *untouched*.
//
// This is ASLS's `toneMapped: false` on the beam material, structurally.
// In Three.js that flag means a material's output skips the renderer's
// tonemapping chunk and lands in the framebuffer as-is, while everything
// else in the scene gets ACES applied — so a beam's own falloff curve
// decides how bright it looks, and it never competes with the room's
// geometry for the same limited tonemapped range.
//
// Ignition used to run both through one shared HDR target and one shared
// ACES pass, which is why a bright beam and a bright wall fought over the
// same headroom, and why a dense rig's overlapping beams flattened into
// one saturated blob. The two now render into separate HDR targets and
// only meet here. The hardware's own clamp on the sRGB output is the only
// ceiling the beams see, exactly as in Three.js.
@fragment
fn fs_composite(in: FullscreenOut) -> @location(0) vec4<f32> {
    let scene = textureSample(hdr_texture, hdr_sampler, in.uv).rgb;
    let glow = textureSample(glow_texture, hdr_sampler, in.uv).rgb;
    return vec4<f32>(aces_tonemap(scene) + glow, 1.0);
}
