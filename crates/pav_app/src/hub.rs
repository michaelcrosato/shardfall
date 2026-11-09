//! AI Hub sessions (Session Protocol v1, docs/AI_HUB.md): the round's info and events from the
//! page's bridge (`window.shardfallHub`, web/index.html, around the session client), and the
//! game's calls back. Native builds are never in a session.

use serde::Deserialize;

/// A seat in the round.
#[derive(Clone, Debug, Deserialize)]
pub struct Player {
    pub id: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: String,
}

/// The round's `info`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    /// This copy's player id.
    pub me: u32,
    #[serde(default)]
    pub players: Vec<Player>,
    #[serde(default)]
    pub host: u32,
    #[serde(default)]
    pub is_host: bool,
    /// A host with no player of its own (shared world).
    #[serde(default)]
    pub server: bool,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub round: u32,
    #[serde(default)]
    pub limit_ms: f64,
    #[serde(default)]
    pub seed: u64,
}

impl Info {
    pub fn parallel(&self) -> bool {
        self.mode == "parallel"
    }
    pub fn ghost(&self) -> bool {
        self.mode == "ghost" || self.mode == "race"
    }
    pub fn shared(&self) -> bool {
        !self.parallel() && !self.ghost()
    }
    /// This copy has a player of its own (not a hub-run server).
    pub fn playing(&self) -> bool {
        !self.server && self.players.iter().any(|p| p.id == self.me)
    }
    pub fn name(&self, id: u32) -> String {
        self.players.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default()
    }
}

/// An event from the hub.
#[derive(Clone, Debug)]
pub enum Event {
    /// The round starts: play begins `in_ms` later, when `time()` reaches 0.
    Start {
        in_ms: f64,
    },
    Message {
        data: serde_json::Value,
        from: u32,
    },
    Leave {
        id: u32,
        reason: String,
    },
    TimeUp {
        grace_ms: f64,
    },
    End {
        reason: String,
        standings: Option<serde_json::Value>,
    },
}

#[derive(Deserialize)]
struct Raw {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    args: Vec<serde_json::Value>,
}

fn parse(json: &str) -> Vec<Event> {
    let Ok(raw) = serde_json::from_str::<Vec<Raw>>(json) else {
        log::warn!("hub events: not JSON");
        return Vec::new();
    };
    let num = |v: Option<&serde_json::Value>, k: &str| v.and_then(|v| v.get(k)).and_then(|x| x.as_f64()).unwrap_or(0.0);
    raw.into_iter()
        .filter_map(|r| {
            let a0 = r.args.first();
            Some(match r.kind.as_str() {
                "start" => Event::Start { in_ms: num(a0, "in") },
                "message" => Event::Message {
                    data: a0.cloned().unwrap_or_default(),
                    from: r.args.get(1).and_then(|v| v.as_u64()).unwrap_or(u64::MAX) as u32,
                },
                "leave" => Event::Leave {
                    id: num(a0, "id") as u32,
                    reason: a0.and_then(|v| v.get("reason")).and_then(|x| x.as_str()).unwrap_or("").to_string(),
                },
                "time-up" => Event::TimeUp { grace_ms: num(a0, "graceMs") },
                "end" => Event::End {
                    reason: a0.and_then(|v| v.get("reason")).and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    standings: a0.and_then(|v| v.get("standings")).cloned(),
                },
                _ => return None,
            })
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
mod js {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = shardfallHub, js_name = isActive)]
        pub fn is_active() -> bool;
        #[wasm_bindgen(js_namespace = shardfallHub, js_name = isWaiting)]
        pub fn is_waiting() -> bool;
        #[wasm_bindgen(js_namespace = shardfallHub, js_name = infoJson)]
        pub fn info_json() -> String;
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn take() -> String;
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn ready();
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn time() -> f64;
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn send(json: &str, to: i32, reliable: bool) -> bool;
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn progress(score: f64, label: &str);
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn result(json: &str);
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn finish(json: &str);
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn fail(message: &str);
    }
}

/// The page was opened by the hub for a session (and the hub hasn't been found absent).
pub fn active() -> bool {
    #[cfg(target_arch = "wasm32")]
    return js::is_active();
    #[cfg(not(target_arch = "wasm32"))]
    false
}

/// Still asking the hub for the round (connect() not answered yet).
pub fn waiting() -> bool {
    #[cfg(target_arch = "wasm32")]
    return js::is_waiting();
    #[cfg(not(target_arch = "wasm32"))]
    false
}

/// The round's info, once the hub answered.
pub fn info() -> Option<Info> {
    #[cfg(target_arch = "wasm32")]
    {
        let text = js::info_json();
        if text.is_empty() {
            return None;
        }
        return serde_json::from_str(&text).map_err(|e| log::warn!("hub info: {e}")).ok();
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}

/// The hub's events since the last call, in order.
pub fn events() -> Vec<Event> {
    #[cfg(target_arch = "wasm32")]
    return parse(&js::take());
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = parse;
        Vec::new()
    }
}

/// Loaded: the round can start (once, with no key press).
pub fn ready() {
    #[cfg(target_arch = "wasm32")]
    js::ready();
}

/// Milliseconds since play began on every device: negative in the countdown, -inf before start.
pub fn time() -> f64 {
    #[cfg(target_arch = "wasm32")]
    return js::time();
    #[cfg(not(target_arch = "wasm32"))]
    f64::NEG_INFINITY
}

/// Sends JSON to every other copy, or to one player. False when the client dropped it.
pub fn send(data: &serde_json::Value, to: Option<u32>, reliable: bool) -> bool {
    #[cfg(target_arch = "wasm32")]
    return js::send(&data.to_string(), to.map(|t| t as i32).unwrap_or(-1), reliable);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (data, to, reliable);
        false
    }
}

/// This player's standing (parallel and ghost): higher is ahead.
pub fn progress(score: f64, label: &str) {
    #[cfg(target_arch = "wasm32")]
    js::progress(score, label);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (score, label);
}

/// This player's result (parallel and ghost), once.
pub fn result(outcome: &serde_json::Value) {
    #[cfg(target_arch = "wasm32")]
    js::result(&outcome.to_string());
    #[cfg(not(target_arch = "wasm32"))]
    let _ = outcome;
}

/// The round's standings (shared world, the host), once.
pub fn finish(outcome: &serde_json::Value) {
    #[cfg(target_arch = "wasm32")]
    js::finish(&outcome.to_string());
    #[cfg(not(target_arch = "wasm32"))]
    let _ = outcome;
}

/// The game can't run here.
pub fn fail(message: &str) {
    #[cfg(target_arch = "wasm32")]
    js::fail(message);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = message;
}

#[cfg(test)]
mod tests {
    #[test]
    fn events_parse() {
        let e = super::parse(
            r#"[{"type":"start","args":[{"in":3000}]},{"type":"message","args":[{"p":[1,2]},2]},{"type":"leave","args":[{"id":1,"reason":"left"}]},{"type":"time-up","args":[{"graceMs":5000}]},{"type":"end","args":[{"reason":"finished","standings":[[0],[1]]}]}]"#,
        );
        assert_eq!(e.len(), 5);
        assert!(matches!(e[0], super::Event::Start { in_ms } if in_ms == 3000.0));
        assert!(matches!(e[1], super::Event::Message { from: 2, .. }));
        assert!(matches!(&e[2], super::Event::Leave { id: 1, reason } if reason == "left"));
        assert!(matches!(e[3], super::Event::TimeUp { grace_ms } if grace_ms == 5000.0));
        assert!(matches!(&e[4], super::Event::End { reason, .. } if reason == "finished"));
    }
}
