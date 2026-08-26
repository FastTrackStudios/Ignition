// The volumetric beam material — a port of ASLS Studio's own beam shader
// (`src/plugins/visualizer/shaders/beam.fragment.glsl`, GPL-3.0, same
// licence as this repo), read from their source rather than approximated.
//
// Everything a fragment needs is world-space, which is why there is no
// custom vertex stage here: Bevy's standard mesh vertex shader already
// hands us `world_position`, and the beam's own parameters (where its
// lens is, where it is aimed, how wide it is) ride in the material's
// uniforms instead of in vertex attributes. ASLS ships the same values as
// per-instance attributes on one shared cylinder; a Bevy material is the
// same idea with the engine doing the batching.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> beam_color: vec4<f32>;
// xyz = normalized world-space aim direction, w = beam HALF angle in
// degrees. Half, not full: ASLS's `MovingHead.set angle()` stores
// `angle / 2` before it ever reaches their shader, so the quadratic term
// below is a half angle, and feeding it a manufacturer's full angle
// makes every beam die at roughly half the distance it should.
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> beam_direction_angle: vec4<f32>;
// xyz = world-space position of the fixture's lens, w = throw distance.
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> beam_origin_length: vec4<f32>;
// x = haze, y = seconds (drifts the haze turbulence), zw spare.
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> beam_params: vec4<f32>;

// --- Ashima Arts / webgl-noise 3D simplex noise (MIT) ---------------
// Mechanically translated GLSL -> WGSL. ASLS vendors the identical file;
// the licence terms are the original's.

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

/// Four-octave turbulence, ASLS's `fogging` and its weighting — a
/// spatially varying density field, so a beam reads as mottled haze
/// rather than a flat-shaded cone.
fn fogging(coord: vec3<f32>) -> f32 {
    var fog = abs(snoise(coord)) * 1.0;
    fog += abs(snoise(coord * 2.0)) * 0.5;
    fog += abs(snoise(coord * 4.0)) * 0.25;
    fog += abs(snoise(coord * 8.0)) * 0.125;
    return fog;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let world_pos = in.world_position.xyz;
    let direction = normalize(beam_direction_angle.xyz);
    let half_angle_deg = beam_direction_angle.w;
    let origin = beam_origin_length.xyz;
    let haze = beam_params.x;
    let time = beam_params.y;

    // --- ASLS's `alignmentFactor` ---
    // How across-the-beam the camera is: 0 looking straight down the
    // beam's axis, 1 square across it. From the beam's *origin*, not this
    // fragment — it is a property of the beam as a whole (their
    // `dirCamToLight` uses `beamPos`).
    let cam_to_beam = normalize(view.world_position.xyz - origin);
    let alignment = 1.0 - abs(dot(direction, cam_to_beam));

    // --- ASLS's `attenuation` ---
    // `2.0 / (1.0 + alignmentFactor * distance + radians(angle) *
    // distance * distance)`. A real inverse-quadratic decay in the metres
    // this fragment sits down the beam, evaluated per-fragment. The angle
    // term is why a wide wash dies off over a shorter distance than a
    // tight spot: the same flux spread over a fast-growing cone section.
    let dist = length(world_pos - origin);
    let attenuation = 2.0 / (1.0 + alignment * dist + radians(half_angle_deg) * dist * dist);

    // --- ASLS's `anglePower` ---
    // `pow(dot(normalize(vWorldPosition.xyz), recomputeVertexNormal()),
    // 4.0 * alignmentFactor)` — a gradient *across* the beam: bright
    // through its middle, falling toward its silhouette, and harder the
    // more across-the-beam the view is. This is what makes a beam read as
    // a shaft of light rather than a flat-shaded solid.
    //
    // Both vectors are in **clip space**, which is not a typo in their
    // shader and matters. An honest world-space N·V is a stronger, more
    // physical falloff, and it collapses a beam's visible silhouette into
    // a tapering finger well before its real far end; clip space
    // compresses depth nonlinearly and leaves the view direction nearly
    // constant across the frame, so their term is much flatter and the
    // beam keeps its cone shape out to where the distance falloff ends
    // it. Their look is the point, so this is their maths.
    //
    // Bevy hands the fragment stage a framebuffer coordinate rather than
    // a clip position, so the clip position is rebuilt here. It is a
    // linear function of the world position, so its screen-space
    // derivatives are exact, and `recomputeVertexNormal` is a geometric
    // normal from exactly those. `max(..., 0.0)` keeps `pow` off a
    // negative base (undefined, and a NaN would poison the additive
    // blend); it also one-sides the beam, so its far wall contributes
    // nothing and a single beam is never double-counted.
    let clip = (view.clip_from_world * vec4<f32>(world_pos, 1.0)).xyz;
    // Negated: WGSL's `dpdy` runs down the framebuffer where GLSL's
    // `dFdy` runs up it, which flips the cross product — without this the
    // `max(..., 0.0)` below zeroes the wall that should be lit and keeps
    // the one that should not, and every beam renders empty.
    let clip_normal = -normalize(cross(dpdx(clip), dpdy(clip)));
    let angle_power = pow(max(dot(normalize(clip), clip_normal), 0.0), 4.0 * alignment);

    // The mesh has a far cap at the throw distance, and an additive cap
    // is a lit disc hanging in the air wherever the throw ends short of
    // a surface. Fade the last stretch of the beam to nothing instead,
    // so the beam ends by dying out rather than being cut off — a real
    // beam has no far end, only a surface it lands on or a distance at
    // which nothing of it is left.
    // r[impl viz.beam-reach] - a beam fades out, it is never cut off
    let length = max(beam_origin_length.w, 0.01);
    let end_fade = 1.0 - smoothstep(0.7, 1.0, dist / length);

    let intensity = attenuation * angle_power * end_fade;

    // --- ASLS's `computeFog` ---
    // `max(fogging(...), intensity)`: haze modulates a beam but never
    // dims it below what the falloff curve alone says, so turbulence
    // mottles it without punching holes in it. Sampled in world space
    // rather than their clip space, so the haze sits in the room and
    // beams sweep through it instead of it being screen-locked, with a
    // slow drift along the noise field's third axis so it churns while
    // the beam holds still.
    let fog_coord = vec3<f32>(world_pos.xy * 1.5, world_pos.z * 1.5 + time * 0.15);
    let density = max(fogging(fog_coord), intensity);

    // `beam_color` already carries the fixture's dimmer and colour; haze
    // is the one global trim — how much particulate is in the air to
    // scatter the beam into something visible at all. At 0 a beam is
    // inert, the way it would genuinely look in clean air.
    //
    // Deliberately unclamped: the camera renders HDR and this material is
    // additive, so a bright core stays bright enough for bloom to catch
    // it. That is Bevy's equivalent of ASLS's `toneMapped: false` — the
    // beam's own curve decides how it looks and it never competes with
    // the room's geometry for the same tonemapped range.
    //
    // Alpha is 0, and that is load-bearing rather than an oversight.
    // `AlphaMode::Add` in Bevy is premultiplied-alpha blending —
    // `src.rgb + dst.rgb * (1 - src.a)` — so alpha is what decides how
    // much of the scene behind survives. At 1.0 a beam *replaces* the
    // room (which, with a beam's near-black tail, renders as an opaque
    // black cone); at 0.0 it adds to it and nothing is occluded, which
    // is what ASLS's `AdditiveBlending` does.
    let rgb = beam_color.rgb * intensity * density * haze;
    return vec4<f32>(rgb, 0.0);
}
