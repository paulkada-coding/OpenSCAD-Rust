// Copies the rendered image (sRGB texture, read back as linear values) into
// the output texture, which stores sRGB-encoded bytes in a plain format.
// A separate pass instead of an sRGB view of the output texture, because
// reinterpreting texture formats is not available on WebGL 2.

@group(0) @binding(0) var rendered: texture_2d<f32>;

@vertex
fn vs_encode(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // Single triangle covering the whole viewport.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn encode_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_encode(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(rendered, vec2<i32>(pixel.xy), 0);
    return vec4<f32>(encode_srgb(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0))), c.a);
}
