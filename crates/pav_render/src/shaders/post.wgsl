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
    filt4: vec4<f32>,          // saturation (filtered side only), pixel-art block, levels, outline
    // Parts of the scene (0 all, 1 characters and objects, 2 environment) for outlines, colour
    // reduction, grading and scanlines; then grain and chromatic aberration.
    filt5: vec4<f32>,
    filt6: vec4<f32>,          // grain part, chroma part, stylize part
    filt7: vec4<f32>,          // stylize (0 off, 1 paint, 2 halftone, 3 ASCII, 4 sketch), size (px), mix, colour kept
    filt8: vec4<f32>,          // transition cover (0 clear .. 1 covered), kind, centre (screen fraction)
    inv_view: mat4x4<f32>,
    light_vp: mat4x4<f32>,
    sun_dir: vec4<f32>,        // xyz direction the sunlight travels, w = sun shadows on
    sun_color: vec4<f32>,      // rgb sunlight (linear, times intensity), w = point light count
    sky: vec4<f32>,            // ambient sky light: the haze's own glow
    air: vec4<f32>,            // haze density, haze height (m), haze base height (m), light shafts
    air2: vec4<f32>,           // forward scattering (0..0.95), lamp halos
};

struct PointLight {
    pos_radius: vec4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> post: Post;
@group(0) @binding(1) var hdr_tex: texture_2d<f32>;
@group(0) @binding(2) var depth_ms: texture_depth_multisampled_2d;
@group(0) @binding(3) var normal_ms: texture_multisampled_2d<f32>;
@group(0) @binding(4) var bloom_tex: texture_2d<f32>;
@group(0) @binding(5) var distort_tex: texture_2d<f32>;
@group(0) @binding(6) var lin: sampler;
@group(0) @binding(7) var shadow_map: texture_depth_2d;
@group(0) @binding(8) var shadow_cmp: sampler_comparison;
@group(0) @binding(9) var<storage, read> point_lights: array<PointLight>;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// The normal buffer's w is 1024 or more (in size) on characters and objects.
fn object_w(w: f32) -> bool {
    return abs(w) > 1023.5;
}

// Whether a filter aimed at `part` (0 whole scene, 1 characters and objects, 2 environment)
// applies to a pixel of this kind.
fn aimed(part: f32, obj: bool) -> bool {
    let t = u32(part + 0.5);
    return t == 0u || (t == 1u) == obj;
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
    if (n0.w == 0.0) {
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
    if (n4.w == 0.0) {
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

// World position on the camera ray through `ndc` at depth `z` (0 = near plane).
fn unproject(ndc: vec2<f32>, z: f32) -> vec3<f32> {
    let v = post.inv_proj * vec4<f32>(ndc, z, 1.0);
    return (post.inv_view * vec4<f32>(v.xyz / v.w, 1.0)).xyz;
}

// 1 where sunlight reaches world point `x`, 0 in shadow (from the sun's shadow map; lit
// outside the square it covers).
fn sun_reaches(x: vec3<f32>) -> f32 {
    let c = post.light_vp * vec4<f32>(x, 1.0);
    let ndc = c.xyz / c.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, -ndc.y * 0.5 + 0.5);
    if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0 || ndc.z > 1.0 || ndc.z < 0.0) {
        return 1.0;
    }
    return textureSampleCompareLevel(shadow_map, shadow_cmp, uv, ndc.z - 0.0015);
}

// Hazy air between the camera and the surface seen at `px` (volumetric light). The ray is
// marched in steps through the haze (thick near its base height, thinning out above it);
// at every step some sunlight scatters toward the camera, but only where the sun's shadow
// map says the sun reaches that point, so shadows cut dark shafts out of the bright air.
// The haze glows a little with the sky's ambient light and dims what is behind it. Lamp
// halos add the light each point light scatters along the ray, in closed form. Returns
// rgb = light added (HDR), a = how much of the surface still shows through.
fn air_light(px: vec2<i32>, dims: vec2<i32>) -> vec4<f32> {
    let d = textureLoad(depth_ms, px, 0);
    let ndc = vec2<f32>((f32(px.x) + 0.5) / f32(dims.x) * 2.0 - 1.0, 1.0 - (f32(px.y) + 0.5) / f32(dims.y) * 2.0);
    let ro = unproject(ndc, 0.0);
    var rd = unproject(ndc, min(d, 0.99999)) - ro;
    var len = length(rd);
    rd = rd / max(len, 1e-4);
    len = min(len, 150.0);
    var light = vec3<f32>(0.0);
    var trans = 1.0;
    let sigma0 = post.air.x * 0.05;
    // Pixels with no surface (sky, or the void around a level) get no haze: their ray would
    // run for ever through it.
    if (sigma0 > 0.0 && d < 1.0) {
        let h = post.air.y;
        let base = post.air.z;
        // March only where there is haze to speak of: from a few haze heights above its base
        // down to just under it (it lies on the ground: rays into the void below stop there).
        let top = base + h * 5.0;
        let bottom = base - 1.0;
        var t0 = 0.0;
        var t1 = len;
        if (abs(rd.y) > 1e-5) {
            let tt = (top - ro.y) / rd.y;
            let tb = (bottom - ro.y) / rd.y;
            if (rd.y < 0.0) {
                t0 = max(t0, tt);
                t1 = min(t1, tb);
            } else {
                t0 = max(t0, tb);
                t1 = min(t1, tt);
            }
        } else if (ro.y > top || ro.y < bottom) {
            t1 = 0.0;
        }
        if (t1 > t0) {
            let steps = 32;
            let ds = (t1 - t0) / f32(steps);
            // Henyey-Greenstein phase: how much light turns toward the camera (normalised so
            // even scattering is 1); forward scattering brightens the air toward the sun.
            let g = post.air2.x;
            let cos_t = dot(-rd, post.sun_dir.xyz);
            let phase = (1.0 - g * g) / pow(max(1.0 + g * g - 2.0 * g * cos_t, 1e-4), 1.5);
            let sun = post.sun_color.rgb * phase * post.air.w;
            let amb = post.sky.rgb * 0.3;
            let shafts = post.sun_dir.w > 0.5 && post.air.w > 0.0;
            // A different start offset per pixel turns banding into fine grain.
            let jitter = fract(52.9829189 * fract(0.06711056 * f32(px.x) + 0.00583715 * f32(px.y)));
            for (var i = 0; i < steps; i++) {
                let x = ro + rd * (t0 + (f32(i) + jitter) * ds);
                let k = sigma0 * exp(-max(x.y - base, 0.0) / h) * smoothstep(bottom, base, x.y) * ds;
                var vis = 1.0;
                if (shafts) {
                    vis = sun_reaches(x);
                }
                light += trans * k * (sun * vis + amb);
                trans *= exp(-k);
            }
        }
    }
    let halos = post.air2.y;
    if (halos > 0.0) {
        let n = u32(post.sun_color.w);
        for (var i = 0u; i < n; i++) {
            let pl = point_lights[i];
            let r = pl.pos_radius.w;
            let tc = dot(pl.pos_radius.xyz - ro, rd);
            let hd = max(length(ro + rd * tc - pl.pos_radius.xyz), 0.03);
            if (hd < r) {
                // The integral of 1 / distance^2 along the ray (in front of the surface).
                let s = (atan((len - tc) / hd) - atan(-tc / hd)) / hd;
                let fade = (1.0 - hd / r) * (1.0 - hd / r);
                light += pl.color.rgb * s * fade * halos * 0.035;
            }
        }
    }
    return vec4<f32>(light, trans);
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
    var px = clamp(vec2<i32>(p), vec2<i32>(0), dims - vec2<i32>(1));
    var uv = p / vec2<f32>(dims);
    if (post.misc.z > 0.5) {
        uv += textureLoad(distort_tex, px, 0).xy;
        // Outlines and GI follow the warped image.
        px = clamp(vec2<i32>(uv * vec2<f32>(dims)), vec2<i32>(0), dims - vec2<i32>(1));
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

    if (post.outline.w > 0.5 && aimed(post.filt5.x, object_w(textureLoad(normal_ms, px, 0).w))) {
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
    if (post.air.x > 0.0 || post.air2.y > 0.0) {
        let a = air_light(px, dims);
        col = col * a.a + a.rgb;
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

// Pixel art: the normal buffer's group is negative on objects drawn as pixel art.
fn group_at(p: vec2<f32>, dims: vec2<i32>) -> f32 {
    return textureLoad(normal_ms, clamp(vec2<i32>(p), vec2<i32>(0), dims - vec2<i32>(1)), 0).w;
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

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

// Smooth value noise (0..1).
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// The picture at pixel `q`, cheaply (display colour): bloom, exposure, tonemap and saturation
// but no GI, outlines, distortion or haze. Styles that look at a neighbourhood read it.
fn base_color(q: vec2<i32>, dims: vec2<i32>) -> vec3<f32> {
    let qq = clamp(q, vec2<i32>(0), dims - vec2<i32>(1));
    var c = textureLoad(hdr_tex, qq, 0).rgb;
    if (post.misc.y > 0.0) {
        c += textureSampleLevel(bloom_tex, lin, (vec2<f32>(qq) + 0.5) / vec2<f32>(dims), 0.0).rgb * post.misc.y;
    }
    c = tonemap(c * post.tone.x, post.tone.y);
    return to_srgb(max(mix(vec3<f32>(luma(c)), c, post.tone.w), vec3<f32>(0.0)));
}

// Oil paint: a generalized Kuwahara filter (Papari et al., with the polynomial sector weights
// of Kyprianidis et al.). Around the pixel, eight overlapping pie-slice sectors each average
// their colours; a sector whose colours vary little (calm paint) gets almost all the weight,
// so areas come out as flat strokes while edges stay sharp (a blur would smear them). With a
// part set, samples from the other part of the scene are skipped: no paint bleeds across.
fn paint(c: vec2<f32>, dims: vec2<i32>, radius: f32, part: f32) -> vec3<f32> {
    let r = i32(clamp(radius, 2.0, 8.0));
    let rf = f32(r);
    let zeta = 2.0 / rf;
    let zc = 0.58;
    let eta = (zeta + cos(zc)) / (sin(zc) * sin(zc));
    var m: array<vec4<f32>, 8>;
    var s: array<vec3<f32>, 8>;
    var w: array<f32, 8>;
    let ci = vec2<i32>(c);
    for (var j = -r; j <= r; j++) {
        for (var i = -r; i <= r; i++) {
            var v = vec2<f32>(f32(i), f32(j)) * 0.5 / rf;
            if (dot(v, v) > 0.25) {
                continue;
            }
            let q = clamp(ci + vec2<i32>(i, j), vec2<i32>(0), dims - vec2<i32>(1));
            if (part > 0.5 && !aimed(part, object_w(textureLoad(normal_ms, q, 0).w))) {
                continue;
            }
            let col = base_color(q, dims);
            var sum = 0.0;
            var vxx = zeta - eta * v.x * v.x;
            var vyy = zeta - eta * v.y * v.y;
            var z = max(0.0, v.y + vxx);
            w[0] = z * z;
            z = max(0.0, -v.x + vyy);
            w[2] = z * z;
            z = max(0.0, -v.y + vxx);
            w[4] = z * z;
            z = max(0.0, v.x + vyy);
            w[6] = z * z;
            v = 0.70710678 * vec2<f32>(v.x - v.y, v.x + v.y);
            vxx = zeta - eta * v.x * v.x;
            vyy = zeta - eta * v.y * v.y;
            z = max(0.0, v.y + vxx);
            w[1] = z * z;
            z = max(0.0, -v.x + vyy);
            w[3] = z * z;
            z = max(0.0, -v.y + vxx);
            w[5] = z * z;
            z = max(0.0, v.x + vyy);
            w[7] = z * z;
            for (var k = 0; k < 8; k++) {
                sum += w[k];
            }
            let gw = exp(-3.125 * dot(v, v)) / max(sum, 1e-6);
            for (var k = 0; k < 8; k++) {
                let wk = w[k] * gw;
                m[k] += vec4<f32>(col * wk, wk);
                s[k] += col * col * wk;
            }
        }
    }
    var acc = vec4<f32>(0.0);
    for (var k = 0; k < 8; k++) {
        if (m[k].w <= 0.0) {
            continue;
        }
        let mean = m[k].rgb / m[k].w;
        let v3 = abs(s[k] / m[k].w - mean * mean);
        let wk = 1.0 / (1.0 + pow(8000.0 * (v3.r + v3.g + v3.b), 4.0));
        acc += vec4<f32>(mean * wk, wk);
    }
    if (acc.w <= 0.0) {
        return base_color(ci, dims);
    }
    return acc.rgb / acc.w;
}

// Brush strokes: streaky noise stretched along the edges of the picture (across its
// brightness gradient), swirling slowly where the picture is flat, lit from the top left so
// the paint's ridges catch the light (impasto). Returns a brightness factor around 1.
fn brush(p: vec2<f32>, dims: vec2<i32>, size: f32) -> f32 {
    let o = i32(max(size, 2.0));
    let q = vec2<i32>(p);
    let gx = luma(base_color(q + vec2<i32>(o, 0), dims)) - luma(base_color(q - vec2<i32>(o, 0), dims));
    let gy = luma(base_color(q + vec2<i32>(0, o), dims)) - luma(base_color(q - vec2<i32>(0, o), dims));
    let swirl = vnoise(p / 160.0) * 3.1415927 + 0.6;
    var dir = vec2<f32>(cos(swirl), sin(swirl));
    let g = length(vec2<f32>(gx, gy));
    if (g > 0.02) {
        dir = normalize(mix(dir, vec2<f32>(-gy, gx) / g, smoothstep(0.02, 0.08, g)));
    }
    let across = vec2<f32>(-dir.y, dir.x);
    let len = max(size, 2.0) * 3.5;
    let wid = max(size, 2.0) * 0.6;
    let uv = vec2<f32>(dot(p, dir) / len, dot(p, across) / wid);
    // Strokes (blobs drawn out along the stroke) and bristle streaks inside them.
    let h = vnoise(uv) * 0.6 + vnoise(uv * 2.3 + 17.0) * 0.4;
    let h2 = vnoise(uv + vec2<f32>(0.0, 0.35)) * 0.6 + vnoise((uv + vec2<f32>(0.0, 0.35)) * 2.3 + 17.0) * 0.4;
    let bristle = vnoise(vec2<f32>(uv.x * 0.8, uv.y * 7.0) + 31.0) - 0.5;
    let ridge = (h - h2) * dot(across, normalize(vec2<f32>(-1.0, -1.0)));
    return 0.88 + 0.22 * h + 0.12 * bristle + ridge * 0.45;
}

// Ink amount of one process colour for a display colour: 0 cyan, 1 magenta, 2 yellow, 3 black.
fn cmyk(c: vec3<f32>, ink: i32) -> f32 {
    let k = 1.0 - max(c.r, max(c.g, c.b));
    if (ink == 3) {
        return smoothstep(0.25, 1.0, k);
    }
    let v = (1.0 - c[ink] - k) / max(1.0 - k, 1e-3);
    return clamp(v, 0.0, 1.0);
}

// Ink coverage at `p` of a dot screen turned by `angle`, its dots `cell` pixels apart. Each
// dot reads the picture at its own centre and grows with the ink there (dot area ~ ink), so
// dots stay round and merge in the dark. `ink` < 0 = black ink from brightness alone.
fn dot_screen(p: vec2<f32>, dims: vec2<i32>, angle: f32, cell: f32, ink: i32) -> f32 {
    let cs = cos(angle);
    let sn = sin(angle);
    let q = vec2<f32>(cs * p.x + sn * p.y, -sn * p.x + cs * p.y) / cell;
    let cq = floor(q) + 0.5;
    let centre = vec2<f32>(cs * cq.x - sn * cq.y, sn * cq.x + cs * cq.y) * cell;
    // Print lightens the midtones a little so the paper shows.
    let col = pow(base_color(vec2<i32>(centre), dims), vec3<f32>(0.75));
    var amount = 1.0 - luma(col);
    if (ink >= 0) {
        amount = cmyk(col, ink);
    }
    let radius = sqrt(amount) * 0.66;
    let aa = 0.8 / cell;
    // Anti-aliased edge; dots smaller than the edge fade out instead of leaving specks.
    return clamp((radius - length(q - cq)) / (2.0 * aa) + 0.5, 0.0, 1.0) * clamp(radius / aa, 0.0, 1.0);
}

// Print: process-colour dot screens at their classic angles (C 15, M 75, Y 0, K 45 degrees)
// on cream paper; with no colour kept, black dots only (newsprint).
fn halftone(p: vec2<f32>, dims: vec2<i32>, size: f32, keep: f32) -> vec3<f32> {
    let cell = clamp(size, 3.0, 24.0);
    let paper = vec3<f32>(0.96, 0.93, 0.85);
    var color = paper;
    if (keep > 0.0) {
        color *= mix(vec3<f32>(1.0), vec3<f32>(0.12, 0.72, 0.94), dot_screen(p, dims, 0.2618, cell, 0));
        color *= mix(vec3<f32>(1.0), vec3<f32>(0.94, 0.2, 0.58), dot_screen(p, dims, 1.309, cell, 1));
        color *= mix(vec3<f32>(1.0), vec3<f32>(1.0, 0.93, 0.12), dot_screen(p, dims, 0.0, cell, 2));
        color *= mix(vec3<f32>(1.0), vec3<f32>(0.12, 0.11, 0.13), dot_screen(p, dims, 0.7854, cell, 3));
    }
    var mono = paper;
    if (keep < 1.0) {
        mono *= mix(vec3<f32>(1.0), vec3<f32>(0.1, 0.1, 0.12), dot_screen(p, dims, 0.7854, cell, -1));
    }
    return mix(mono, color, keep);
}

// Text mode: the screen becomes a grid of character cells; each cell's brightness picks a
// character from a ramp of 12, from blank to '@', drawn from 5x7 bitmaps packed into
// integers (rows 0-3 in the first table, 4-6 in the second, one bit per dot).
fn ascii(p: vec2<f32>, dims: vec2<i32>, size: f32, keep: f32) -> vec3<f32> {
    let cw = clamp(size, 4.0, 24.0);
    let csz = vec2<f32>(cw, round(cw * 1.4));
    let cell = floor(p / csz);
    let centre = (cell + 0.5) * csz;
    var col = vec3<f32>(0.0);
    col += base_color(vec2<i32>(centre + csz * vec2<f32>(-0.25, -0.25)), dims);
    col += base_color(vec2<i32>(centre + csz * vec2<f32>(0.25, -0.25)), dims);
    col += base_color(vec2<i32>(centre + csz * vec2<f32>(-0.25, 0.25)), dims);
    col += base_color(vec2<i32>(centre + csz * vec2<f32>(0.25, 0.25)), dims);
    col *= 0.25;
    let l = luma(col);
    let idx = u32(clamp(floor(pow(l, 0.8) * 12.0), 0.0, 11.0));
    let f = (p - cell * csz) / csz;
    let gx = i32(floor(f.x * 7.0)) - 1;
    let gy = i32(floor(f.y * 9.0)) - 1;
    var on = false;
    if (gx >= 0 && gx < 5 && gy >= 0 && gy < 7) {
        // " .-:+=o&*8#@"
        var top = array<u32, 12>(0u, 0u, 491520u, 6336u, 1020032u, 31744u, 571392u, 206118u, 1030816u, 476718u, 359754u, 718382u);
        var bottom = array<u32, 12>(0u, 6336u, 0u, 198u, 132u, 31u, 14897u, 22837u, 686u, 14897u, 10591u, 30781u);
        var bits = 0u;
        if (gy < 4) {
            bits = top[idx] >> u32(gx + 5 * gy);
        } else {
            bits = bottom[idx] >> u32(gx + 5 * (gy - 4));
        }
        on = (bits & 1u) == 1u;
    }
    let phosphor = vec3<f32>(0.3, 1.0, 0.45) * (0.45 + 0.75 * l);
    let vivid = col / max(max(col.r, max(col.g, col.b)), 0.2) * (0.55 + 0.45 * l);
    let ink = mix(phosphor, vivid, keep);
    let ground = mix(vec3<f32>(0.01, 0.035, 0.018), col * 0.14, keep);
    return select(ground, ink, on);
}

// One family of parallel pencil lines across direction `dir`, `spacing` pixels apart,
// shifted by `wob` (a fraction of the spacing). 1 on a line, 0 between.
fn hatch(p: vec2<f32>, dir: vec2<f32>, spacing: f32, wob: f32) -> f32 {
    let d = abs(fract(dot(p, dir) / spacing + wob) - 0.5) * spacing;
    return 1.0 - smoothstep(0.3, 1.1, d);
}

// Pencil on paper: crossing families of hatch lines appear as the picture gets darker (a
// tonal art map), contours where the brightness changes quickly (Sobel) or the outline pass
// darkened the picture, and lines that wobble and redraw six times a second. The colour kept
// tints the paper like coloured pencil.
fn sketch(p: vec2<f32>, s: vec3<f32>, bp: vec3<f32>, dims: vec2<i32>, size: f32, keep: f32) -> vec3<f32> {
    let sp = clamp(size, 2.0, 16.0);
    let boil = floor(post.filt3.w * 6.0);
    let wob = (vnoise(p * 0.03 + vec2<f32>(boil * 7.31, boil * 3.17)) - 0.5) * 0.9;
    // Tone: lifted so midtones stay light, then each darker band adds a family of lines.
    let dark = 1.0 - pow(luma(s), 0.6);
    var ink = hatch(p, vec2<f32>(0.70710678, 0.70710678), sp, wob) * smoothstep(0.30, 0.42, dark);
    ink = max(ink, hatch(p, vec2<f32>(0.70710678, -0.70710678), sp, wob) * smoothstep(0.50, 0.62, dark));
    ink = max(ink, hatch(p, vec2<f32>(0.0, 1.0), sp * 0.75, wob) * smoothstep(0.68, 0.8, dark));
    let o = max(i32(sp * 0.3), 1);
    let q = vec2<i32>(p);
    let tl = luma(base_color(q + vec2<i32>(-o, -o), dims));
    let tt = luma(base_color(q + vec2<i32>(0, -o), dims));
    let tr = luma(base_color(q + vec2<i32>(o, -o), dims));
    let ml = luma(base_color(q + vec2<i32>(-o, 0), dims));
    let mr = luma(base_color(q + vec2<i32>(o, 0), dims));
    let bl = luma(base_color(q + vec2<i32>(-o, o), dims));
    let bb = luma(base_color(q + vec2<i32>(0, o), dims));
    let br = luma(base_color(q + vec2<i32>(o, o), dims));
    let gx = (tr + 2.0 * mr + br) - (tl + 2.0 * ml + bl);
    let gy = (bl + 2.0 * bb + br) - (tl + 2.0 * tt + tr);
    ink = max(ink, smoothstep(0.2, 0.5, length(vec2<f32>(gx, gy))));
    ink = max(ink, smoothstep(0.08, 0.25, luma(bp) - luma(s)));
    let paper = vec3<f32>(0.95, 0.93, 0.87) * (0.95 + 0.05 * vnoise(p * 0.8));
    // Coloured pencil: the paper takes the picture's hue (not its darkness, that is the lines').
    let hue = s / max(max(s.r, max(s.g, s.b)), 0.05);
    let wash = paper * mix(vec3<f32>(1.0), mix(hue, vec3<f32>(1.0), 0.45), keep);
    let lead = mix(vec3<f32>(0.2, 0.2, 0.24), s * 0.45, keep * 0.6);
    return mix(wash, lead, ink * 0.85);
}

// A painterly or print style over the finished picture `s` at `p`.
fn stylize(kind: u32, p: vec2<f32>, s: vec3<f32>, dims: vec2<i32>) -> vec3<f32> {
    let size = max(post.filt7.y, 1.0);
    let keep = clamp(post.filt7.w, 0.0, 1.0);
    let bp = base_color(vec2<i32>(p), dims);
    // Outlines, bounce light and occlusion darken the cheap picture by this much here.
    let shade = clamp((luma(s) + 0.03) / (luma(bp) + 0.03), 0.0, 1.0);
    if (kind == 1u) {
        // The paint plus what the outlines and GI added here, then brush strokes and canvas.
        var c = paint(p, dims, size, post.filt6.z) + (s - bp);
        c *= brush(p, dims, size);
        c *= 1.0 + 0.03 * sin(p.x * 1.9) * sin(p.y * 1.9);
        let under = vec3<f32>(0.42, 0.3, 0.2) * (0.3 + 1.4 * luma(c));
        return mix(under, c, keep);
    }
    if (kind == 2u) {
        return halftone(p, dims, size, keep) * shade;
    }
    if (kind == 3u) {
        return ascii(p, dims, size, keep);
    }
    return sketch(p, s, bp, dims, size, keep);
}

// How much a screen transition covers the pixel at `frag` (1 = covered).
fn transition_cover(frag: vec2<f32>, fd: vec2<f32>) -> f32 {
    let t = clamp(post.filt8.x, 0.0, 1.0);
    let kind = u32(post.filt8.y + 0.5);
    if (kind == 1u) {
        // Iris: a circle around the centre shrinks to nothing.
        let c = post.filt8.zw * fd;
        let far = max(max(length(c), length(c - vec2<f32>(fd.x, 0.0))), max(length(c - vec2<f32>(0.0, fd.y)), length(c - fd)));
        let r = (1.0 - t) * (far + 2.0);
        return smoothstep(r - 1.5, r + 1.5, length(frag - c));
    }
    if (kind == 2u) {
        // Diamonds grow in a wave from left to right.
        let q = abs(fract(frag / 56.0) - 0.5) * 2.0;
        let sweep = t * 2.0 - frag.x / fd.x;
        return smoothstep(-0.03, 0.03, sweep - (q.x + q.y) * 0.5);
    }
    if (kind == 3u) {
        // Random blocks, each with its own moment.
        return step(hash2(floor(frag / 6.0)), t * 1.05 - 0.025);
    }
    if (kind == 4u) {
        // Mosaic: the pixels have grown (in fs_post); then fade to black.
        return smoothstep(0.6, 1.0, t);
    }
    if (kind == 5u) {
        // Blinds close.
        return step(fract(frag.y / 36.0), t);
    }
    return t;
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
        var block = max(post.filt0.x, 1.0);
        if (post.filt8.x > 0.0 && u32(post.filt8.y + 0.5) == 4u) {
            block = max(block, floor(1.0 + post.filt8.x * post.filt8.x * 48.0));
        }
        if (block > 1.0) {
            p = (floor(p / block) + 0.5) * block;
        }
        // Pixel art for marked objects only: a block shows its centre's colour where this
        // pixel or the centre is marked, so marked silhouettes step in whole blocks while
        // everything else stays sharp.
        let art_px = post.filt4.y;
        var art = false;
        var art_edge = false;
        if (art_px > 1.0) {
            let c = (floor(p / art_px) + 0.5) * art_px;
            let gc = group_at(c, dims);
            if (gc < 0.0 || group_at(p, dims) < 0.0) {
                p = c;
                art = true;
            }
            // A one-block outline just outside marked silhouettes: a block whose neighbour
            // is a different marked object in front of it turns dark.
            if (post.filt4.w > 0.5) {
                let dc = view_depth(clamp(vec2<i32>(c), vec2<i32>(0), dims - vec2<i32>(1)), 0);
                var offs = array<vec2<f32>, 4>(vec2<f32>(1.0, 0.0), vec2<f32>(-1.0, 0.0), vec2<f32>(0.0, 1.0), vec2<f32>(0.0, -1.0));
                for (var i = 0; i < 4; i++) {
                    let q = c + offs[i] * art_px;
                    let gq = group_at(q, dims);
                    let qi = clamp(vec2<i32>(q), vec2<i32>(0), dims - vec2<i32>(1));
                    if (gq < 0.0 && gq != gc && view_depth(qi, 0) < dc - 0.05) {
                        art_edge = true;
                    }
                }
                if (art_edge) {
                    p = c;
                    art = true;
                }
            }
        }
        // Which part of the scene this pixel shows (after pixelation, so whole blocks agree).
        let obj = object_w(group_at(p, dims));
        s = to_srgb(scene_color(p, dims, select(0.0, post.filt3.z, aimed(post.filt6.y, obj))));
        if (art) {
            let al = post.filt4.z;
            if (al >= 2.0) {
                s = floor(s * (al - 1.0) + 0.5) / (al - 1.0);
            }
            if (art_edge) {
                s = s * 0.18;
            }
        }
        // Painterly and print styles.
        let sty = u32(post.filt7.x + 0.5);
        if (sty > 0u && aimed(post.filt6.z, obj)) {
            s = mix(s, clamp(stylize(sty, p, s, dims), vec3<f32>(0.0), vec3<f32>(1.0)), clamp(post.filt7.z, 0.0, 1.0));
        }
        // Colour grading (display space).
        if (aimed(post.filt5.z, obj)) {
            let temp = post.filt2.x;
            s = s * vec3<f32>(1.0 + 0.12 * temp, 1.0 + 0.08 * post.filt2.y, 1.0 - 0.12 * temp);
            s = (s - 0.5) * post.filt2.z + 0.5;
            s = s * post.filt2.w;
            let gray = dot(s, vec3<f32>(0.2126, 0.7152, 0.0722));
            s = mix(vec3<f32>(gray), s, post.filt4.x);
        }
        // Scanlines follow the curved tube.
        if (post.filt0.z > 0.0 && aimed(post.filt5.w, obj)) {
            let period = max(post.filt0.w, 1.0);
            let w = 0.5 - 0.5 * cos(uvn.y * fd.y / period * 6.2831853);
            s = s * (1.0 - post.filt0.z * 0.6 * w);
        }
        // Vignette and film grain.
        if (post.filt3.x > 0.0) {
            let r = length(frag.xy / fd - 0.5) * 1.414;
            s = s * (1.0 - post.filt3.x * smoothstep(0.35, 1.05, r));
        }
        if (post.filt3.y > 0.0 && aimed(post.filt6.x, obj)) {
            let h = fract(sin(dot(frag.xy + vec2<f32>(post.filt3.w * 61.0, post.filt3.w * 17.0), vec2<f32>(12.9898, 78.233))) * 43758.5453);
            s = s + (h - 0.5) * post.filt3.y * 0.16;
        }
        s = clamp(s, vec3<f32>(0.0), vec3<f32>(1.0));
        // Ordered dithering to a few levels or a fixed palette.
        let pal = u32(post.filt1.z + 0.5);
        let levels = post.filt1.y;
        // The dither pattern steps with the pixels: pixel-art blocks or the whole screen's.
        let bp = floor(frag.xy / select(block, art_px, art));
        let th = mix(0.5, bayer4i(bp), clamp(post.filt1.x, 0.0, 1.0));
        if (aimed(post.filt5.y, obj)) {
            if (pal > 0u) {
                s = palette_nearest(s, pal, th);
            } else if (levels >= 2.0) {
                let l = levels - 1.0;
                s = floor(s * l + th) / l;
            }
        }
    }
    if (post.filt8.x > 0.0) {
        s *= 1.0 - transition_cover(frag.xy, fd);
    }
    if (split > 0.0 && abs(frag.x - split * fd.x) < 1.5) {
        s = vec3<f32>(1.0);
    }
    if (post.tone.z > 0.5) {
        return vec4<f32>(s, 1.0);
    }
    return vec4<f32>(to_linear(s), 1.0);
}
