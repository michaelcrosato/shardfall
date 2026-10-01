// Composite: screen-space outlines from depth/normal/group, exposure, tonemap, saturation.

struct Post {
    inv_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    gi: vec4<f32>,             // x strength, y radius (m), z pixels per metre at 1 m, w frame
    outline_color: vec4<f32>,  // rgb; a = 1 use this color, 0 darken the pixel instead
    outline: vec4<f32>,        // x px, y depth threshold, z normal threshold, w enabled
    tone: vec4<f32>,           // x exposure, y tonemap mode, z encode sRGB manually, w saturation
    misc: vec4<f32>,           // x darken factor, y bloom strength, z distortion on
    fwd: vec4<f32>,            // camera forward (world)
    filt0: vec4<f32>,          // pixel block (px), CRT curvature, scanlines, scanline period (px)
    filt1: vec4<f32>,          // dither, levels, palette, split (0 = off, else screen fraction)
    filt2: vec4<f32>,          // temperature, tint, contrast, brightness
    filt3: vec4<f32>,          // vignette, grain, chromatic aberration, time
};

@group(0) @binding(0) var<uniform> post: Post;
@group(0) @binding(1) var hdr_tex: texture_2d<f32>;
@group(0) @binding(2) var depth_ms: texture_depth_multisampled_2d;
@group(0) @binding(3) var normal_ms: texture_multisampled_2d<f32>;
@group(0) @binding(4) var bloom_tex: texture_2d<f32>;
@group(0) @binding(5) var distort_tex: texture_2d<f32>;
@group(0) @binding(6) var lin: sampler;

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

fn view_pos(px: vec2<i32>, dims: vec2<i32>) -> vec3<f32> {
    let d = textureLoad(depth_ms, px, 0);
    let ndc = vec2<f32>((f32(px.x) + 0.5) / f32(dims.x) * 2.0 - 1.0, 1.0 - (f32(px.y) + 0.5) / f32(dims.y) * 2.0);
    let v = post.inv_proj * vec4<f32>(ndc, d, 1.0);
    return v.xyz / v.w;
}

// Screen-space global illumination: light bouncing off nearby visible surfaces (colour bleed)
// and ambient occlusion from nearby geometry. rgb = bounce, a = occlusion.
fn screen_gi(px: vec2<i32>, dims: vec2<i32>) -> vec4<f32> {
    if (textureLoad(depth_ms, px, 0) >= 1.0) {
        return vec4<f32>(0.0);
    }
    let p = view_pos(px, dims);
    let n4 = textureLoad(normal_ms, px, 0);
    if (n4.w <= 0.0) {
        return vec4<f32>(0.0);
    }
    let n = normalize((post.view * vec4<f32>(n4.xyz, 0.0)).xyz);
    let rad = post.gi.y;
    let rad_px = clamp(rad * post.gi.z / max(-p.z, 0.1), 3.0, 90.0);
    // Per-pixel rotation (interleaved gradient noise) hides the sample pattern.
    let ign = fract(52.9829189 * fract(0.06711056 * f32(px.x) + 0.00583715 * f32(px.y) + post.gi.w * 0.618));
    var bounce = vec3<f32>(0.0);
    var occ = 0.0;
    let count = 12;
    for (var i = 0; i < count; i++) {
        let fi = f32(i);
        let a = fi * 2.39996 + ign * 6.2831;
        let r = sqrt((fi + 0.5) / f32(count)) * rad_px;
        let q = clamp(px + vec2<i32>(vec2<f32>(cos(a), sin(a)) * r), vec2<i32>(0), dims - vec2<i32>(1));
        if (textureLoad(depth_ms, q, 0) >= 1.0) {
            continue;
        }
        let v = view_pos(q, dims) - p;
        let dist = length(v);
        if (dist < 1e-3 || dist > rad * 2.0) {
            continue;
        }
        let dir = v / dist;
        let ndl = max(dot(n, dir) - 0.05, 0.0);
        let fall = 1.0 / (1.0 + dist * dist / (rad * rad));
        occ += ndl * fall;
        let qn = normalize((post.view * vec4<f32>(textureLoad(normal_ms, q, 0).xyz, 0.0)).xyz);
        let emit = max(dot(qn, -dir), 0.0);
        bounce += textureLoad(hdr_tex, q, 0).rgb * ndl * emit * fall;
    }
    return vec4<f32>(bounce / f32(count), occ / f32(count));
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

// The rendered scene at screen position `p` (pixels): distortion, GI, outlines, bloom,
// exposure, tonemap and saturation, in linear colour.
fn scene_color(p: vec2<f32>, dims: vec2<i32>, chroma: f32) -> vec3<f32> {
    let px = clamp(vec2<i32>(p), vec2<i32>(0), dims - vec2<i32>(1));
    var uv = p / vec2<f32>(dims);
    if (post.misc.z > 0.5) {
        uv += textureLoad(distort_tex, px, 0).xy;
    }
    var col = textureSampleLevel(hdr_tex, lin, uv, 0.0).rgb;
    if (chroma > 0.0) {
        let off = (uv - 0.5) * chroma * 0.012;
        col.r = textureSampleLevel(hdr_tex, lin, uv + off, 0.0).r;
        col.b = textureSampleLevel(hdr_tex, lin, uv - off, 0.0).b;
    }
    if (post.gi.x > 0.0) {
        let gi = screen_gi(px, dims);
        col = col * (1.0 - min(gi.a * post.gi.x * 1.5, 0.7)) + gi.rgb * post.gi.x * 1.6;
    }

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
    if (post.misc.y > 0.0) {
        col += textureSampleLevel(bloom_tex, lin, uv, 0.0).rgb * post.misc.y;
    }

    col = col * post.tone.x;
    col = tonemap(col, post.tone.y);
    let luma = dot(col, vec3<f32>(0.2126, 0.7152, 0.0722));
    return max(mix(vec3<f32>(luma), col, post.tone.w), vec3<f32>(0.0));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn bayer4i(p: vec2<f32>) -> f32 {
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    let x = u32(p.x) % 4u;
    let y = u32(p.y) % 4u;
    return (m[y * 4u + x] + 0.5) / 16.0;
}

fn hex3(v: u32) -> vec3<f32> {
    return vec3<f32>(f32((v >> 16u) & 255u), f32((v >> 8u) & 255u), f32(v & 255u)) / 255.0;
}

// A fixed palette (sRGB). 1 Game Boy, 2 PICO-8, 3 CGA, 4 1-bit, 5 amber. Monochrome palettes
// spread the whole brightness range over their shades (dark to light); colour palettes pick the
// nearest colour. `th` is the dither threshold (0.5 = none).
fn palette_nearest(c0: vec3<f32>, pal: u32, th: f32) -> vec3<f32> {
    if (pal == 1u || pal == 4u || pal == 5u) {
        var gbm = array<u32, 4>(0x0f380fu, 0x306230u, 0x8bac0fu, 0x9bbc0fu);
        var ambm = array<u32, 4>(0x140800u, 0x5a2a00u, 0xb86400u, 0xffb000u);
        let n = select(4.0, 2.0, pal == 4u);
        let l = dot(c0, vec3<f32>(0.299, 0.587, 0.114));
        let i = u32(clamp(floor(l * (n - 1.0) + th), 0.0, n - 1.0));
        if (pal == 4u) {
            return vec3<f32>(f32(i));
        }
        if (pal == 1u) {
            return hex3(gbm[i]);
        }
        return hex3(ambm[i]);
    }
    let c = clamp(c0 + (th - 0.5) * 0.18, vec3<f32>(0.0), vec3<f32>(1.0));
    var gb = array<u32, 4>(0x0f380fu, 0x306230u, 0x8bac0fu, 0x9bbc0fu);
    var p8 = array<u32, 16>(0x000000u, 0x1d2b53u, 0x7e2553u, 0x008751u, 0xab5236u, 0x5f574fu, 0xc2c3c7u, 0xfff1e8u,
                            0xff004du, 0xffa300u, 0xffec27u, 0x00e436u, 0x29adffu, 0x83769cu, 0xff77a8u, 0xffccaau);
    var cga = array<u32, 4>(0x000000u, 0x55ffffu, 0xff55ffu, 0xffffffu);
    var amb = array<u32, 4>(0x140800u, 0x5a2a00u, 0xb86400u, 0xffb000u);
    var best = c;
    var bd = 1e9;
    var n = 4u;
    if (pal == 2u) {
        n = 16u;
    } else if (pal == 4u) {
        n = 2u;
    }
    for (var i = 0u; i < n; i++) {
        var q: vec3<f32>;
        if (pal == 1u) {
            q = hex3(gb[i]);
        } else if (pal == 2u) {
            q = hex3(p8[i]);
        } else if (pal == 3u) {
            q = hex3(cga[i]);
        } else if (pal == 4u) {
            q = vec3<f32>(f32(i));
        } else {
            q = hex3(amb[i]);
        }
        var d: f32;
        if (pal == 1u || pal == 4u || pal == 5u) {
            // Monochrome palettes: match by brightness.
            let l = dot(c, vec3<f32>(0.299, 0.587, 0.114));
            let lq = dot(q, vec3<f32>(0.299, 0.587, 0.114));
            d = abs(l - lq);
        } else {
            let e = c - q;
            d = dot(e, e * vec3<f32>(0.3, 0.59, 0.11));
        }
        if (d < bd) {
            bd = d;
            best = q;
        }
    }
    return best;
}

@fragment
fn fs_post(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(hdr_tex));
    let fd = vec2<f32>(dims);
    let split = post.filt1.w;
    let filtered = split <= 0.0 || frag.x >= split * fd.x;
    var s: vec3<f32>;
    if (!filtered) {
        s = to_srgb(scene_color(frag.xy, dims, 0.0));
    } else {
        var uvn = frag.xy / fd;
        // CRT: barrel curvature, black outside the tube.
        let curv = post.filt0.y;
        if (curv > 0.0) {
            var c = uvn * 2.0 - 1.0;
            c = c * (1.0 + curv * c.yx * c.yx);
            uvn = c * 0.5 + 0.5;
            if (uvn.x < 0.0 || uvn.y < 0.0 || uvn.x > 1.0 || uvn.y > 1.0) {
                return vec4<f32>(0.0, 0.0, 0.0, 1.0);
            }
        }
        var p = uvn * fd;
        // Pixelation: everything is looked up at the centre of its block.
        let block = max(post.filt0.x, 1.0);
        if (block > 1.0) {
            p = (floor(p / block) + 0.5) * block;
        }
        s = to_srgb(scene_color(p, dims, post.filt3.z));
        // Colour grading (display space).
        let temp = post.filt2.x;
        s = s * vec3<f32>(1.0 + 0.12 * temp, 1.0 + 0.08 * post.filt2.y, 1.0 - 0.12 * temp);
        s = (s - 0.5) * post.filt2.z + 0.5;
        s = s * post.filt2.w;
        // Scanlines follow the curved tube.
        if (post.filt0.z > 0.0) {
            let period = max(post.filt0.w, 1.0);
            let w = 0.5 - 0.5 * cos(uvn.y * fd.y / period * 6.2831853);
            s = s * (1.0 - post.filt0.z * 0.6 * w);
        }
        // Vignette and film grain.
        if (post.filt3.x > 0.0) {
            let r = length(frag.xy / fd - 0.5) * 1.414;
            s = s * (1.0 - post.filt3.x * smoothstep(0.35, 1.05, r));
        }
        if (post.filt3.y > 0.0) {
            let h = fract(sin(dot(frag.xy + vec2<f32>(post.filt3.w * 61.0, post.filt3.w * 17.0), vec2<f32>(12.9898, 78.233))) * 43758.5453);
            s = s + (h - 0.5) * post.filt3.y * 0.16;
        }
        s = clamp(s, vec3<f32>(0.0), vec3<f32>(1.0));
        // Ordered dithering to a few levels or a fixed palette.
        let pal = u32(post.filt1.z + 0.5);
        let levels = post.filt1.y;
        let bp = floor(frag.xy / block);
        let th = mix(0.5, bayer4i(bp), clamp(post.filt1.x, 0.0, 1.0));
        if (pal > 0u) {
            s = palette_nearest(s, pal, th);
        } else if (levels >= 2.0) {
            let l = levels - 1.0;
            s = floor(s * l + th) / l;
        }
    }
    if (split > 0.0 && abs(frag.x - split * fd.x) < 1.5) {
        s = vec3<f32>(1.0);
    }
    if (post.tone.z > 0.5) {
        return vec4<f32>(s, 1.0);
    }
    return vec4<f32>(to_linear(s), 1.0);
}
