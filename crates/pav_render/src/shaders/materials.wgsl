// Procedural surface materials (asset plan §6.4). One fixed pattern library; materials are rows
// of a storage buffer, so an edit is a 368-byte upload and never rebuilds a pipeline. This file
// is concatenated in front of scene.wgsl (it reads `g`, the globals declared there).
//
// No derivatives anywhere: anti-aliasing uses the analytic pixel footprint in `g.params.w`, so
// the code may run in non-uniform control flow (inside loops, after fs_sdf's discard).
//
// `pav_core::material::eval_packed` is the CPU port of `eval_material`: the same hash, the same
// f32 operations in the same order. Keep the two in step; `pav_view/tests/material_parity.rs`
// compares them pixel by pixel.

struct MatLayer {
    head: vec4<u32>,   // x: kernel | variant<<8 | axis<<12 | facing<<16 | mask<<20 | paint<<24 | ncells<<28 ; y: salt
    paint: vec4<f32>,  // rgb linear colour (paint 0) or r = linear shade factor (paint 1); a = strength
    a: vec4<f32>,
    b: vec4<f32>,      // x: the kernel's; yzw: the cells' mean linear colour (layers with cells)
    c: vec4<f32>,      // x vary, y cavity, z glow, w bevel
    cells: vec4<u32>,  // up to 4 sRGB8 colours, r in the low byte
    dir: vec4<f32>,    // xyz unit axis for 1D patterns
};

struct Material {
    head: vec4<u32>,   // x layer count, y map (0 object, 1 world, 2 axis), z bit 0: turn
    pbr: vec4<f32>,    // x roughness (< 0 = legacy shading), y metallic, z 1/scale
    layers: array<MatLayer, 3>,
};

// Bindings 0-1 of group 1 are the font's (text pipeline); the mesh and SDF pipelines bind this.
@group(1) @binding(2) var<storage, read> materials: array<Material>;

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}
fn hash3i(x: i32, y: i32, z: i32, salt: u32) -> u32 {
    return pcg((bitcast<u32>(x) * 0x8da6b343u) ^ (bitcast<u32>(y) * 0xd8163841u) ^ (bitcast<u32>(z) * 0xcb1ab31fu) ^ salt);
}
fn rnd_lo(h: u32) -> f32 {
    return f32(h & 0xffffu) * (1.0 / 65536.0);
}
fn rnd(h: u32) -> f32 {
    return f32(h >> 8u) * (1.0 / 16777216.0);
}
fn vnoise(p: vec3<f32>, salt: u32) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = p - floor(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(rnd(hash3i(i.x, i.y, i.z, salt)), rnd(hash3i(i.x + 1, i.y, i.z, salt)), u.x);
    let b = mix(rnd(hash3i(i.x, i.y + 1, i.z, salt)), rnd(hash3i(i.x + 1, i.y + 1, i.z, salt)), u.x);
    let c = mix(rnd(hash3i(i.x, i.y, i.z + 1, salt)), rnd(hash3i(i.x + 1, i.y, i.z + 1, salt)), u.x);
    let d = mix(rnd(hash3i(i.x, i.y + 1, i.z + 1, salt)), rnd(hash3i(i.x + 1, i.y + 1, i.z + 1, salt)), u.x);
    return mix(mix(a, b, u.y), mix(c, d, u.y), u.z);
}
// Fractal noise in [0,1]; octaves smaller than ~2 pixels fade to their mean (0.5).
fn fbm(p: vec3<f32>, octaves: u32, size: f32, fw: f32, salt: u32) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var norm = 0.0;
    var q = p;
    var s = size;
    for (var o = 0u; o < octaves; o++) {
        let k = smoothstep(1.0, 3.0, s / max(fw, 1e-5));
        if (k <= 0.0) {
            // This and all finer octaves are below a pixel: they average to 0.5.
            for (var r = o; r < octaves; r++) {
                sum += amp * 0.5;
                norm += amp;
                amp *= 0.5;
            }
            break;
        }
        sum += amp * mix(0.5, vnoise(q, salt + o * 7u), k);
        norm += amp;
        amp *= 0.5;
        q = q * 2.03;
        s = s / 2.03;
    }
    return sum / norm;
}
// Box-filtered coverage of a band of width w centred at d = 0, seen through a pixel of width fw.
fn pulse(d: f32, w: f32, fw: f32) -> f32 {
    let f = max(fw, 1e-5);
    let lo = max(d - 0.5 * f, -0.5 * w);
    let hi = min(d + 0.5 * f, 0.5 * w);
    return clamp((hi - lo) / f, 0.0, 1.0);
}
fn fade(feature: f32, fw: f32) -> f32 {
    return smoothstep(1.0, 3.0, feature / max(fw, 1e-5));
}
fn srgb8_to_linear(v: u32) -> vec3<f32> {
    let c = unpack4x8unorm(v).rgb;
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}
// 2D coordinates on the dominant plane, with the layer's long axis along u.
fn plane_uv(p: vec3<f32>, n: vec3<f32>, axis: u32) -> vec2<f32> {
    let a = abs(n);
    var uv: vec2<f32>;
    var vaxis = 1u;
    if (a.x >= a.y && a.x >= a.z) {
        uv = p.zy;
    } else if (a.y >= a.z) {
        uv = p.xz;
        vaxis = 2u;
    } else {
        uv = p.xy;
    }
    if (axis == vaxis) {
        uv = uv.yx;
    }
    return uv;
}

struct Pat {
    m: f32,     // paint mask (anti-aliased)
    mean: f32,  // mask when unresolved
    feat: f32,  // feature size (m) for fading
    cell: u32,  // per-cell hash (0 = not cellular)
    d: f32,     // distance to the nearest cell border (m), for cavity
    gap: f32,
};

fn pat_grid(uv: vec2<f32>, l: MatLayer, fw: f32, salt: u32, random_offset: bool) -> Pat {
    let w = l.a.x;
    let h = l.a.y;
    let gap = l.a.z;
    let row = floor(uv.y / h);
    var off = l.a.w * w * (row - 2.0 * floor(row * 0.5));
    if (random_offset) {
        off = rnd(hash3i(i32(row), 7, 0, salt)) * w;
    }
    let x = (uv.x - off) / w;
    let col = floor(x);
    let fu = x - col;
    let fv = uv.y / h - row;
    let du = min(fu, 1.0 - fu) * w;
    let dv = min(fv, 1.0 - fv) * h;
    var o: Pat;
    o.m = max(pulse(du, gap, fw), pulse(dv, gap, fw));
    o.mean = 1.0 - (1.0 - min(gap / w, 1.0)) * (1.0 - min(gap / h, 1.0));
    o.feat = min(w, h);
    o.cell = hash3i(i32(col), i32(row), 1, salt) | 1u;
    o.d = min(du, dv);
    o.gap = gap;
    return o;
}

fn pat_cells(uv: vec2<f32>, l: MatLayer, variant: u32, fw: f32, salt: u32) -> Pat {
    let size = l.a.x;
    let jit = l.a.y;
    let gap = l.a.z;
    let q = uv / size;
    let ic = floor(q);
    var d1 = 1e9;
    var d2 = 1e9;
    var bid = vec2<i32>(0);
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let c = ic + vec2<f32>(f32(i), f32(j));
            let ci = vec2<i32>(c);
            let h = hash3i(ci.x, ci.y, 2, salt);
            let fp = c + 0.5 + jit * (vec2<f32>(rnd(h), rnd_lo(h)) - 0.5);
            let dd = length(fp - q);
            if (dd < d1) {
                d2 = d1;
                d1 = dd;
                bid = ci;
            } else if (dd < d2) {
                d2 = dd;
            }
        }
    }
    var o: Pat;
    o.cell = hash3i(bid.x, bid.y, 3, salt) | 1u;
    o.feat = size;
    let edge = 0.5 * (d2 - d1) * size; // approximate distance to the cell border (m)
    o.d = edge;
    o.gap = gap;
    if (variant == 0u) {
        // spots: discs of radius r (a fraction of a cell) around the feature points
        let r = l.b.x * size;
        o.m = pulse(d1 * size, 2.0 * r, fw);
        o.mean = l.a.w;
        o.feat = 2.0 * r;
    } else if (variant == 1u) {
        o.m = pulse(edge, gap, fw);
        o.mean = min(2.2 * gap / size, 1.0);
    } else {
        o.m = step(rnd(pcg(o.cell)), l.a.w);
        o.mean = l.a.w;
    }
    return o;
}

fn pat_shingles(uv: vec2<f32>, l: MatLayer, variant: u32, fw: f32, salt: u32) -> Pat {
    let w = l.a.x;
    let h = l.a.y;
    let gap = l.a.z;
    var row = floor(uv.y / h);
    let fv = uv.y / h - row;
    var off = l.a.w * w * (row - 2.0 * floor(row * 0.5));
    var x = (uv.x - off) / w;
    var col = floor(x);
    var fu = x - col - 0.5;
    // Tongue: the tile's lower edge rises from its centre to its sides.
    var edge = 0.0;
    if (variant == 0u) {
        edge = 0.5 - sqrt(max(0.25 - fu * fu, 0.0));
    } else if (variant == 2u) {
        edge = abs(fu);
    }
    let rise = edge * w; // metres above the row bottom
    var dy = fv * h - rise;
    if (dy < 0.0) {
        // Below this tile's tongue: the exposed part of the tile in the row below.
        row = row - 1.0;
        off = l.a.w * w * (row - 2.0 * floor(row * 0.5));
        x = (uv.x - off) / w;
        col = floor(x);
        fu = x - col - 0.5;
        dy = dy + h;
    }
    let dside = (0.5 - abs(fu)) * w;
    var o: Pat;
    // Outline along the tongue edge (distance taken vertically) and the tile sides.
    o.m = max(pulse(min(dy, h - dy), gap, fw), pulse(dside, gap, fw));
    o.mean = min(gap / h + gap / w, 1.0);
    o.feat = min(w, h);
    o.cell = hash3i(i32(col), i32(row), 4, salt) | 1u;
    // Cavity: the shadow just under the overlapping tile edge above.
    o.d = h - dy;
    o.gap = gap;
    return o;
}

fn magic(p: vec3<f32>, depth: u32, distort: f32) -> f32 {
    var x = sin((p.x + p.y + p.z) * 5.0);
    var y = cos((-p.x + p.y - p.z) * 5.0);
    var z = -cos((-p.x - p.y + p.z) * 5.0);
    if (depth > 0u) {
        x *= distort;
        y = -cos(x - y - z) * distort;
    }
    if (depth > 1u) {
        z = sin(-x - y - z) * distort;
        x = -cos(-x + y - z) * distort;
    }
    if (depth > 2u) {
        y = -sin(-x + y + z) * distort;
        z = cos(x + y + z) * distort;
    }
    if (depth > 3u) {
        x = cos(-x - y + z) * distort;
    }
    if (distort != 0.0) {
        x = x / distort;
    }
    return 0.5 + 0.5 * x;
}

struct MatOut {
    albedo: vec3<f32>,
    glow: f32,
};

// Evaluates material row `mid` over `base` (the instance colour). `p_loc`/`n_loc`: the part's
// local metric position and normal (object and axis maps); `wp`/`wn`: world position and
// normal; `ext`: the part's half extents; `seed`: the instance's 20-bit seed.
fn eval_material(mid: u32, seed: u32, base: vec3<f32>, p_loc: vec3<f32>, n_loc: vec3<f32>, wp: vec3<f32>, wn: vec3<f32>, ext: vec3<f32>) -> MatOut {
    let mhead = materials[mid].head;
    let count = min(mhead.x, 3u);
    let map = mhead.y;
    let inv_scale = materials[mid].pbr.z;
    var p = p_loc;
    var n = n_loc;
    if (map == 1u) {
        p = wp;
        n = wn;
    }
    // Analytic footprint: metres per pixel on the surface (`material::footprint` on the CPU).
    let to_eye = g.eye.xyz - wp;
    let dist = select(1.0, length(to_eye), g.eye.w > 0.5);
    let vdir = select(-g.forward.xyz, to_eye / max(length(to_eye), 1e-4), g.eye.w > 0.5);
    let fw0 = g.params.w * dist / max(abs(dot(wn, vdir)), 0.25) * inv_scale;
    p = p * inv_scale;
    let p3 = p - n * 1e-3; // inside the surface, so lattice faces never land on it
    var c = base;
    var glow = 0.0;
    var prev = 0.0;
    for (var li = 0u; li < count; li++) {
        let l = materials[mid].layers[li];
        let kind = l.head.x & 0xffu;
        let variant = (l.head.x >> 8u) & 0xfu;
        let axis = (l.head.x >> 12u) & 0xfu;
        let facing = (l.head.x >> 16u) & 0xfu;
        let maskby = (l.head.x >> 20u) & 0xfu;
        let paint = (l.head.x >> 24u) & 0xfu;
        let ncells = (l.head.x >> 28u) & 0xfu;
        let salt = l.head.y + seed * 0x9e3779b9u;
        let fw = fw0;
        var uv: vec2<f32>;
        if (map == 2u && abs(n.y) < 0.7) {
            // around the part's local y axis: u = arc length, v = height
            let r = max(ext.x, 1e-3) * inv_scale;
            uv = vec2<f32>(atan2(p.x, p.z) * r, p.y);
            if (axis == 1u) {
                uv = uv.yx;
            }
        } else {
            uv = plane_uv(p, n, axis);
        }
        if ((mhead.z & 1u) != 0u) {
            uv = uv.yx; // material `turn`: 2D patterns run the other way
        }
        var pt: Pat;
        pt.cell = 0u;
        pt.d = 1e9;
        pt.gap = 0.0;
        // Feature size and mean coverage from the parameters alone: a layer whose features
        // are below a pixel is replaced by its mean without evaluating the pattern.
        var feat = 1e9;
        var mean = 0.0;
        var cav_mean = 0.0;
        switch kind {
            case 1u, 8u: { feat = l.a.x * select(0.5, 0.3, kind == 8u); mean = l.a.w; }
            case 2u: {
                // spots: the spot's diameter
                feat = select(l.a.x, 2.0 * l.b.x * l.a.x, variant == 0u);
                mean = select(select(l.a.w, min(2.2 * l.a.z / l.a.x, 1.0), variant == 1u), l.a.w, variant == 0u);
                cav_mean = l.c.y * min(l.c.w / feat, 1.0) * 0.5;
            }
            case 3u, 10u: {
                feat = min(l.a.x, l.a.y);
                mean = 1.0 - (1.0 - min(l.a.z / l.a.x, 1.0)) * (1.0 - min(l.a.z / l.a.y, 1.0));
                cav_mean = l.c.y * min(l.c.w / feat, 1.0) * 0.5;
            }
            case 4u: { feat = l.a.x; mean = 0.5; }
            case 6u, 7u: { feat = l.a.x * min(l.a.y, 1.0 - l.a.y); mean = l.a.y; }
            case 9u: { feat = 2.0 * l.a.y; mean = min(3.14159 * l.a.y * l.a.y / (l.a.x * l.a.x), 1.0); }
            default: {}
        }
        let resolved = fade(feat, fw) > 0.0;
        if (!resolved) {
            pt.m = mean;
            pt.mean = mean;
            pt.feat = feat;
            if (ncells > 0u) {
                c = l.b.yzw; // the cells' mean colour
            }
            c *= (1.0 - cav_mean) * (1.0 - cav_mean);
        }
        switch select(0u, kind, resolved) {
            case 1u: { // noise
                var q = p3 / l.a.x;
                if (l.a.z > 0.0) {
                    q += l.a.z * (vnoise(q * 0.5 + 17.3, salt + 99u) - 0.5);
                }
                let v = fbm(q, u32(l.a.y), l.a.x, fw, salt);
                pt.m = smoothstep(l.b.x - l.b.y, l.b.x + l.b.y, v);
                pt.mean = l.a.w;
                pt.feat = l.a.x * 0.5;
            }
            case 2u: { // voronoi, cobbles
                pt = pat_cells(uv, l, variant, fw, salt);
            }
            case 3u: { // brick, tiles, planks
                pt = pat_grid(uv, l, fw, salt, variant == 1u);
            }
            case 4u: { // checker (3D)
                let q = floor(p3 / l.a.x);
                let k = q.x + q.y + q.z;
                pt.m = k - 2.0 * floor(k * 0.5);
                pt.mean = 0.5;
                pt.feat = l.a.x;
            }
            case 5u: { // gradient along an axis: 1 at `from`, 0 at `to`
                var t = dot(p, l.dir.xyz);
                if (map != 1u) {
                    t += dot(ext * inv_scale, abs(l.dir.xyz));
                }
                pt.m = smoothstep(0.0, 1.0, clamp((t - l.a.y) / (l.a.x - l.a.y), 0.0, 1.0));
                pt.mean = pt.m;
                pt.feat = 1e9;
            }
            case 6u: { // bands or rings (wave, stripes)
                var t: f32;
                if (variant == 1u) {
                    t = length(p - l.dir.xyz * dot(p, l.dir.xyz));
                } else {
                    t = dot(p, l.dir.xyz);
                }
                if (l.a.z > 0.0) {
                    t += l.a.z * l.a.x * (fbm(p3 / (l.a.x * 4.0), 2u, l.a.x * 4.0, fw, salt) - 0.5);
                }
                let dd = abs(fract(t / l.a.x) - 0.5) * l.a.x; // distance to the band centre
                let hw = l.a.y * l.a.x * 0.5;
                let sw = max(l.b.x * l.a.x * 0.25, 0.0);
                if (sw > 0.0) {
                    pt.m = 1.0 - smoothstep(hw - sw, hw + sw, dd);
                } else {
                    pt.m = pulse(dd, 2.0 * hw, fw);
                }
                pt.mean = l.a.y;
                pt.feat = l.a.x * min(l.a.y, 1.0 - l.a.y);
            }
            case 7u: { // wood: rings around the axis plus grain along it
                let ax = l.dir.xyz;
                let r = length(p - ax * dot(p, ax));
                let along = dot(p, ax);
                let warp = l.a.z * (vnoise(vec3<f32>(along / (l.a.x * 12.0), r / (l.a.x * 3.0), 0.0) + 5.1, salt) - 0.5);
                let t = r / l.a.x + warp;
                let dd = abs(fract(t) - 0.5) * l.a.x;
                let hw = l.a.y * l.a.x * 0.5;
                let sw = max(l.b.x * l.a.x * 0.25, 0.0);
                pt.m = 1.0 - smoothstep(hw - sw, hw + sw, dd);
                pt.mean = l.a.y;
                pt.feat = l.a.x * min(l.a.y, 1.0 - l.a.y);
            }
            case 8u: { // magic
                let v = magic(p3 / l.a.x, u32(l.a.y), l.a.z);
                pt.m = smoothstep(l.b.x - l.b.y, l.b.x + l.b.y, v);
                pt.mean = l.a.w;
                pt.feat = l.a.x * 0.3;
            }
            case 9u: { // dots
                let s = l.a.x;
                let row = floor(uv.y / s);
                let off = l.a.z * s * (row - 2.0 * floor(row * 0.5));
                let x = (uv.x - off) / s;
                let col = floor(x);
                let h = hash3i(i32(col), i32(row), 5, salt);
                let ctr = vec2<f32>(0.5) + l.a.w * (vec2<f32>(rnd(h), rnd_lo(h)) - 0.5);
                let cp = vec2<f32>(x - col, uv.y / s - row);
                let dd = length(cp - ctr) * s;
                pt.m = pulse(dd, 2.0 * l.a.y, fw);
                pt.mean = min(3.14159 * l.a.y * l.a.y / (s * s), 1.0);
                pt.feat = 2.0 * l.a.y;
                pt.cell = h | 1u;
            }
            case 10u: { // shingles
                pt = pat_shingles(uv, l, variant, fw, salt);
            }
            default: {
                if (resolved) {
                    pt.m = 0.0;
                    pt.mean = 0.0;
                    pt.feat = 1e9;
                }
            }
        }
        let k = fade(pt.feat, fw);
        var m = mix(pt.mean, pt.m, k);
        if (facing == 1u) {
            m *= smoothstep(0.3, 0.7, wn.y);
        } else if (facing == 2u) {
            m *= smoothstep(0.3, 0.7, -wn.y);
        } else if (facing == 3u) {
            m *= 1.0 - smoothstep(0.3, 0.7, abs(wn.y));
        }
        if (maskby == 1u) {
            m *= prev;
        } else if (maskby == 2u) {
            m *= 1.0 - prev;
        }
        // Cells: colour choice, variation and cavity under the painted gaps.
        if (pt.cell != 0u) {
            if (ncells > 0u) {
                let pick = min(u32(rnd(pcg(pt.cell ^ 0x51u)) * f32(ncells)), ncells - 1u);
                // Fades to the cells' mean colour, as an unresolved layer shows it.
                c = mix(l.b.yzw, srgb8_to_linear(l.cells[pick]), k);
            }
            let vary = l.c.x * (2.0 * rnd(pt.cell) - 1.0) * k;
            c *= (1.0 + vary) * (1.0 + vary);
            if (l.c.y > 0.0) {
                let bev = max(l.c.w, 1e-4);
                let prof = 1.0 - smoothstep(pt.gap * 0.5, pt.gap * 0.5 + bev, pt.d);
                let sh = 1.0 - l.c.y * mix(min(bev / max(pt.feat, 1e-4), 1.0) * 0.5, prof, fade(bev, fw));
                c *= sh * sh;
            }
        }
        var col = l.paint.rgb;
        if (paint == 1u) {
            col = c * l.paint.r;
        }
        let a = m * l.paint.a;
        c = mix(c, col, a);
        glow += l.c.z * a;
        prev = m;
    }
    var o: MatOut;
    o.albedo = c;
    o.glow = glow;
    return o;
}
