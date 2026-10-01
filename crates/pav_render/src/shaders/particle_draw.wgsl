// GPU particles: camera-facing soft discs (or streaks along their velocity), premultiplied so
// additive and normal particles share one blend state.

struct Particle {
    p0: vec4<f32>,
    p1: vec4<f32>,
    c0: vec4<f32>,
    c1: vec4<f32>,
    s: vec4<f32>,
    t: vec4<f32>,
};

struct Cam {
    view_proj: mat4x4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    eye: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Cam;
@group(0) @binding(1) var<storage, read> parts: array<Particle>;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) add: f32,
};

struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) normal: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0)
    );
    let q = parts[ii];
    var o: Out;
    if (q.p0.w <= 0.0) {
        o.pos = vec4<f32>(0.0, 0.0, -2.0, 1.0);
        return o;
    }
    let age = clamp(1.0 - q.p0.w / max(q.p1.w, 1e-4), 0.0, 1.0);
    let size = mix(q.s.x, q.s.y, age);
    let c = corners[vi];
    let flags = u32(q.t.w + 0.5);
    var ax = cam.right.xyz * size;
    var ay = cam.up.xyz * size;
    if ((flags & 2u) != 0u) {
        let v = q.p1.xyz;
        let vs2 = vec2<f32>(dot(v, cam.right.xyz), dot(v, cam.up.xyz));
        let len = length(vs2);
        if (len > 1e-3) {
            let d = vs2 / len;
            let along = cam.right.xyz * d.x + cam.up.xyz * d.y;
            let across = cam.right.xyz * -d.y + cam.up.xyz * d.x;
            ax = along * (size + len * 0.03);
            ay = across * size * 0.45;
        }
    }
    let world = q.p0.xyz + ax * c.x + ay * c.y;
    o.pos = cam.view_proj * vec4<f32>(world, 1.0);
    o.uv = c;
    o.color = mix(q.c0, q.c1, age);
    o.add = f32(flags & 1u);
    return o;
}

@fragment
fn fs(in: Out) -> FsOut {
    let r2 = dot(in.uv, in.uv);
    if (r2 > 1.0) {
        discard;
    }
    let a = in.color.a * pow(1.0 - r2, 1.5);
    var o: FsOut;
    o.color = vec4<f32>(in.color.rgb * a, a * (1.0 - in.add));
    o.normal = vec4<f32>(0.0);
    return o;
}
