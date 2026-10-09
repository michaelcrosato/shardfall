//! Moments cut out of long motion captures. A database's takes run for seconds or minutes; a
//! clip is a stretch of one, played from the origin facing forward. A loop is cut at its best
//! cycle (the span whose end matches its start in pose and in speed, among spans that keep
//! moving), played in place facing the way it travels, and closed exactly by spreading what its
//! seam is still off over the cycle. The rest pose is stood on the floor the takes stand on.
//!
//! Ported from `readCmu` in my-3D2dge's tools/asf-amc.mjs. The skeleton formats (ASF/AMC, BVH)
//! only supply each frame's body points.

use std::collections::HashMap;

use anyhow::{Result, bail};

use super::readable::{BALL, Cap, P, PELVIS, PELVIS_F, TOE, V, js_round, side};
use libm::{atan2, cos, sin};

const MM: f64 = 1000.0;

/// A take: its frames, each the 36 body points in the world (metres, y up).
pub trait Take {
    fn frames(&self) -> usize;
    fn points(&self, frame: usize) -> Vec<V>;
}

/// A skeleton's rest pose and the floor directions found from it: forward from the heel toward
/// the toes, right toward the right hip.
pub struct Body {
    /// The 36 body points at rest (metres, y up).
    pub rest: Vec<V>,
    pub fwd: V,
    pub right: V,
}

/// One moment to cut from a take.
#[derive(Clone, Debug)]
pub struct Pick {
    pub name: String,
    pub take: String,
    /// Seconds into the take (the start when absent).
    pub from: Option<f64>,
    /// Seconds into the take (the end when absent).
    pub to: Option<f64>,
    /// A loop's shortest cycle (seconds; 0.5 when absent). A gait whose feet meet between steps
    /// (a limp, a robot) matches itself after one step; a full stride is longer.
    pub min_cycle: Option<f64>,
    /// The take's frame rate.
    pub fps: f64,
    pub looping: bool,
    /// With no stretch given: find one (`Some`), else play the whole take.
    pub find: Option<Find>,
}

/// A stretch found in a take.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Find {
    /// The longest run where the hips travel straight and steadily: a clean loop out of a take
    /// that walks back and forth across a capture area, turning at each end.
    Straight,
    /// The longest straight run (as `Straight`, never doubling back) that travels this way of
    /// where the hips face: a take that walks backward, or sidesteps one way and then the other,
    /// with no turn between.
    Going(Way),
    /// The longest stretch where the hips stay put: an idle.
    Still,
    /// The stretch as given, a loop as it is: a take that holds one cycle already.
    Whole,
}

/// Which way a stretch travels, seen from the hips.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Way {
    Forward,
    Right,
    Back,
    Left,
}

impl Way {
    pub fn parse(s: &str) -> Option<Way> {
        match s.to_ascii_lowercase().as_str() {
            "forward" | "fwd" => Some(Way::Forward),
            "right" => Some(Way::Right),
            "back" | "backward" => Some(Way::Back),
            "left" => Some(Way::Left),
            _ => None,
        }
    }
    /// The angle from the facing, toward the right.
    fn angle(self) -> f64 {
        std::f64::consts::FRAC_PI_2
            * match self {
                Way::Forward => 0.0,
                Way::Right => 1.0,
                Way::Back => 2.0,
                Way::Left => -1.0,
            }
    }
}

/// An angle brought into -pi..pi.
fn wrap(a: f64) -> f64 {
    let t = std::f64::consts::TAU;
    a - t * ((a + std::f64::consts::PI) / t).floor()
}

/// The hips on the floor every tenth of a second: (seconds, x, z, and the way they face: x, z).
fn hips_path(take: &dyn Take, src: f64) -> Vec<(f64, f64, f64, f64, f64)> {
    let step = (src / 10.0).round().max(1.0) as usize;
    (0..take.frames())
        .step_by(step)
        .map(|f| {
            let w = take.points(f);
            let (p, h) = (w[PELVIS], sub(w[PELVIS_F], w[PELVIS]));
            (f as f64 / src, p[0], p[2], h[0], h[2])
        })
        .collect()
}

/// The hips' path within `window` (else all but the first and last 3 s, for a stretch that
/// moves: a performer walks in and out of the capture area at an ordinary pace before and after
/// taking up a style).
fn path_in(take: &dyn Take, src: f64, still: bool, window: Option<(f64, f64)>) -> Vec<(f64, f64, f64, f64, f64)> {
    let mut p = hips_path(take, src);
    if let Some((a, b)) = window {
        p.retain(|q| q.0 >= a && q.0 <= b);
    } else if !still && p.len() > 1 {
        let last = p[p.len() - 1].0;
        p.retain(|q| q.0 >= 3.0 && q.0 <= last - 3.0);
    }
    p
}

/// Every straight run of the path: for each sample, the furthest one it runs straight to
/// (`way`: going that way of where the hips face, never doubling back).
fn runs(p: &[(f64, f64, f64, f64, f64)], way: Option<Way>, body: &Body) -> Vec<(usize, usize)> {
    let n = p.len();
    // Every hip position within 12 cm of the line from the run's start to its end (a sway or a
    // lurch stays inside; a turn does not), covering at least 10 cm a second.
    let off = |i: usize, j: usize, k: usize| -> f64 {
        let (ax, az, bx, bz) = (p[i].1, p[i].2, p[j].1, p[j].2);
        let (dx, dz) = (bx - ax, bz - az);
        let l = (dx * dx + dz * dz).sqrt().max(1e-9);
        ((p[k].1 - ax) * dz - (p[k].2 - az) * dx).abs() / l
    };
    // Going a way: never more than 5 cm back along the line (stepping backward or sideways, a
    // performer turns back without turning round), and heading that way of where the hips face
    // on average (within 45 degrees).
    let onward = |i: usize, j: usize| -> bool {
        let (dx, dz) = (p[j].1 - p[i].1, p[j].2 - p[i].2);
        let l = (dx * dx + dz * dz).sqrt().max(1e-9);
        let mut most = f64::NEG_INFINITY;
        (i..=j).all(|k| {
            let along = ((p[k].1 - p[i].1) * dx + (p[k].2 - p[i].2) * dz) / l;
            most = most.max(along);
            along >= most - 0.05
        })
    };
    let heading_ok = |i: usize, j: usize, way: Way| -> bool {
        let (fw, rt) = (body.fwd, body.right);
        let ang = |x: f64, z: f64| atan2(x * rt[0] + z * rt[2], x * fw[0] + z * fw[2]);
        let (mut hx, mut hz) = (0.0, 0.0);
        for q in &p[i..=j] {
            let l = (q.3 * q.3 + q.4 * q.4).sqrt().max(1e-9);
            (hx, hz) = (hx + q.3 / l, hz + q.4 / l);
        }
        let rel = ang(p[j].1 - p[i].1, p[j].2 - p[i].2) - ang(hx, hz);
        wrap(rel - way.angle()).abs() < std::f64::consts::FRAC_PI_4
    };
    let mut out = Vec::new();
    for i in 0..n {
        let mut j = i;
        while j + 1 < n && (i..=j + 1).all(|k| off(i, j + 1, k) < 0.12) && (way.is_none() || onward(i, j + 1)) {
            j += 1;
        }
        let fast = |j: usize| {
            let (dx, dz) = (p[j].1 - p[i].1, p[j].2 - p[i].2);
            (dx * dx + dz * dz).sqrt() >= 0.1 * (p[j].0 - p[i].0)
        };
        while j > i && !fast(j) {
            j -= 1;
        }
        if j > i && way.is_none_or(|w| heading_ok(i, j, w)) {
            out.push((i, j));
        }
    }
    out
}

/// Finds a stretch (seconds) in a take, within `window` when given. `body` gives the floor's
/// directions (for `Find::Going`: its longest pass).
pub fn find(take: &dyn Take, src: f64, how: Find, window: Option<(f64, f64)>, body: &Body) -> Option<(f64, f64)> {
    let p = path_in(take, src, how == Find::Still, window);
    let n = p.len();
    let mut best: Option<(usize, usize)> = None;
    let longer = |best: Option<(usize, usize)>, i: usize, j: usize| best.is_none_or(|(a, b)| j - i > b - a);
    match how {
        Find::Straight => {
            for (i, j) in runs(&p, None, body) {
                if longer(best, i, j) {
                    best = Some((i, j));
                }
            }
            // Less the steps into and out of the turns at either end.
            let (i, j) = best?;
            let (a, b) = (p[i].0 + 0.3, p[j].0 - 0.3);
            (b - a >= 1.5).then_some((a, b))
        }
        Find::Going(w) => passes(take, src, w, window, body).into_iter().next(),
        Find::Whole => window,
        Find::Still => {
            // The hips within 15 cm of where the stretch began.
            for i in 0..n {
                let mut j = i;
                while j + 1 < n && ((p[j + 1].1 - p[i].1).powi(2) + (p[j + 1].2 - p[i].2).powi(2)).sqrt() < 0.15 {
                    j += 1;
                }
                if longer(best, i, j) {
                    best = Some((i, j));
                }
            }
            let (i, j) = best?;
            let (a, b) = (p[i].0 + 0.2, p[j].0 - 0.2);
            (b - a >= 1.0).then_some((a, b))
        }
    }
}

/// Every pass of a take going `way` (seconds, within `window` when given), longest first: a
/// performer crossing a capture area again and again, a second or more each time once the steps
/// into and out of the turns are left out. A short pass (a run) holds few cycles, so a loop looks
/// for its cycle in all of them.
pub fn passes(take: &dyn Take, src: f64, way: Way, window: Option<(f64, f64)>, body: &Body) -> Vec<(f64, f64)> {
    let p = path_in(take, src, false, window);
    let mut all = runs(&p, Some(way), body);
    all.sort_by(|x, y| (y.1 - y.0).cmp(&(x.1 - x.0)).then(x.0.cmp(&y.0)));
    let mut kept: Vec<(usize, usize)> = Vec::new();
    for (i, j) in all {
        if kept.iter().all(|&(a, b)| j < a || i > b) {
            kept.push((i, j));
        }
    }
    kept.into_iter().map(|(i, j)| (p[i].0 + 0.3, p[j].0 - 0.3)).filter(|(a, b)| b - a >= 1.0).collect()
}

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// JavaScript's `toFixed`: the nearest, a tie going to the larger.
pub fn to_fixed(x: f64, digits: usize) -> String {
    let exact = format!("{:.60}", x.abs());
    let (int, frac) = exact.split_once('.').unwrap_or((&exact, ""));
    let mut d: Vec<u8> = int.bytes().chain(frac.bytes().take(digits)).map(|b| b - b'0').collect();
    let rest = &frac.as_bytes()[digits.min(frac.len())..];
    let up = match rest.first() {
        Some(&c) if c > b'5' => true,
        Some(&b'5') => x >= 0.0 || rest[1..].iter().any(|&c| c != b'0'),
        _ => false,
    };
    if up {
        let mut i = d.len();
        loop {
            if i == 0 {
                d.insert(0, 1);
                break;
            }
            i -= 1;
            if d[i] == 9 {
                d[i] = 0;
            } else {
                d[i] += 1;
                break;
            }
        }
    }
    let n = d.len() - digits;
    let s: String = d.iter().map(|v| (v + b'0') as char).collect();
    let body = if digits > 0 { format!("{}.{}", &s[..n], &s[n..]) } else { s };
    if x < 0.0 && d.iter().any(|&v| v != 0) { format!("-{body}") } else { body }
}

/// The floor's directions, from the rest pose.
struct Ground<'a> {
    body: &'a Body,
}

impl Ground<'_> {
    /// Seen from above, turned into the rig's frame (forward, right), height kept.
    fn local(&self, p: V, o: V) -> V {
        let (f, r) = (self.body.fwd, self.body.right);
        let d = [p[0] - o[0], p[1], p[2] - o[2]];
        [d[0] * f[0] + d[2] * f[2], d[0] * r[0] + d[2] * r[2], d[1]]
    }
    /// Which way the hips face, as an angle on the floor.
    fn heading(&self, w: &[V]) -> f64 {
        let h = sub(w[PELVIS_F], w[PELVIS]);
        atan2(dot([h[0], 0.0, h[2]], self.body.right), dot([h[0], 0.0, h[2]], self.body.fwd))
    }

    /// The best cycle in [from, to] (seconds): frames i < j, `min` (else half a second) to 2.5 s
    /// (or `min` + 1.5 s) apart, whose poses (seen from the hips, turned to face the same way) and speeds match
    /// best, among the spans that keep moving (a pause matches itself perfectly: a span must move
    /// at least half as fast as the stretch's typical frame).
    ///
    /// `steady`: a cycle that turns the body costs as well (20 cm a radian): just after a turn
    /// at the end of a pass, a performer can match their own pose while still coming round.
    fn cycle(&self, take: &dyn Take, src: f64, from: f64, to: f64, min: f64, steady: bool) -> Result<(f64, f64, f64)> {
        let (fw, rt) = (self.body.fwd, self.body.right);
        let a = js_round(from * src) as usize;
        let b = (take.frames() - 1).min(js_round(to * src) as usize);
        let mut fr: Vec<Vec<f64>> = Vec::new();
        let mut heads: Vec<f64> = Vec::new();
        for k in a..=b {
            let w = take.points(k);
            let g = -self.heading(&w);
            heads.push(-g);
            let (c, s) = (cos(g), sin(g));
            let o = w[PELVIS];
            let mut v = vec![0.0; P * 3];
            for (i, p) in w.iter().enumerate() {
                let d = [p[0] - o[0], p[2] - o[2]];
                let f = d[0] * fw[0] + d[1] * fw[2];
                let r = d[0] * rt[0] + d[1] * rt[2];
                v[i * 3] = f * c - r * s;
                v[i * 3 + 1] = f * s + r * c;
                v[i * 3 + 2] = p[1];
            }
            fr.push(v);
        }
        let rms = |x: &[f64], y: &[f64]| -> f64 {
            let mut e = 0.0;
            for k in 0..x.len() {
                let d = x[k] - y[k];
                e += d * d;
            }
            (e / x.len() as f64).sqrt()
        };
        let rms4 = |x: &[f64], y: &[f64], z: &[f64], w: &[f64]| -> f64 {
            let mut e = 0.0;
            for k in 0..x.len() {
                let d = (x[k] - y[k]) - (z[k] - w[k]);
                e += d * d;
            }
            (e / x.len() as f64).sqrt()
        };
        // How far the pose travels, frame to frame (summed, so any span's motion is a subtraction).
        let step: Vec<f64> = (0..fr.len()).map(|k| if k > 0 { rms(&fr[k], &fr[k - 1]) } else { 0.0 }).collect();
        let mut sum = vec![0.0];
        for (k, d) in step.iter().enumerate() {
            sum.push(sum[k] + d);
        }
        let mut sorted: Vec<f64> = step.iter().skip(1).copied().collect();
        sorted.sort_by(|x, y| x.total_cmp(y));
        let typical = sorted.get((step.len().saturating_sub(1)) / 2).copied().unwrap_or(0.0);
        let mut best: Option<(f64, usize, usize)> = None;
        let n = fr.len();
        // Cycles up to 2.5 s (a slow gait's stride), longer when the shortest asked for is long
        // (an idle's breathing and weight shifts).
        let longest = 2.5f64.max(min + 1.5);
        for i in 1..n.saturating_sub(1) {
            let mut j = i + js_round(src * min) as usize;
            while (j as f64) < ((n - 1) as f64).min(i as f64 + longest * src) {
                if (sum[j + 1] - sum[i + 1]) / (j - i) as f64 >= typical / 2.0 {
                    let mut cost = rms(&fr[i], &fr[j])
                        + 0.1 * rms4(&fr[i + 1], &fr[i - 1], &fr[j + 1], &fr[j - 1]) * src / 2.0
                        + 0.002 * (j - i) as f64 / src;
                    if steady {
                        cost += 0.2 * wrap(heads[j] - heads[i]).abs();
                    }
                    if best.is_none_or(|b| cost < b.0) {
                        best = Some((cost, i, j));
                    }
                }
                j += 1;
            }
        }
        let Some((cost, i, j)) = best else {
            bail!("no cycle of {min} s or more between {from} s and {to} s");
        };
        Ok(((a + i) as f64 / src, (a + j) as f64 / src, cost))
    }
}

/// The picked moments of one skeleton's takes as captured clips (body points in mm, the rig's
/// frame), plus `_rest`: the rest pose stood on the floor its takes stand on. `takes` holds each
/// pick's take by id. Notes on the cycles found go to `log`.
pub fn cut(
    body: &Body,
    takes: &HashMap<String, Box<dyn Take + '_>>,
    picks: &[Pick],
    fps: f64,
    log: &mut Vec<String>,
) -> Result<Vec<Cap>> {
    cut_picks(body, takes, picks, fps, false, log)
}

/// As `cut`, but a pick that cannot be cut (no stretch going its way, no cycle) is left out, the
/// reason logged, rather than failing them all: a catalog of a hundred styles.
pub fn cut_lenient(
    body: &Body,
    takes: &HashMap<String, Box<dyn Take + '_>>,
    picks: &[Pick],
    fps: f64,
    log: &mut Vec<String>,
) -> Result<Vec<Cap>> {
    cut_picks(body, takes, picks, fps, true, log)
}

fn cut_picks(
    body: &Body,
    takes: &HashMap<String, Box<dyn Take + '_>>,
    picks: &[Pick],
    fps: f64,
    lenient: bool,
    log: &mut Vec<String>,
) -> Result<Vec<Cap>> {
    let fx = Ground { body };
    let low = |w: &[V]| -> f64 {
        [side(0, TOE), side(1, TOE), side(0, BALL), side(1, BALL)].iter().map(|&i| w[i][1]).fold(f64::INFINITY, f64::min)
    };
    let mut seen: Vec<&str> = Vec::new();
    let mut lows: Vec<f64> = Vec::new();
    let mut clips: Vec<Cap> = Vec::new();
    for pk in picks {
        let mut one = || -> Result<()> {
            let Some(take) = takes.get(&pk.take).map(|t| t.as_ref()) else {
                bail!("{}: take {} is not available", pk.name, pk.take)
            };
            if !seen.contains(&pk.take.as_str()) {
                // A new take: its lowest foot point, frame by frame (where the floor is, below).
                seen.push(&pk.take);
                let stride = (js_round(pk.fps / 30.0) as usize).max(1);
                let mut k = 0;
                while k < take.frames() {
                    lows.push(low(&take.points(k)));
                    k += stride;
                }
            }
            let src = pk.fps;
            let last = (take.frames() - 1) as f64 / src;
            let window = match (pk.from, pk.to) {
                (None, None) => None,
                (a, b) => Some((a.unwrap_or(0.0), b.unwrap_or(last))),
            };
            let found = match pk.find {
                // A loop going a way: the pass that holds the cleanest cycle.
                Some(Find::Going(w)) if pk.looping => {
                    let all = passes(take, src, w, window, body);
                    let min = pk.min_cycle.unwrap_or(0.5);
                    let mut best: Option<(f64, (f64, f64))> = None;
                    for &(a, b) in &all {
                        if let Ok((_, _, cost)) = fx.cycle(take, src, a, b, min, true) {
                            if best.is_none_or(|x| cost < x.0) {
                                best = Some((cost, (a, b)));
                            }
                        }
                    }
                    if !all.is_empty() {
                        log.push(format!(
                            "{}: {} passes of {} going {}",
                            pk.name,
                            all.len(),
                            pk.take,
                            format!("{w:?}").to_lowercase()
                        ));
                    }
                    best.map(|b| b.1)
                }
                Some(how) => find(take, src, how, window, body),
                None => None,
            };
            if let (Some(Find::Going(w)), None) = (pk.find, found) {
                bail!("{} has no straight stretch going {}", pk.take, format!("{w:?}").to_lowercase());
            }
            if let Some((a, b)) = found.filter(|_| pk.find != Some(Find::Whole)) {
                log.push(format!(
                    "{}: {} stretch of {} at {}-{} s",
                    pk.name,
                    if pk.find == Some(Find::Still) { "a still" } else { "a straight" },
                    pk.take,
                    to_fixed(a, 2),
                    to_fixed(b, 2)
                ));
            }
            let mut from = found.map_or(pk.from.unwrap_or(0.0), |f| f.0);
            let mut to = found.map_or(pk.to.unwrap_or(f64::INFINITY), |f| f.1).min(last);
            if to.is_nan() || to <= from {
                bail!(
                    "{}: {} has no frames from {} s to {} s (it lasts {} s)",
                    pk.name,
                    pk.take,
                    from,
                    pk.to.map_or("its end".into(), |t| t.to_string()),
                    to_fixed(last, 2)
                );
            }
            if pk.looping && pk.find != Some(Find::Whole) {
                let min = pk.min_cycle.unwrap_or(0.5);
                let steady = matches!(pk.find, Some(Find::Going(_)));
                let (a, b, seam) = fx.cycle(take, src, from, to, min, steady)?;
                log.push(format!(
                    "{}: a {} s cycle at {}-{} s of {} (its seam is off by {} mm)",
                    pk.name,
                    to_fixed(b - a, 2),
                    to_fixed(a, 2),
                    to_fixed(b, 2),
                    pk.take,
                    js_round(seam * MM)
                ));
                (from, to) = (a, b);
            }
            // A loop's frames are spaced to end exactly on its cycle.
            let n = ((js_round((to - from) * fps) as i64) + 1).max(1) as usize;
            let step = if pk.looping && n > 1 { (to - from) / (n - 1) as f64 } else { 1.0 / fps };
            let world: Vec<Vec<V>> =
                (0..n).map(|f| take.points((take.frames() - 1).min(js_round((from + f as f64 * step) * src) as usize))).collect();
            // Face forward and start at the origin: turned about the vertical so the hips face the
            // rig's forward at the first frame (a loop: the way it travels, when it travels, so it
            // plays in place without drifting; one going a given way, the way the hips face over the
            // cycle, so a backward walk or a sidestep faces forward), slid so the root starts at 0.
            let travel = sub(world[n - 1][PELVIS], world[0][PELVIS]);
            let far = super::readable::hypot(&[travel[0], travel[2]]) > 0.2;
            let (fw, rt) = (body.fwd, body.right);
            let ang = match pk.find {
                Some(Find::Going(_)) if pk.looping => {
                    let mut h = [0.0; 3];
                    for w in &world {
                        let d = sub(w[PELVIS_F], w[PELVIS]);
                        let l = (d[0] * d[0] + d[2] * d[2]).sqrt().max(1e-9);
                        h = [h[0] + d[0] / l, 0.0, h[2] + d[2] / l];
                    }
                    atan2(dot(h, rt), dot(h, fw))
                }
                _ if pk.looping && far => atan2(dot([travel[0], 0.0, travel[2]], rt), dot([travel[0], 0.0, travel[2]], fw)),
                _ => fx.heading(&world[0]),
            };
            let (c, s) = (cos(ang), sin(ang));
            let o = world[0][PELVIS];
            let spin = |p: V| -> V {
                let d = [p[0] - o[0], p[2] - o[2]];
                let f = d[0] * fw[0] + d[1] * fw[2];
                let r = d[0] * rt[0] + d[1] * rt[2];
                let (f2, r2) = (f * c + r * s, -f * s + r * c);
                [fw[0] * f2 + rt[0] * r2, p[1], fw[2] * f2 + rt[2] * r2]
            };
            let mut data = vec![0f32; n * P * 3];
            let mut mv = vec![0f32; n * 2];
            let wn: Vec<V> = world[n - 1].iter().map(|p| spin(*p)).collect();
            let mut moved = false;
            for (f, w) in world.iter().enumerate() {
                // The root: the hips (a loop: the straight line from its first frame's hips to its
                // last's, so it plays in place with the hips' sway kept).
                let wr: Vec<V> = w.iter().map(|p| spin(*p)).collect();
                let u = if n > 1 { f as f64 / (n - 1) as f64 } else { 0.0 };
                let root = if pk.looping { [wn[PELVIS][0] * u, 0.0, wn[PELVIS][2] * u] } else { wr[PELVIS] };
                let rl = fx.local(root, [0.0; 3]);
                mv[f * 2] = (rl[0] * MM) as f32;
                mv[f * 2 + 1] = (rl[1] * MM) as f32;
                if !pk.looping && rl[0].abs() + rl[1].abs() > 0.01 {
                    moved = true;
                }
                for (i, p) in wr.iter().enumerate() {
                    let q = fx.local(*p, root);
                    for k in 0..3 {
                        data[(f * P + i) * 3 + k] = (q[k] * MM) as f32;
                    }
                }
            }
            // A loop closes exactly: what its last frame still differs from its first is spread over
            // the cycle, a little a frame (the last frame is corrected last).
            if pk.looping && n > 2 {
                let l = (n - 1) * P * 3;
                for f in 1..n {
                    for k in 0..P * 3 {
                        let fix = (data[l + k] as f64 - data[k] as f64) * f as f64 / (n - 1) as f64;
                        data[f * P * 3 + k] = (data[f * P * 3 + k] as f64 - fix) as f32;
                    }
                }
            }
            let cap = Cap {
                name: pk.name.clone(),
                n,
                dur: (n - 1) as f64 / fps,
                looping: pk.looping,
                fps,
                data,
                travel: moved.then_some(mv),
                take: Some(format!("{} {}-{}", pk.take, to_fixed(from, 2), to_fixed(to, 2))),
                stride: pk.looping.then(|| super::readable::hypot(&[wn[PELVIS][0], wn[PELVIS][2]]) * MM),
            };
            match clips.iter_mut().find(|c| c.name == pk.name) {
                Some(c) => *c = cap,
                None => clips.push(cap),
            }
            Ok(())
        };
        if let Err(e) = one() {
            if !lenient {
                return Err(e);
            }
            log.push(format!("left out {}: {e:#}", pk.name));
        }
    }
    // The rest pose, stood where the takes stand: its lowest toe at the floor, the height the
    // lowest foot point keeps most often over the takes (a foot spends most of its time flat on
    // the floor; the most common centimetre among the lower half of the frames, so a landing's
    // dip or a climb's rungs do not move it).
    let r = &body.rest;
    let low_r = low(r);
    let mut floor = 0.0;
    if !lows.is_empty() {
        lows.sort_by(|a, b| a.total_cmp(b));
        let half = &lows[..lows.len().div_ceil(2)];
        let mut bins: Vec<(i64, usize)> = Vec::new();
        for v in half {
            let b = js_round(v * 100.0) as i64;
            match bins.iter_mut().find(|x| x.0 == b) {
                Some(x) => x.1 += 1,
                None => bins.push((b, 1)),
            }
        }
        bins.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        floor = bins[0].0 as f64 / 100.0;
    }
    let lift = floor - low_r;
    let mut rd = vec![0f32; P * 3];
    let pel = r[PELVIS];
    for (i, p) in r.iter().enumerate() {
        let q = fx.local([p[0], p[1] + lift, p[2]], [pel[0], 0.0, pel[2]]);
        for k in 0..3 {
            rd[i * 3 + k] = (q[k] * MM) as f32;
        }
    }
    clips.push(Cap {
        name: "_rest".into(),
        n: 1,
        dur: 0.0,
        looping: false,
        fps,
        data: rd,
        travel: None,
        take: None,
        stride: None,
    });
    Ok(clips)
}

#[cfg(test)]
mod tests {
    use super::to_fixed;

    #[test]
    fn to_fixed_rounds_like_javascript() {
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(0.145, 2), "0.14"); // 0.14499999999999999 in binary
        assert_eq!(to_fixed(2.0, 2), "2.00");
        assert_eq!(to_fixed(9.999, 2), "10.00");
        assert_eq!(to_fixed(12.875, 2), "12.88");
        assert_eq!(to_fixed(0.0, 2), "0.00");
    }
}
