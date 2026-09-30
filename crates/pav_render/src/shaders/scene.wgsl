// Main scene shader: instanced meshes, analytic SDF impostors, shadow passes.

struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    eye: vec4<f32>,          // xyz camera position; w = 1 perspective, 0 orthographic
    forward: vec4<f32>,      // xyz camera forward
    sun_dir: vec4<f32>,      // xyz direction light travels; w = shadows enabled
    sun_color: vec4<f32>,
    sky_color: vec4<f32>,
    ground_color: vec4<f32>,
    cut: vec4<f32>,          // xyz focus; w = cut height
    cut2: vec4<f32>,         // x cut radius, y height cut on, z fade radius, w fade on
    params: vec4<f32>,       // x time, y point light count, z shadow texel (world), w unused
    style: vec4<f32>,        // x flat shadow, y cel bands, z rim, w specular
    viewport: vec4<f32>,     // w, h, 1/w, 1/h
};

struct PointLight {
    pos_radius: vec4<f32>,
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var<storage, read> point_lights: array<PointLight>;
@group(0) @binding(2) var shadow_map: texture_depth_2d;
@group(0) @binding(3) var shadow_sampler: sampler_comparison;

const FLAG_NO_CUT: u32 = 1u;
const FLAG_NO_RECEIVE_SHADOW: u32 = 4u;

struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) normal: vec4<f32>,
};

fn bayer4(frag: vec2<f32>) -> f32 {
    var m = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    let x = u32(frag.x) % 4u;
    let y = u32(frag.y) % 4u;
    return (m[y * 4u + x] + 0.5) / 16.0;
}

// True when the fragment should be removed by the camera helpers.
fn cut_away(p: vec3<f32>, frag: vec2<f32>, flags: u32) -> bool {
    if ((flags & FLAG_NO_CUT) != 0u) {
        return false;
    }
    if (g.cut2.y > 0.5) {
        let dxz = length(p.xz - g.cut.xz);
        if (p.y > g.cut.w && dxz < g.cut2.x) {
            // soft dithered rim on the outer 25%
            let k = smoothstep(g.cut2.x * 0.75, g.cut2.x, dxz);
            if (bayer4(frag) >= k) {
                return true;
            }
        }
    }
    if (g.cut2.w > 0.5) {
        let f = g.cut.xyz;
        var ro: vec3<f32>;
        var rd: vec3<f32>;
        if (g.eye.w > 0.5) {
            ro = g.eye.xyz;
            rd = normalize(f - ro);
        } else {
            rd = g.forward.xyz;
            ro = f - rd * 1000.0;
        }
        let t_f = dot(f - ro, rd);
        let t_p = dot(p - ro, rd);
        if (t_p < t_f - 0.75) {
            let d = length((p - ro) - rd * t_p);
            let r = g.cut2.z;
            if (d < r) {
                let alpha = smoothstep(0.8, 1.0, d / r);
                if (bayer4(frag) > alpha) {
                    return true;
                }
            }
        }
    }
    return false;
}

fn shadow_factor(p: vec3<f32>, n: vec3<f32>) -> f32 {
    if (g.sun_dir.w < 0.5) {
        return 1.0;
    }
    let texel = g.params.z;
    let lp = g.light_view_proj * vec4<f32>(p + n * texel * 1.5, 1.0);
    let ndc = lp.xyz / lp.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, -ndc.y * 0.5 + 0.5);
    if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0 || ndc.z > 1.0 || ndc.z < 0.0) {
        return 1.0;
    }
    let ts = 1.0 / f32(textureDimensions(shadow_map).x);
    let d = ndc.z - 0.0008;
    var s = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            s += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(f32(x), f32(y)) * ts, d);
        }
    }
    return s / 9.0;
}

fn view_dir(p: vec3<f32>) -> vec3<f32> {
    if (g.eye.w > 0.5) {
        return normalize(g.eye.xyz - p);
    }
    return -g.forward.xyz;
}

fn quantize(x: f32, bands: f32) -> f32 {
    // Hard-edged bands with a hair of smoothing to avoid shimmering.
    let s = x * bands;
    let f = floor(s);
    return (f + smoothstep(0.45, 0.55, s - f)) / bands;
}

fn shade(p: vec3<f32>, n: vec3<f32>, albedo: vec3<f32>, emissive: f32, style: u32, flags: u32) -> vec3<f32> {
    if (style == 3u) {
        return albedo * max(1.0, 1.0 + emissive);
    }
    var sh = 1.0;
    if ((flags & FLAG_NO_RECEIVE_SHADOW) == 0u) {
        sh = shadow_factor(p, n);
    }
    let l = -g.sun_dir.xyz;
    let ndl = dot(n, l);
    let v = view_dir(p);
    let hemi = mix(g.ground_color.rgb, g.sky_color.rgb, n.y * 0.5 + 0.5);
    var col: vec3<f32>;
    if (style == 0u) {
        col = albedo * mix(g.style.x, 1.0, sh);
    } else if (style == 1u) {
        let lit = quantize(clamp(ndl, 0.0, 1.0) * sh, max(g.style.y - 1.0, 1.0));
        let direct = step(0.02, ndl) * sh;
        let amount = select(lit, direct, g.style.y <= 2.0);
        let rim = step(0.72, 1.0 - max(dot(n, v), 0.0)) * g.style.z * (0.4 + 0.6 * amount);
        col = albedo * (hemi + g.sun_color.rgb * amount) + albedo * rim;
    } else {
        let dl = max(ndl, 0.0) * sh;
        let h = normalize(l + v);
        let spec = pow(max(dot(n, h), 0.0), 40.0) * g.style.w * sh;
        col = albedo * (hemi + g.sun_color.rgb * dl) + g.sun_color.rgb * spec;
    }
    let count = u32(g.params.y);
    for (var i = 0u; i < count; i++) {
        let pl = point_lights[i];
        let d = pl.pos_radius.xyz - p;
        let dist = length(d);
        let r = pl.pos_radius.w;
        if (dist < r) {
            let ld = d / max(dist, 1e-4);
            let att = pow(clamp(1.0 - dist / r, 0.0, 1.0), 2.0);
            var k = max(dot(n, ld), 0.0) * att;
            if (style == 0u) {
                k = att;
            } else if (style == 1u) {
                k = quantize(k, 3.0);
            }
            col += albedo * pl.color.rgb * k;
        }
    }
    return col + albedo * emissive;
}

fn group_out(n: vec3<f32>, group: u32) -> vec4<f32> {
    return vec4<f32>(n, f32(group & 2047u));
}

// ---------------------------------------------------------------- meshes

struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct InstIn {
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) color: vec4<f32>,
    @location(8) params: vec4<u32>,   // style, flags, group, unused
};

struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) @interpolate(flat) params: vec4<u32>,
};

@vertex
fn vs_mesh(v: MeshIn, i: InstIn) -> MeshOut {
    let model = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    let wp = model * vec4<f32>(v.pos, 1.0);
    let c0 = i.m0.xyz;
    let c1 = i.m1.xyz;
    let c2 = i.m2.xyz;
    let cof = mat3x3<f32>(cross(c1, c2), cross(c2, c0), cross(c0, c1));
    var n = cof * v.normal;
    if (dot(cross(c0, c1), c2) < 0.0) {
        n = -n;
    }
    var o: MeshOut;
    o.clip = g.view_proj * wp;
    o.world = wp.xyz;
    o.normal = normalize(n);
    o.color = i.color;
    o.params = i.params;
    return o;
}

@fragment
fn fs_mesh(in: MeshOut) -> FsOut {
    if (cut_away(in.world, in.clip.xy, in.params.y)) {
        discard;
    }
    let n = normalize(in.normal);
    var o: FsOut;
    o.color = vec4<f32>(shade(in.world, n, in.color.rgb, in.color.a, in.params.x, in.params.y), 1.0);
    o.normal = group_out(n, in.params.z);
    return o;
}

@vertex
fn vs_mesh_shadow(v: MeshIn, i: InstIn) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    return g.view_proj * model * vec4<f32>(v.pos, 1.0);
}

// ---------------------------------------------------------------- SDF impostors

struct SdfIn {
    @location(0) a: vec4<f32>,      // xyz, radius a
    @location(1) b: vec4<f32>,      // xyz, radius b
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<u32>,
};

struct SdfOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(perspective, sample) world: vec3<f32>,
    @location(1) @interpolate(flat) a: vec4<f32>,
    @location(2) @interpolate(flat) b: vec4<f32>,
    @location(3) @interpolate(flat) color: vec4<f32>,
    @location(4) @interpolate(flat) params: vec4<u32>,
};

struct SdfFsOut {
    @location(0) color: vec4<f32>,
    @location(1) normal: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

fn cube_corner(vi: u32) -> vec3<f32> {
    var idx = array<u32, 36>(
        5u, 1u, 3u, 5u, 3u, 7u,
        0u, 4u, 6u, 0u, 6u, 2u,
        6u, 7u, 3u, 6u, 3u, 2u,
        0u, 1u, 5u, 0u, 5u, 4u,
        4u, 5u, 7u, 4u, 7u, 6u,
        1u, 0u, 2u, 1u, 2u, 3u);
    let c = idx[vi];
    return vec3<f32>(select(-1.0, 1.0, (c & 1u) != 0u), select(-1.0, 1.0, (c & 2u) != 0u), select(-1.0, 1.0, (c & 4u) != 0u));
}

@vertex
fn vs_sdf(@builtin(vertex_index) vi: u32, s: SdfIn) -> SdfOut {
    let c = cube_corner(vi);
    let a = s.a.xyz;
    let b = s.b.xyz;
    let r = max(s.a.w, s.b.w) * 1.02 + 0.001;
    let axis = b - a;
    let len = length(axis);
    var dir = vec3<f32>(0.0, 1.0, 0.0);
    if (len > 1e-5) {
        dir = axis / len;
    }
    let up = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(dir.y) > 0.9);
    let t1 = normalize(cross(dir, up));
    let t2 = cross(t1, dir);
    let mid = (a + b) * 0.5;
    let w = mid + dir * (c.y * (len * 0.5 + r)) + t1 * (c.x * r) + t2 * (c.z * r);
    var o: SdfOut;
    o.clip = g.view_proj * vec4<f32>(w, 1.0);
    o.world = w;
    o.a = s.a;
    o.b = s.b;
    o.color = s.color;
    o.params = s.params;
    return o;
}

// Ray / rounded-cone intersection (Inigo Quilez, MIT). Returns (t, normal) or t < 0 on miss.
fn round_cone(ro: vec3<f32>, rd: vec3<f32>, pa: vec3<f32>, pb: vec3<f32>, ra: f32, rb: f32) -> vec4<f32> {
    let ba = pb - pa;
    let oa = ro - pa;
    let ob = ro - pb;
    let rr = ra - rb;
    let m0 = dot(ba, ba);
    let m1 = dot(ba, oa);
    let m2 = dot(ba, rd);
    let m3 = dot(rd, oa);
    let m5 = dot(oa, oa);
    let m6 = dot(ob, rd);
    let m7 = dot(ob, ob);
    let d2 = m0 - rr * rr;
    if (d2 > 1e-8) {
        let k2 = d2 - m2 * m2;
        let k1 = d2 * m3 - m1 * m2 + m2 * rr * ra;
        let k0 = d2 * m5 - m1 * m1 + m1 * rr * ra * 2.0 - m0 * ra * ra;
        let h = k1 * k1 - k0 * k2;
        if (h < 0.0) {
            return vec4<f32>(-1.0);
        }
        if (abs(k2) > 1e-8) {
            let t = (-sqrt(h) - k1) / k2;
            let y = m1 - ra * rr + t * m2;
            if (y > 0.0 && y < d2) {
                return vec4<f32>(t, normalize(d2 * (oa + t * rd) - ba * y));
            }
        }
    }
    let h1 = m3 * m3 - m5 + ra * ra;
    let h2 = m6 * m6 - m7 + rb * rb;
    if (max(h1, h2) < 0.0) {
        return vec4<f32>(-1.0);
    }
    var r = vec4<f32>(1e20);
    if (h1 > 0.0) {
        let t = -m3 - sqrt(h1);
        r = vec4<f32>(t, (oa + t * rd) / ra);
    }
    if (h2 > 0.0) {
        let t = -m6 - sqrt(h2);
        if (t < r.x) {
            r = vec4<f32>(t, (ob + t * rd) / rb);
        }
    }
    if (r.x > 1e19) {
        return vec4<f32>(-1.0);
    }
    return r;
}

fn sdf_ray(world: vec3<f32>, a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    var ro: vec3<f32>;
    var rd: vec3<f32>;
    if (g.eye.w > 0.5) {
        ro = g.eye.xyz;
        rd = normalize(world - ro);
    } else {
        rd = g.forward.xyz;
        let span = length(b.xyz - a.xyz) + 4.0 * max(a.w, b.w) + 1.0;
        ro = world - rd * span * 2.0;
    }
    let hit = round_cone(ro, rd, a.xyz, b.xyz, a.w, b.w);
    if (hit.x < 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, -1.0);
    }
    return vec4<f32>(ro + rd * hit.x, 1.0);
}

@fragment
fn fs_sdf(in: SdfOut) -> SdfFsOut {
    var ro: vec3<f32>;
    var rd: vec3<f32>;
    if (g.eye.w > 0.5) {
        ro = g.eye.xyz;
        rd = normalize(in.world - ro);
    } else {
        rd = g.forward.xyz;
        let span = length(in.b.xyz - in.a.xyz) + 4.0 * max(in.a.w, in.b.w) + 1.0;
        ro = in.world - rd * span * 2.0;
    }
    let hit = round_cone(ro, rd, in.a.xyz, in.b.xyz, in.a.w, in.b.w);
    if (hit.x < 0.0) {
        discard;
    }
    let p = ro + rd * hit.x;
    if (cut_away(p, in.clip.xy, in.params.y)) {
        discard;
    }
    let n = normalize(hit.yzw);
    let clip = g.view_proj * vec4<f32>(p, 1.0);
    var o: SdfFsOut;
    o.color = vec4<f32>(shade(p, n, in.color.rgb, in.color.a, in.params.x, in.params.y), 1.0);
    o.normal = group_out(n, in.params.z);
    o.depth = clip.z / clip.w;
    return o;
}

@fragment
fn fs_sdf_shadow(in: SdfOut) -> @builtin(frag_depth) f32 {
    let h = sdf_ray(in.world, in.a, in.b);
    if (h.w < 0.0) {
        discard;
    }
    let clip = g.view_proj * vec4<f32>(h.xyz, 1.0);
    return clip.z / clip.w;
}
