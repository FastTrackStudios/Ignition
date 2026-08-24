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

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let l = normalize(-camera.light_dir.xyz);
    let ndotl = max(dot(n, l), 0.0);
    let ambient = 0.35;
    let lit = ambient + (1.0 - ambient) * ndotl;

    let view_dir = normalize(camera.camera_pos.xyz - in.world_pos);
    let half = normalize(l + view_dir);
    let spec = pow(max(dot(n, half), 0.0), 24.0) * 0.15;

    let rgb = in.color * lit + vec3<f32>(spec, spec, spec);
    return vec4<f32>(rgb, 1.0);
}
