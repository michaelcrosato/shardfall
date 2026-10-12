//! The canonical writer: one layout for every saved prop, so diffs stay small and a file
//! always reads back to the same definition and revision.

use std::collections::BTreeMap;

use glam::Vec3;
use serde_json::Value;

use super::doc::{Anchor, Attach, Doc, Kind, MaterialEntry, Node, Prim, Replicate, Socket, Surface};
use super::{Look, PropAsset, PropPart, Shape};
use crate::params::ChoiceParam;
use crate::shape::Sway;

pub(crate) fn quoted(text: &str) -> String {
    serde_json::to_string(text).expect("strings serialize")
}

/// The shortest decimal that reads back as the same f32: integers without `.0`, no exponents.
/// Negative zero keeps its sign so that a written file keeps its revision.
pub(crate) fn number(x: f32) -> String {
    if x == 0.0 && x.is_sign_negative() { "-0.0".into() } else { x.to_string() }
}

pub(crate) fn numbers(values: &[f32]) -> String {
    format!("[{}]", values.iter().map(|x| number(*x)).collect::<Vec<_>>().join(","))
}

impl PropAsset {
    /// The canonical file text: a header line, one line per part, defaults omitted and the
    /// shortest numbers that read back exactly. Reading it gives the same revision.
    pub fn text(&self) -> String {
        let mut out = format!("{{\"format\":{},\"name\":{}", self.format, quoted(&self.name));
        if !self.description.is_empty() {
            out += &format!(",\n\"description\":{}", quoted(&self.description));
        }
        out += ",\n\"parts\":{";
        for (index, (name, part)) in self.parts.iter().enumerate() {
            out += if index == 0 { "\n" } else { ",\n" };
            out += &quoted(name);
            out.push(':');
            part.write(&mut out);
        }
        out + "}}\n"
    }
}

impl PropPart {
    /// Canonical part order: shape, placement, rotation, surface, collision. A field is left
    /// out only when it is bit-identical to its default.
    fn write(&self, out: &mut String) {
        let shape = match self.shape {
            Shape::Box { half } => format!("{{\"type\":\"box\",\"half\":{}}}", numbers(&half.to_array())),
            Shape::RoundedBox { half, radius } => {
                format!("{{\"type\":\"rounded_box\",\"half\":{},\"radius\":{}}}", numbers(&half.to_array()), number(radius))
            }
            Shape::Sphere { radius } => format!("{{\"type\":\"sphere\",\"radius\":{}}}", number(radius)),
            Shape::Capsule { half_height, radius } => {
                format!("{{\"type\":\"capsule\",\"radius\":{},\"half_height\":{}}}", number(radius), number(half_height))
            }
            Shape::Cylinder { half_height, radius } => {
                format!("{{\"type\":\"cylinder\",\"radius\":{},\"half_height\":{}}}", number(radius), number(half_height))
            }
        };
        out.push_str(&format!("{{\"shape\":{shape}"));
        if self.pos.to_array().iter().any(|x| x.to_bits() != 0) {
            out.push_str(&format!(",\"pos\":{}", numbers(&self.pos.to_array())));
        }
        for (key, angle) in [("yaw", self.yaw), ("pitch", self.pitch), ("roll", self.roll)] {
            if angle.to_bits() != 0 {
                out.push_str(&format!(",\"{key}\":{}", number(angle)));
            }
        }
        out.push_str(&format!(",\"color\":{}", quoted(&self.color)));
        if self.look != Look::Cel {
            out.push_str(&format!(",\"look\":{}", serde_json::to_string(&self.look).expect("looks serialize")));
        }
        if self.emissive.to_bits() != 0 {
            out.push_str(&format!(",\"emissive\":{}", number(self.emissive)));
        }
        if !self.solid {
            out.push_str(",\"solid\":false");
        }
        out.push('}');
    }
}

// --- Format 2 ------------------------------------------------------------------------------

/// A number as format 2 writes it: the shortest decimal that reads back as the same f32,
/// integers without `.0`, no exponents; zero is `0` whatever its sign.
fn num(x: f32) -> String {
    if x == 0.0 { "0".into() } else { x.to_string() }
}

fn nums(values: &[f32]) -> String {
    format!("[{}]", values.iter().map(|x| num(*x)).collect::<Vec<_>>().join(","))
}

fn vec3(v: Vec3) -> String {
    nums(&v.to_array())
}

fn pairs(points: &[[f32; 2]]) -> String {
    format!("[{}]", points.iter().map(|p| nums(p)).collect::<Vec<_>>().join(","))
}

fn strings(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|s| quoted(s)).collect::<Vec<_>>().join(","))
}

fn look_name(look: Look) -> &'static str {
    <Look as ChoiceParam>::NAMES[look.to_index()]
}

/// Checked JSON (material layer parameters) with format 2's numbers.
fn value(value: &Value) -> String {
    match value {
        // Whole numbers (a seed can pass 2^24) stay exact; others are f32 like every length.
        Value::Number(n) if n.is_u64() || n.is_i64() => n.to_string(),
        Value::Number(n) => num(n.as_f64().unwrap_or_default() as f32),
        Value::Array(items) => format!("[{}]", items.iter().map(self::value).collect::<Vec<_>>().join(",")),
        Value::Object(map) => {
            format!("{{{}}}", map.iter().map(|(k, v)| format!("{}:{}", quoted(k), self::value(v))).collect::<Vec<_>>().join(","))
        }
        other => other.to_string(),
    }
}

/// The fields of one object, written in the order they are added.
#[derive(Default)]
struct Fields(Vec<String>);

impl Fields {
    fn add(&mut self, key: &str, text: String) {
        self.0.push(format!("{}:{text}", quoted(key)));
    }

    fn opt<T>(&mut self, key: &str, value: Option<T>, text: impl FnOnce(T) -> String) {
        if let Some(value) = value {
            self.add(key, text(value));
        }
    }

    fn object(self) -> String {
        format!("{{{}}}", self.0.join(","))
    }
}

/// Surface fields in canonical order; `solid` is written apart from them.
fn surface(f: &mut Fields, s: &Surface) {
    f.opt("mat", s.mat.as_ref(), |m| quoted(m));
    f.opt("color", s.color.as_ref(), |c| quoted(c));
    f.opt("look", s.look, |l| quoted(look_name(l)));
    f.opt("emissive", s.emissive, num);
}

fn attach(a: &Attach) -> String {
    if a.at.is_none() && a.own.is_none() {
        return quoted(&a.to);
    }
    let anchor = |anchor: &Anchor| match anchor {
        Anchor::Word(word) => quoted(word),
        Anchor::Point(point) => nums(point),
    };
    let mut f = Fields::default();
    f.add("to", quoted(&a.to));
    f.opt("at", a.at.as_ref(), anchor);
    f.opt("self", a.own.as_ref(), anchor);
    f.object()
}

fn cycle(f: &mut Fields, cycle: Option<&Vec<String>>) {
    f.opt("cycle", cycle, |c| strings(c));
}

fn replicate(f: &mut Fields, r: &Replicate) {
    let mut o = Fields::default();
    let key = match r {
        Replicate::Array(a) => {
            o.add("count", a.count.to_string());
            o.opt("step", a.step, vec3);
            o.opt("span", a.span, vec3);
            cycle(&mut o, a.cycle.as_ref());
            "array"
        }
        Replicate::Grid(g) => {
            o.add("count", format!("[{},{}]", g.count[0], g.count[1]));
            o.add("step", nums(&g.step));
            o.opt("plane", g.plane, |[a, b]| quoted(&format!("{}{}", a.name(), b.name())));
            cycle(&mut o, g.cycle.as_ref());
            "grid"
        }
        Replicate::Radial(r) => {
            o.add("count", r.count.to_string());
            o.add("radius", num(r.radius));
            o.opt("start", r.start, num);
            o.opt("arc", r.arc, num);
            o.opt("axis", r.axis, |d| quoted(d.name()));
            cycle(&mut o, r.cycle.as_ref());
            "radial"
        }
    };
    f.add(key, o.object());
}

fn prim(f: &mut Fields, p: &Prim) {
    f.add("type", quoted(p.kind.name()));
    f.opt("size", p.size.as_ref(), |s| nums(s));
    f.opt("half", p.half, vec3);
    f.opt("radius", p.radius, num);
    f.opt("radius_end", p.radius_end, num);
    f.opt("minor_radius", p.minor_radius, num);
    f.opt("thickness", p.thickness, num);
    f.opt("height", p.height, num);
    f.opt("half_height", p.half_height, num);
    f.opt("arc", p.arc, num);
    f.opt("facing", p.facing, |d| quoted(d.name()));
    f.opt("peak", p.peak, num);
    f.opt("profile", p.profile.as_ref(), |p| pairs(p));
    f.opt("points", p.points.as_ref(), |p| pairs(p));
    f.opt("depth", p.depth, num);
    f.opt("segments", p.segments, |n| n.to_string());
}

fn map(m: &BTreeMap<String, String>) -> String {
    let mut f = Fields::default();
    for (key, value) in m {
        f.add(key, quoted(value));
    }
    f.object()
}

fn material(entry: &MaterialEntry) -> String {
    let m = match entry {
        MaterialEntry::Library(name) => return quoted(name),
        MaterialEntry::Inline(m) => m,
    };
    let mut f = Fields::default();
    f.opt("base", m.base.as_ref(), |b| quoted(b));
    f.opt("color", m.color.as_ref(), |c| quoted(c));
    f.opt("look", m.look, |l| quoted(look_name(l)));
    f.opt("emissive", m.emissive, num);
    f.opt("roughness", m.roughness, num);
    f.opt("metallic", m.metallic, num);
    f.opt("map", m.map, |map| quoted(map.name()));
    f.opt("scale", m.scale, num);
    f.opt("turn", m.turn, |t| t.to_string());
    f.opt("layers", m.layers.as_ref(), |layers| {
        let layers: Vec<String> = layers
            .iter()
            .map(|layer| {
                let mut l = Fields::default();
                l.add("type", quoted(layer.kind.name()));
                for (key, v) in &layer.params {
                    l.add(key, value(v));
                }
                l.object()
            })
            .collect();
        format!("[{}]", layers.join(","))
    });
    f.object()
}

fn socket(s: &Socket) -> String {
    let mut f = Fields::default();
    f.opt("pos", s.pos, vec3);
    f.opt("attach", s.attach.as_ref(), attach);
    f.opt("offset", s.offset, vec3);
    f.opt("axis", s.axis, |d| quoted(d.name()));
    f.opt("yaw", s.yaw, num);
    f.opt("pitch", s.pitch, num);
    f.opt("roll", s.roll, num);
    f.object()
}

/// Line breaks and indentation between entries; none in the compact text.
struct Layout {
    compact: bool,
}

impl Layout {
    fn line(&self, depth: usize) -> String {
        if self.compact { String::new() } else { format!("\n{}", "  ".repeat(depth)) }
    }

    /// `{` then each entry on its own line at `depth`, then `}`.
    fn entries<'a, T: 'a>(
        &self,
        depth: usize,
        items: impl IntoIterator<Item = (&'a String, &'a T)>,
        mut text: impl FnMut(&T) -> String,
    ) -> String {
        let lines: Vec<String> =
            items.into_iter().map(|(key, item)| format!("{}{}:{}", self.line(depth), quoted(key), text(item))).collect();
        format!("{{{}}}", lines.join(","))
    }

    fn node(&self, n: &Node, depth: usize) -> String {
        let mut f = Fields::default();
        match &n.kind {
            Kind::Prim(p) => prim(&mut f, p),
            Kind::Ref(name) => f.add("ref", quoted(name)),
            Kind::Mesh(name) => f.add("mesh", quoted(name)),
            Kind::Group(_) => {}
        }
        f.opt("from", n.from, vec3);
        f.opt("to", n.to, vec3);
        f.opt("pos", n.pos, vec3);
        f.opt("attach", n.attach.as_ref(), attach);
        f.opt("offset", n.offset, vec3);
        f.opt("axis", n.axis, |d| quoted(d.name()));
        f.opt("yaw", n.yaw, num);
        f.opt("pitch", n.pitch, num);
        f.opt("roll", n.roll, num);
        f.opt("spin", n.spin, num);
        surface(&mut f, &n.surface);
        f.opt("solid", n.surface.solid, |s| s.to_string());
        f.opt("hidden", n.hidden, |h| h.to_string());
        f.opt("light", n.light.as_ref(), |l| {
            let mut o = Fields::default();
            o.opt("color", l.color.as_ref(), |c| quoted(c));
            o.opt("radius", l.radius, num);
            o.opt("intensity", l.intensity, num);
            o.opt("flicker", l.flicker, num);
            o.opt("pulse", l.pulse, num);
            o.opt("shadows", l.shadows, |s| s.to_string());
            o.object()
        });
        f.opt("fx", n.fx.as_ref(), |fx| {
            let mut o = Fields::default();
            o.add("preset", quoted(&fx.preset));
            o.opt("rate", fx.rate, num);
            o.opt("color", fx.color.as_ref(), |c| quoted(c));
            o.opt("size", fx.size, num);
            o.opt("speed", fx.speed, num);
            o.opt("life", fx.life, num);
            o.opt("area", fx.area, vec3);
            o.object()
        });
        f.opt("sway", n.sway, |s| quoted(<Sway as ChoiceParam>::NAMES[s.to_index()]));
        match n.mirror.as_slice() {
            [] => {}
            [axis] => f.add("mirror", quoted(axis.name())),
            axes => f.add("mirror", format!("[{}]", axes.iter().map(|a| quoted(a.name())).collect::<Vec<_>>().join(","))),
        }
        if let Some(r) = &n.replicate {
            replicate(&mut f, r);
        }
        f.opt("chance", n.chance, num);
        f.opt("jitter", n.jitter.as_ref(), |j| {
            let mut o = Fields::default();
            o.opt("pos", j.pos, vec3);
            o.opt("rot", j.rot, vec3);
            o.opt("scale", j.scale, |s| nums(&s));
            o.opt("color", j.color, num);
            o.object()
        });
        f.opt("pick", n.pick, |p| p.to_string());
        f.opt("scale", n.scale, num);
        f.opt("remap", n.remap.as_ref(), map);
        f.opt("recolor", n.recolor.as_ref(), map);
        if let Kind::Group(children) = &n.kind {
            f.add("parts", self.entries(depth + 1, children, |child| self.node(child, depth + 1)));
        }
        f.object()
    }
}

impl Doc {
    /// Format-2 text. The canonical layout is a header line (format, name, unit, the root's
    /// surface defaults), then each section starting a line, each map entry on its own line,
    /// group children indented two spaces per depth. The compact text, which `prop2-`
    /// revisions hash, is the same without line breaks.
    pub(crate) fn write(&self, compact: bool) -> String {
        let layout = Layout { compact };
        let mut head = Fields::default();
        head.add("format", "2".into());
        head.add("name", quoted(&self.name));
        head.add("unit", quoted(self.unit.name()));
        surface(&mut head, &self.surface);
        head.opt("solid", self.surface.solid, |s| s.to_string());
        let mut sections = vec![head.0.join(",")];
        let mut section = |key: &str, text: String| sections.push(format!("{}{}:{text}", layout.line(0), quoted(key)));
        if !self.description.is_empty() {
            section("description", quoted(&self.description));
        }
        if !self.tags.is_empty() {
            section("tags", strings(&self.tags));
        }
        if let Some(category) = self.category {
            section("category", quoted(category.name()));
        }
        if let Some(s) = &self.source {
            let mut f = Fields::default();
            for (key, value) in [
                ("library", &s.library),
                ("url", &s.url),
                ("license", &s.license),
                ("author", &s.author),
                ("sha256", &s.sha256),
                ("title", &s.title),
            ] {
                f.opt(key, value.as_ref(), |v| quoted(v));
            }
            section("source", f.object());
        }
        if let Some(fit) = &self.fit {
            let mut f = Fields::default();
            for (key, value) in [
                ("mean_mm", fit.mean_mm),
                ("worst_mm", fit.worst_mm),
                ("iou", fit.iou),
                ("de", fit.de),
                ("mesh_pct", fit.mesh_pct),
            ] {
                f.opt(key, value, num);
            }
            f.opt("parts_hash", fit.parts_hash.as_ref(), |h| quoted(h));
            section("fit", f.object());
        }
        if !self.materials.is_empty() {
            section("materials", layout.entries(0, &self.materials, material));
        }
        section("parts", layout.entries(0, &self.parts, |node| layout.node(node, 0)));
        if !self.sockets.is_empty() {
            section("sockets", layout.entries(0, &self.sockets, socket));
        }
        let mut text = format!("{{{}}}", sections.join(","));
        if !compact {
            text.push('\n');
        }
        text
    }
}
