// Screen distortion sources (shockwave rings, heat haze, lenses, ripples) drawn as camera-facing
// discs into a screen-space offset buffer; the composite samples the scene through it.

struct Cam {
    view_proj: mat4x4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    eye: vec4<f32>,      // w = time
};

struct D {
    pos_radius: vec4<f32>,
    params: vec4<f32>,   // strength, progress, kind, unused
};

@group(0) @binding(0) var<uniform> cam: Cam;
@group(0) @binding(1) var<storage, read> ds: array<D>;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) l: vec2<f32>,
    @location(1) @interpolate(flat) idx: u32,
    @location(2) ruv: f32,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0)
    );
    let d = ds[ii];
    let c = corners[vi];
    let r = d.pos_radius.w;
    let world = d.pos_radius.xyz + (cam.right.xyz * c.x + cam.up.xyz * c.y) * r;
    var o: Out;
    o.pos = cam.view_proj * vec4<f32>(world, 1.0);
    o.l = c;
    o.idx = ii;
    let p0 = cam.view_proj * vec4<f32>(d.pos_radius.xyz, 1.0);
    let p1 = cam.view_proj * vec4<f32>(d.pos_radius.xyz + cam.right.xyz * r, 1.0);
    o.ruv = length(p1.xy / p1.w - p0.xy / p0.w) * 0.5;
    return o;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    let d = ds[in.idx];
    let r = length(in.l);
    if (r > 1.0) {
        discard;
    }
    let dir = in.l / max(r, 1e-4);
    let dir_uv = vec2<f32>(dir.x, -dir.y);
    let s = d.params.x;
    let kind = u32(d.params.z + 0.5);
    let t = cam.eye.w;
    var off = vec2<f32>(0.0);
    if (kind == 0u) {
        // Shockwave: a ring pushing outward, fading as it grows.
        let p = d.params.y;
        let f = exp(-pow((r - p) / 0.13, 2.0)) * (1.0 - p);
        off = dir_uv * f * s;
    } else if (kind == 1u) {
        // Heat haze: wobbling noise rising upward.
        let q = in.l * 6.0 + vec2<f32>(0.0, -t * 2.5);
        let n = vec2<f32>(sin(q.x * 1.7 + q.y * 2.3 + t * 3.1), cos(q.y * 2.1 - q.x * 1.3 + t * 2.7));
        off = n * s * (1.0 - r) * 0.35;
    } else if (kind == 2u) {
        // Lens: magnify the middle.
        off = -vec2<f32>(in.l.x, -in.l.y) * s * (1.0 - r * r);
    } else {
        // Ripple: rings travelling outward.
        off = dir_uv * sin(r * 26.0 - t * 7.0) * s * (1.0 - r) * 0.4;
    }
    return vec4<f32>(off * in.ruv, 0.0, 0.0);
}
