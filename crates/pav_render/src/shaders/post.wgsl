// Composite: screen-space outlines from depth/normal/group, exposure, tonemap, saturation.

struct Post {
    inv_proj: mat4x4<f32>,
    outline_color: vec4<f32>,  // rgb; a = 1 use this color, 0 darken the pixel instead
    outline: vec4<f32>,        // x px, y depth threshold, z normal threshold, w enabled
    tone: vec4<f32>,           // x exposure, y tonemap mode, z encode sRGB manually, w saturation
    misc: vec4<f32>,           // x darken factor
    fwd: vec4<f32>,            // camera forward (world)
};

@group(0) @binding(0) var<uniform> post: Post;
@group(0) @binding(1) var hdr_tex: texture_2d<f32>;
@group(0) @binding(2) var depth_ms: texture_depth_multisampled_2d;
@group(0) @binding(3) var normal_ms: texture_multisampled_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn view_depth(px: vec2<i32>, sample: i32) -> f32 {
    let d = textureLoad(depth_ms, px, sample);
    let v = post.inv_proj * vec4<f32>(0.0, 0.0, d, 1.0);
    return -v.z / v.w;
}

// Edge strength for one MSAA sample index; averaged over samples for anti-aliased outlines.
fn edge_at(px: vec2<i32>, dims: vec2<i32>, t: i32, sample: i32) -> f32 {
    let d0 = view_depth(px, sample);
    let n0 = textureLoad(normal_ms, px, sample);
    if (n0.w <= 0.0) {
        return 0.0;
    }
    var offs = array<vec2<i32>, 4>(vec2<i32>(t, 0), vec2<i32>(-t, 0), vec2<i32>(0, t), vec2<i32>(0, -t));
    let ndv = abs(dot(n0.xyz, post.fwd.xyz));
    let rel = post.outline.y * max(d0, 0.001) * (1.0 + 3.0 * (1.0 - ndv));
    for (var i = 0; i < 4; i++) {
        let q = clamp(px + offs[i], vec2<i32>(0), dims - vec2<i32>(1));
        let d = view_depth(q, sample);
        let n = textureLoad(normal_ms, q, sample);
        // Only mark the nearer side so outlines stay one line thick.
        if (d0 > d + 1e-4) {
            continue;
        }
        if (d - d0 > rel) {
            return 1.0;
        }
        if (n.w != n0.w) {
            return 1.0;
        }
        if (dot(n.xyz, n0.xyz) < post.outline.z) {
            return 1.0;
        }
    }
    return 0.0;
}

fn tonemap(c: vec3<f32>, mode: f32) -> vec3<f32> {
    if (mode < 0.5) {
        return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    if (mode < 1.5) {
        let k = 0.8;
        let over = k + (1.0 - k) * (1.0 - exp(-(c - vec3<f32>(k)) / (1.0 - k)));
        return select(c, over, c > vec3<f32>(k));
    }
    let a = 2.51;
    let b = 0.03;
    let cc = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((c * (a * c + b)) / (c * (cc * c + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_post(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(frag.xy);
    let dims = vec2<i32>(textureDimensions(hdr_tex));
    var col = textureLoad(hdr_tex, px, 0).rgb;

    if (post.outline.w > 0.5) {
        let t = max(i32(post.outline.x + 0.5), 1);
        let samples = i32(textureNumSamples(normal_ms));
        var edge = 0.0;
        for (var k = 0; k < samples; k++) {
            edge += edge_at(px, dims, t, k);
        }
        edge = edge / f32(samples);
        let oc = select(col * post.misc.x, post.outline_color.rgb, post.outline_color.a > 0.5);
        col = mix(col, oc, edge);
    }

    col = col * post.tone.x;
    col = tonemap(col, post.tone.y);
    let luma = dot(col, vec3<f32>(0.2126, 0.7152, 0.0722));
    col = max(mix(vec3<f32>(luma), col, post.tone.w), vec3<f32>(0.0));
    if (post.tone.z > 0.5) {
        col = to_srgb(col);
    }
    return vec4<f32>(col, 1.0);
}
