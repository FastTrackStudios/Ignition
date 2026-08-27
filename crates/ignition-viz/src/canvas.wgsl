// A procedural canvas, evaluated per fragment.
//
// This is `ignition_core::canvas::CanvasRecipe::sample` in WGSL, line
// for line: the same hash, the same noise, the same ramp, so a screen
// drawn here shows the picture the CPU reference (`recipe.render`) would
// — which the cooker uses for bitmap channels and the test in
// `canvas_material.rs` checks pixel against pixel. Any change to one
// side is a change to both. See `canvas_material.rs` for the uniform
// layout and `docs/spec/canvas.md` (`canvas.procedural`).

#import bevy_pbr::forward_io::VertexOutput

struct CanvasParams {
    // The piece of the canvas this panel shows, cover-fitted: u0, v0,
    // u1, v1 — `Slice::cover_at` in canvas.rs, done once on the CPU.
    rect: vec4<f32>,
    // kind, seed, count, direction (0 = horizontal, 1 = vertical).
    ints: vec4<u32>,
    // cycles, glow, angle (radians), width.
    scalars: vec4<f32>,
    // noise scale, sparkle density, colour count, spare.
    extra: vec4<f32>,
    colors: array<vec4<f32>, 8>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> canvas: CanvasParams;

const KIND_SOLID: u32 = 0u;
const KIND_GRADIENT: u32 = 1u;
const KIND_WIPE: u32 = 2u;
const KIND_NOISE: u32 = 3u;
const KIND_BAND: u32 = 4u;
const KIND_SPARKLE: u32 = 5u;

fn frac(t: f32) -> f32 {
    return t - floor(t);
}

// `canvas::hash3`: a small integer hash, wrapping u32 arithmetic.
fn hash3(x: u32, y: u32, seed: u32) -> u32 {
    var h = seed ^ 0x8F1BBCDCu;
    h ^= x * 0x9E3779B9u;
    h = ((h << 13u) | (h >> 19u)) * 0x85EBCA6Bu;
    h ^= h >> 16u;
    h ^= y * 0x9E3779B9u;
    h = ((h << 13u) | (h >> 19u)) * 0x85EBCA6Bu;
    h ^= h >> 16u;
    h ^= h >> 13u;
    h = h * 0xC2B2AE35u;
    return h ^ (h >> 16u);
}

fn unit(h: u32) -> f32 {
    return f32(h >> 8u) / 16777216.0;
}

// `x.floor() as i64 as u32` on the CPU: a negative lattice index wraps
// to its two's complement, which is what the bitcast does.
fn lattice(x: f32) -> u32 {
    return bitcast<u32>(i32(floor(x)));
}

// `canvas::value_noise`: bilinear value noise, smoothstepped.
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let x0 = floor(x);
    let y0 = floor(y);
    let fx = x - x0;
    let fy = y - y0;
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let ix = lattice(x);
    let iy = lattice(y);
    let a00 = unit(hash3(ix, iy, seed));
    let a10 = unit(hash3(ix + 1u, iy, seed));
    let a01 = unit(hash3(ix, iy + 1u, seed));
    let a11 = unit(hash3(ix + 1u, iy + 1u, seed));
    let top = a00 + (a10 - a00) * sx;
    let bottom = a01 + (a11 - a01) * sx;
    return top + (bottom - top) * sy;
}

// `canvas::ramp`: through the colour list at `t`, wrapping.
fn ramp(t: f32) -> vec3<f32> {
    let n = u32(canvas.extra.z);
    if n == 0u {
        return vec3<f32>(0.0);
    }
    if n == 1u {
        return canvas.colors[0].rgb;
    }
    let x = frac(t) * f32(n);
    let i = u32(floor(x)) % n;
    let j = (i + 1u) % n;
    let f = x - floor(x);
    return mix(canvas.colors[i].rgb, canvas.colors[j].rgb, f);
}

fn along(u: f32, v: f32) -> f32 {
    if canvas.ints.w == 1u {
        return v;
    }
    return u;
}

fn wrapped_distance(a: f32, b: f32) -> f32 {
    let d = frac(a - b);
    return min(d, 1.0 - d);
}

// `CanvasRecipe::sample(u, v, cycles)`.
fn sample(u_in: f32, v_in: f32, cycles: f32) -> vec3<f32> {
    let u = clamp(u_in, 0.0, 1.0);
    let v = clamp(v_in, 0.0, 1.0);
    let kind = canvas.ints.x;
    let seed = canvas.ints.y;
    let angle = canvas.scalars.z;
    let width = canvas.scalars.w;
    let color = canvas.colors[0].rgb;
    switch kind {
        case KIND_GRADIENT: {
            let s = sin(angle);
            let c = cos(angle);
            let t = ((u - 0.5) * c + (0.5 - v) * s) + 0.5 + cycles;
            return ramp(frac(t));
        }
        case KIND_WIPE: {
            let pos = along(u, v);
            let head = frac(cycles);
            let w = max(width, 0.001);
            let d = wrapped_distance(pos, head);
            let a = clamp(1.0 - d / (w * 0.5), 0.0, 1.0);
            return color * a;
        }
        case KIND_NOISE: {
            let cells = max(canvas.extra.x, 0.001);
            let n = value_noise(u * cells + cycles, v * cells, seed);
            return ramp(n);
        }
        case KIND_BAND: {
            let count = f32(max(canvas.ints.z, 1u));
            let pos = along(u, v);
            let phase = frac(pos * count - cycles);
            if phase < clamp(width, 0.0, 1.0) {
                return color;
            }
            return vec3<f32>(0.0);
        }
        case KIND_SPARKLE: {
            let gx = u32(floor(u * 32.0));
            let gy = u32(floor(v * 18.0));
            let generation = lattice(cycles);
            let roll = hash3(gx, gy, seed + generation * 0x9E3779B9u);
            if unit(roll) < clamp(canvas.extra.y, 0.0, 1.0) {
                return color * (1.0 - frac(cycles));
            }
            return vec3<f32>(0.0);
        }
        default: {
            return color;
        }
    }
}

// The texture path stores the recipe's value as an sRGB byte and lets
// the sampler decode it; this does the decode by hand so the screen
// shows the same tone either way.
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // The quad's UVs run 0..1 with V down (Bevy's `Rectangle`); the
    // slice is the same orientation as the texture path's `Slice::uvs`.
    let r = canvas.rect;
    let u = mix(r.x, r.z, in.uv.x);
    let v = mix(1.0 - r.w, 1.0 - r.y, in.uv.y);
    let c = clamp(sample(u, v, canvas.scalars.x), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(srgb_to_linear(c) * canvas.scalars.y, 1.0);
}
