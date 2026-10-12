//! The canonical writer: one layout for every saved prop, so diffs stay small and a file
//! always reads back to the same definition and revision.

use super::{Look, PropAsset, PropPart, Shape};

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
