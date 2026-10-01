// GPU particles: integrate every live particle (gravity, drag, turbulence, floor bounce).

struct Particle {
    p0: vec4<f32>,   // position, life left (s)
    p1: vec4<f32>,   // velocity, total life (s)
    c0: vec4<f32>,   // colour at birth (linear, premultiplied later)
    c1: vec4<f32>,   // colour at death
    s: vec4<f32>,    // size at birth, size at death, gravity, drag
    t: vec4<f32>,    // floor height, bounce, turbulence, flags (1 additive, 2 stretch)
};

struct Sim {
    dt: f32,
    time: f32,
    count: u32,
    pad: u32,
};

@group(0) @binding(0) var<uniform> sim: Sim;
@group(0) @binding(1) var<storage, read_write> parts: array<Particle>;

@compute @workgroup_size(64)
fn cs_update(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= sim.count) {
        return;
    }
    var q = parts[i];
    if (q.p0.w <= 0.0) {
        return;
    }
    let dt = sim.dt;
    var v = q.p1.xyz;
    var p = q.p0.xyz;
    v.y -= q.s.z * dt;
    v *= max(1.0 - q.s.w * dt, 0.0);
    if (q.t.z > 0.0) {
        let tt = sim.time;
        v += vec3<f32>(
            sin(p.y * 1.7 + tt * 1.3 + p.z * 0.7),
            sin(p.z * 1.9 + tt * 1.1 + p.x * 0.8) * 0.5,
            cos(p.x * 1.5 + tt * 1.7 + p.y * 0.9)
        ) * q.t.z * dt;
    }
    p += v * dt;
    if (p.y < q.t.x) {
        p.y = q.t.x;
        v = vec3<f32>(v.x * 0.7, -v.y * q.t.y, v.z * 0.7);
    }
    q.p0 = vec4<f32>(p, q.p0.w - dt);
    q.p1 = vec4<f32>(v, q.p1.w);
    parts[i] = q;
}
