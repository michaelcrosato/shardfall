//! Live-tunable parameters. Any struct implementing `Tunable` exposes its fields through a
//! visitor, which powers the tuning panel, agent get/set by path, and preset files, with no
//! string lookups in game code (values stay plain struct fields).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

impl ParamValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            ParamValue::Float(f) => Some(*f),
            ParamValue::Int(i) => Some(*i as f64),
            ParamValue::Bool(b) => Some(*b as i64 as f64),
            ParamValue::Text(t) => t.parse().ok(),
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParamValue::Bool(b) => Some(*b),
            ParamValue::Int(i) => Some(*i != 0),
            ParamValue::Float(f) => Some(*f != 0.0),
            ParamValue::Text(t) => match t.as_str() {
                "true" | "on" | "yes" | "1" => Some(true),
                "false" | "off" | "no" | "0" => Some(false),
                _ => None,
            },
        }
    }
    pub fn from_json(v: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(v.clone()).ok()
    }
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum ParamKind {
    Float { min: f32, max: f32 },
    Int { min: i32, max: i32 },
    Bool,
    Choice { options: Vec<String> },
}

#[derive(Clone, Debug, Serialize)]
pub struct ParamInfo {
    pub path: String,
    pub kind: ParamKind,
    pub value: ParamValue,
    pub help: String,
}

/// Visits every tunable field. Implementations decide what to do (draw UI, get, set, ...).
pub trait ParamVisitor {
    fn float(&mut self, name: &str, v: &mut f32, min: f32, max: f32, help: &str);
    fn int(&mut self, name: &str, v: &mut i32, min: i32, max: i32, help: &str);
    fn bool(&mut self, name: &str, v: &mut bool, help: &str);
    fn choice(&mut self, name: &str, v: &mut usize, options: &[&str], help: &str);
    /// Called around nested groups. Return false from `enter` to skip the group.
    fn enter(&mut self, name: &str) -> bool;
    fn exit(&mut self);
}

pub trait Tunable {
    fn visit(&mut self, v: &mut dyn ParamVisitor);
}

/// Visits `t` as a nested group called `name`.
pub fn nested(v: &mut dyn ParamVisitor, name: &str, t: &mut dyn Tunable) {
    if v.enter(name) {
        t.visit(v);
        v.exit();
    }
}

/// Enums usable with `ParamVisitor::choice`.
pub trait ChoiceParam: Sized + Copy {
    const NAMES: &'static [&'static str];
    fn to_index(self) -> usize;
    fn from_index(i: usize) -> Self;
    fn name(self) -> &'static str {
        Self::NAMES[self.to_index()]
    }
    fn visit_choice(&mut self, v: &mut dyn ParamVisitor, name: &str, help: &str) {
        let mut i = self.to_index();
        v.choice(name, &mut i, Self::NAMES, help);
        *self = Self::from_index(i.min(Self::NAMES.len() - 1));
    }
}

/// Declares a C-like enum as a `ChoiceParam` with snake_case display names.
#[macro_export]
macro_rules! choice_enum {
    ($(#[$m:meta])* $vis:vis enum $name:ident { $($(#[$vm:meta])* $var:ident => $label:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        $vis enum $name { $($(#[$vm])* $var),+ }
        impl $crate::params::ChoiceParam for $name {
            const NAMES: &'static [&'static str] = &[$($label),+];
            fn to_index(self) -> usize { self as usize }
            fn from_index(i: usize) -> Self {
                const ALL: &[$name] = &[$($name::$var),+];
                ALL[i.min(ALL.len() - 1)]
            }
        }
    };
}

struct PathTracker {
    stack: Vec<String>,
}

impl PathTracker {
    fn path(&self, name: &str) -> String {
        if self.stack.is_empty() { name.to_string() } else { format!("{}.{}", self.stack.join("."), name) }
    }
}

/// Lists every parameter with its current value.
pub fn list(t: &mut dyn Tunable) -> Vec<ParamInfo> {
    struct L {
        p: PathTracker,
        out: Vec<ParamInfo>,
    }
    impl ParamVisitor for L {
        fn float(&mut self, name: &str, v: &mut f32, min: f32, max: f32, help: &str) {
            self.out.push(ParamInfo {
                path: self.p.path(name),
                kind: ParamKind::Float { min, max },
                value: ParamValue::Float(*v as f64),
                help: help.into(),
            });
        }
        fn int(&mut self, name: &str, v: &mut i32, min: i32, max: i32, help: &str) {
            self.out.push(ParamInfo {
                path: self.p.path(name),
                kind: ParamKind::Int { min, max },
                value: ParamValue::Int(*v as i64),
                help: help.into(),
            });
        }
        fn bool(&mut self, name: &str, v: &mut bool, help: &str) {
            self.out.push(ParamInfo {
                path: self.p.path(name),
                kind: ParamKind::Bool,
                value: ParamValue::Bool(*v),
                help: help.into(),
            });
        }
        fn choice(&mut self, name: &str, v: &mut usize, options: &[&str], help: &str) {
            self.out.push(ParamInfo {
                path: self.p.path(name),
                kind: ParamKind::Choice { options: options.iter().map(|s| s.to_string()).collect() },
                value: ParamValue::Text(options.get(*v).copied().unwrap_or("?").to_string()),
                help: help.into(),
            });
        }
        fn enter(&mut self, name: &str) -> bool {
            self.p.stack.push(name.into());
            true
        }
        fn exit(&mut self) {
            self.p.stack.pop();
        }
    }
    let mut l = L { p: PathTracker { stack: vec![] }, out: vec![] };
    t.visit(&mut l);
    l.out
}

/// Current values as a flat path -> value map (the preset format).
pub fn to_map(t: &mut dyn Tunable) -> BTreeMap<String, ParamValue> {
    list(t).into_iter().map(|p| (p.path, p.value)).collect()
}

/// Sets every matching path from `values`. Returns the paths that were not found or invalid.
pub fn apply_map(t: &mut dyn Tunable, values: &BTreeMap<String, ParamValue>) -> Vec<String> {
    struct S<'a> {
        p: PathTracker,
        values: &'a BTreeMap<String, ParamValue>,
        used: Vec<String>,
    }
    impl S<'_> {
        fn get(&mut self, name: &str) -> Option<&ParamValue> {
            let path = self.p.path(name);
            let v = self.values.get(&path);
            if v.is_some() {
                self.used.push(path);
            }
            v
        }
    }
    impl ParamVisitor for S<'_> {
        fn float(&mut self, name: &str, v: &mut f32, min: f32, max: f32, _: &str) {
            if let Some(x) = self.get(name).and_then(|x| x.as_f64()) {
                *v = (x as f32).clamp(min.min(max), max.max(min));
            }
        }
        fn int(&mut self, name: &str, v: &mut i32, min: i32, max: i32, _: &str) {
            if let Some(x) = self.get(name).and_then(|x| x.as_f64()) {
                *v = (x.round() as i32).clamp(min, max);
            }
        }
        fn bool(&mut self, name: &str, v: &mut bool, _: &str) {
            if let Some(x) = self.get(name).and_then(|x| x.as_bool()) {
                *v = x;
            }
        }
        fn choice(&mut self, name: &str, v: &mut usize, options: &[&str], _: &str) {
            if let Some(x) = self.get(name).cloned() {
                match x {
                    ParamValue::Text(t) => {
                        if let Some(i) = options.iter().position(|o| o.eq_ignore_ascii_case(&t)) {
                            *v = i;
                        }
                    }
                    other => {
                        if let Some(i) = other.as_f64() {
                            *v = (i as usize).min(options.len().saturating_sub(1));
                        }
                    }
                }
            }
        }
        fn enter(&mut self, name: &str) -> bool {
            self.p.stack.push(name.into());
            true
        }
        fn exit(&mut self) {
            self.p.stack.pop();
        }
    }
    let mut s = S { p: PathTracker { stack: vec![] }, values, used: vec![] };
    t.visit(&mut s);
    values.keys().filter(|k| !s.used.contains(k)).cloned().collect()
}

/// Sets one parameter by path. Errors if the path does not exist.
pub fn set(t: &mut dyn Tunable, path: &str, value: ParamValue) -> Result<(), String> {
    let mut m = BTreeMap::new();
    m.insert(path.to_string(), value);
    let missing = apply_map(t, &m);
    if missing.is_empty() { Ok(()) } else { Err(format!("unknown parameter '{path}'")) }
}

pub fn get(t: &mut dyn Tunable, path: &str) -> Option<ParamValue> {
    list(t).into_iter().find(|p| p.path == path).map(|p| p.value)
}

#[cfg(test)]
mod tests {
    use super::*;

    choice_enum! {
        enum Mode { A => "alpha", B => "beta" }
    }

    struct Inner {
        speed: f32,
    }
    impl Tunable for Inner {
        fn visit(&mut self, v: &mut dyn ParamVisitor) {
            v.float("speed", &mut self.speed, 0.0, 10.0, "");
        }
    }
    struct Root {
        on: bool,
        mode: Mode,
        inner: Inner,
    }
    impl Tunable for Root {
        fn visit(&mut self, v: &mut dyn ParamVisitor) {
            v.bool("on", &mut self.on, "");
            self.mode.visit_choice(v, "mode", "");
            nested(v, "inner", &mut self.inner);
        }
    }

    #[test]
    fn get_set_roundtrip() {
        let mut r = Root { on: false, mode: Mode::A, inner: Inner { speed: 1.0 } };
        set(&mut r, "inner.speed", ParamValue::Float(20.0)).unwrap();
        assert_eq!(r.inner.speed, 10.0); // clamped
        set(&mut r, "mode", ParamValue::Text("beta".into())).unwrap();
        assert_eq!(r.mode, Mode::B);
        assert!(set(&mut r, "nope", ParamValue::Bool(true)).is_err());
        let m = to_map(&mut r);
        assert_eq!(m.len(), 3);
        assert_eq!(get(&mut r, "on"), Some(ParamValue::Bool(false)));
    }
}
