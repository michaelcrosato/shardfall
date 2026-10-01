// Bloom: bright-pass downsample into a mip chain, then tent-filtered upsampling back up
// (each level added onto the next larger one). The composite adds the top level.

struct B {
    src_texel: vec2<f32>,
    dst_texel: vec2<f32>,
    threshold: f32,
    knee: f32,
    pad0: f32,
    pad1: f32,
};

@group(0) @binding(0) var<uniform> b: B;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn tap(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src, samp, uv, 0.0).rgb;
}

fn down(uv: vec2<f32>) -> vec3<f32> {
    let t = b.src_texel;
    var c = tap(uv) * 0.5;
    c += (tap(uv + vec2<f32>(-t.x, -t.y)) + tap(uv + vec2<f32>(t.x, -t.y)) + tap(uv + vec2<f32>(-t.x, t.y)) + tap(uv + t)) * 0.125;
    return c;
}

@fragment
fn fs_prefilter(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let c = min(down(frag.xy * b.dst_texel), vec3<f32>(32.0));
    let l = max(c.r, max(c.g, c.b));
    let soft = clamp(l - b.threshold + b.knee, 0.0, 2.0 * b.knee);
    let s = soft * soft / (4.0 * b.knee + 1e-4);
    let w = max(s, l - b.threshold) / max(l, 1e-4);
    return vec4<f32>(c * w, 1.0);
}

@fragment
fn fs_down(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(down(frag.xy * b.dst_texel), 1.0);
}

@fragment
fn fs_up(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = frag.xy * b.dst_texel;
    let t = b.src_texel;
    var c = tap(uv) * 4.0;
    c += (tap(uv + vec2<f32>(t.x, 0.0)) + tap(uv - vec2<f32>(t.x, 0.0)) + tap(uv + vec2<f32>(0.0, t.y)) + tap(uv - vec2<f32>(0.0, t.y))) * 2.0;
    c += tap(uv + t) + tap(uv - t) + tap(uv + vec2<f32>(t.x, -t.y)) + tap(uv + vec2<f32>(-t.x, t.y));
    return vec4<f32>(c / 16.0, 1.0);
}
