// Scene shaders: background gradient, lit model, overlay lines.
// All colors are linear; the render target is sRGB so encoding is automatic.

struct Uniforms {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    // xyz: camera forward, w: 1.0 for orthographic projection.
    forward: vec4<f32>,
    key_dir: vec4<f32>,
    fill_dir: vec4<f32>,
    model_color: vec4<f32>,
    back_color: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

// ---------------------------------------------------------------- background

struct BgOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) t: f32,
};

@vertex
fn vs_background(@builtin(vertex_index) i: u32) -> BgOut {
    // Single triangle covering the whole viewport.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: BgOut;
    out.clip = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.t = uv.y;
    return out;
}

@fragment
fn fs_background(in: BgOut) -> @location(0) vec4<f32> {
    return vec4<f32>(mix(u.bg_bottom.rgb, u.bg_top.rgb, clamp(in.t, 0.0, 1.0)), 1.0);
}

// --------------------------------------------------------------------- model

struct MeshIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_mesh(v: MeshIn) -> MeshOut {
    var out: MeshOut;
    out.clip = u.view_proj * vec4<f32>(v.position, 1.0);
    out.world = v.position;
    out.normal = v.normal;
    return out;
}

@fragment
fn fs_mesh(in: MeshOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    var n = normalize(in.normal);
    var base = u.model_color.rgb;
    // Back faces are visible only through holes or inverted triangles:
    // tint them so such defects stand out, as OpenSCAD does.
    if (!front) {
        n = -n;
        base = u.back_color.rgb;
    }

    var to_eye = normalize(u.eye.xyz - in.world);
    if (u.forward.w > 0.5) {
        to_eye = -u.forward.xyz;
    }

    let key = max(dot(n, u.key_dir.xyz), 0.0);
    let fill = max(dot(n, u.fill_dir.xyz), 0.0);
    let sky = mix(vec3<f32>(0.20, 0.19, 0.18), vec3<f32>(0.42, 0.45, 0.50), n.z * 0.5 + 0.5);
    let half_vec = normalize(u.key_dir.xyz + to_eye);
    let spec = pow(max(dot(n, half_vec), 0.0), 48.0) * 0.18;

    let lit = base * (sky * 0.45 + vec3<f32>(key * 0.85 + fill * 0.18)) + vec3<f32>(spec);
    return vec4<f32>(lit, 1.0);
}

// --------------------------------------------------------------------- lines

struct LineIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

fn line_vertex(v: LineIn) -> LineOut {
    var out: LineOut;
    out.clip = u.view_proj * vec4<f32>(v.position, 1.0);
    out.color = v.color;
    return out;
}

@vertex
fn vs_line(v: LineIn) -> LineOut {
    return line_vertex(v);
}

// Model outlines lie exactly on the surface: pull them slightly towards the
// camera so they win the depth test against their own faces.
@vertex
fn vs_edge(v: LineIn) -> LineOut {
    var out = line_vertex(v);
    out.clip.z -= 2e-5 * out.clip.w;
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return in.color;
}
