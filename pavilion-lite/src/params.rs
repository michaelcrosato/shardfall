//! Live-tunable parameters. A struct implements `Tunable` by visiting its fields; that one
//! method powers the `params` / `set` tools (paths like `movement.speed`), with no string
//! lookups in game code: values stay plain struct fields.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Bool(bool),
    Float(f64),
    Text(String),
}

impl ParamValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            ParamValue::Float(f) => Some(*f),
            ParamValue::Bool(b) => Some(*b as i32 as f64),
            ParamValue::Text(t) => t.parse().ok(),
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParamValue::Bool(b) => Some(*b),
            ParamValue::Float(f) => Some(*f != 0.0),
            ParamValue::Text(t) => match t.as_str() {
                "true" | "on" | "yes" | "1" => Some(true),
                "false" | "off" | "no" | "0" => Some(false),
                _ => None,
            },
        }
    }
    pub fn from_json(v: &Value) -> Self {
        match v {
            Value::Bool(b) => ParamValue::Bool(*b),
            Value::Number(n) => ParamValue::Float(n.as_f64().unwrap_or(0.0)),
            Value::String(s) => ParamValue::Text(s.clone()),
            other => ParamValue::Text(other.to_string()),
        }
    }
}

/// Visits every tunable field. Implementations decide what to do (list, get, set).
pub trait ParamVisitor {
    fn float(&mut self, name: &str, v: &mut f32, min: f32, max: f32, help: &str);
    fn bool(&mut self, name: &str, v: &mut bool, help: &str);
    /// An index into `options` (enums: see `choice_enum!`).
    fn choice(&mut self, name: &str, v: &mut usize, options: &[&str], help: &str);
    /// Called around nested groups.
    fn enter(&mut self, name: &str);
    fn exit(&mut self);
}

pub trait Tunable {
    fn visit(&mut self, v: &mut dyn ParamVisitor);
}

/// Visits `t` as a group called `name` (paths become `name.field`).
pub fn nested(v: &mut dyn ParamVisitor, name: &str, t: &mut dyn Tunable) {
    v.enter(name);
    t.visit(v);
    v.exit();
}

/// Enums usable as choice parameters.
pub trait Choice: Sized + Copy {
    const NAMES: &'static [&'static str];
    fn to_index(self) -> usize;
    fn from_index(i: usize) -> Self;
    fn name(self) -> &'static str {
        Self::NAMES[self.to_index()]
    }
    fn visit_choice(&mut self, v: &mut dyn ParamVisitor, name: &str, help: &str) {
        let mut i = self.to_index();
        v.choice(name, &mut i, Self::NAMES, help);
        *self = Self::from_index(i);
    }
}

/// Declares a C-like enum as a `Choice` (serialised in snake_case).
#[macro_export]
macro_rules! choice_enum {
    ($(#[$m:meta])* $vis:vis enum $name:ident { $($(#[$vm:meta])* $var:ident => $label:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        $vis enum $name { $($(#[$vm])* $var),+ }
        impl $crate::params::Choice for $name {
            const NAMES: &'static [&'static str] = &[$($label),+];
            fn to_index(self) -> usize { self as usize }
            fn from_index(i: usize) -> Self {
                const ALL: &[$name] = &[$($name::$var),+];
                ALL[i.min(ALL.len() - 1)]
            }
        }
    };
}

#[derive(Clone, Debug, Serialize)]
pub struct ParamInfo {
    pub path: String,
    pub value: ParamValue,
    /// "float 0..10", "bool" or "one of a|b|c".
    pub kind: String,
    pub help: String,
}

#[derive(Default)]
struct Path(Vec<String>);

impl Path {
    fn of(&self, name: &str) -> String {
        if self.0.is_empty() { name.to_string() } else { format!("{}.{}", self.0.join("."), name) }
    }
}

/// Every parameter with its current value.
pub fn list(t: &mut dyn Tunable) -> Vec<ParamInfo> {
    struct L(Path, Vec<ParamInfo>);
    impl ParamVisitor for L {
        fn float(&mut self, name: &str, v: &mut f32, min: f32, max: f32, help: &str) {
            let (path, value) = (self.0.of(name), ParamValue::Float((*v as f64 * 1e4).round() / 1e4));
            self.1.push(ParamInfo { path, value, kind: format!("float {min}..{max}"), help: help.into() });
        }
        fn bool(&mut self, name: &str, v: &mut bool, help: &str) {
            let path = self.0.of(name);
            self.1.push(ParamInfo { path, value: ParamValue::Bool(*v), kind: "bool".into(), help: help.into() });
        }
        fn choice(&mut self, name: &str, v: &mut usize, options: &[&str], help: &str) {
            let value = ParamValue::Text(options.get(*v).copied().unwrap_or("?").into());
            let kind = format!("one of {}", options.join("|"));
            self.1.push(ParamInfo { path: self.0.of(name), value, kind, help: help.into() });
        }
        fn enter(&mut self, name: &str) {
            self.0.0.push(name.into());
        }
        fn exit(&mut self) {
            self.0.0.pop();
        }
    }
    let mut l = L(Path::default(), Vec::new());
    t.visit(&mut l);
    l.1
}

/// Sets one parameter by path (numbers are clamped to the range). Errors on unknown paths or
/// values of the wrong kind.
pub fn set(t: &mut dyn Tunable, path: &str, value: &ParamValue) -> Result<(), String> {
    struct S<'a> {
        p: Path,
        path: &'a str,
        value: &'a ParamValue,
        result: Option<Result<(), String>>,
    }
    impl S<'_> {
        fn hit(&self, name: &str) -> bool {
            self.result.is_none() && self.p.of(name) == self.path
        }
        fn bad(&self, want: &str) -> Option<Result<(), String>> {
            Some(Err(format!("'{}' wants {want}, got {:?}", self.path, self.value)))
        }
    }
    impl ParamVisitor for S<'_> {
        fn float(&mut self, name: &str, v: &mut f32, min: f32, max: f32, _: &str) {
            if self.hit(name) {
                self.result = match self.value.as_f64() {
                    Some(x) => {
                        *v = (x as f32).clamp(min.min(max), max.max(min));
                        Some(Ok(()))
                    }
                    None => self.bad("a number"),
                };
            }
        }
        fn bool(&mut self, name: &str, v: &mut bool, _: &str) {
            if self.hit(name) {
                self.result = match self.value.as_bool() {
                    Some(x) => {
                        *v = x;
                        Some(Ok(()))
                    }
                    None => self.bad("true/false"),
                };
            }
        }
        fn choice(&mut self, name: &str, v: &mut usize, options: &[&str], _: &str) {
            if self.hit(name) {
                let text = match self.value {
                    ParamValue::Text(t) => t.clone(),
                    other => other.as_f64().map(|f| f.to_string()).unwrap_or_default(),
                };
                self.result = match options.iter().position(|o| o.eq_ignore_ascii_case(&text)) {
                    Some(i) => {
                        *v = i;
                        Some(Ok(()))
                    }
                    None => self.bad(&format!("one of {}", options.join("|"))),
                };
            }
        }
        fn enter(&mut self, name: &str) {
            self.p.0.push(name.into());
        }
        fn exit(&mut self) {
            self.p.0.pop();
        }
    }
    let mut s = S { p: Path::default(), path, value, result: None };
    t.visit(&mut s);
    s.result.unwrap_or_else(|| Err(format!("unknown parameter '{path}' (see the `params` tool)")))
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
        set(&mut r, "inner.speed", &ParamValue::Float(20.0)).unwrap();
        assert_eq!(r.inner.speed, 10.0); // clamped
        set(&mut r, "mode", &ParamValue::Text("beta".into())).unwrap();
        assert_eq!(r.mode, Mode::B);
        assert!(set(&mut r, "nope", &ParamValue::Bool(true)).is_err());
        assert!(set(&mut r, "on", &ParamValue::Text("maybe".into())).is_err());
        assert_eq!(list(&mut r).len(), 3);
        assert_eq!(get(&mut r, "on"), Some(ParamValue::Bool(false)));
    }
}
