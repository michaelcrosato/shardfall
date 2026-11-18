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
    /// The seat's colour, as the hub shows it (`#rrggbb`).
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
    /// A shared world's play, when the page's `hub:multiplayer` names it: `co-op`, `versus` or
    /// `other` (none: the shared world as it always played, co-op).
    #[serde(default)]
    pub play: Option<String>,
    /// Moments: the moment this load plays (`arpg::moments`).
    #[serde(default)]
    pub moment: Option<String>,
    #[serde(default)]
    pub round: u32,
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
    pub fn moments(&self) -> bool {
        self.mode == "moments"
    }
    /// One world for every player, run by the host (`co-op` and `versus` are older names).
    pub fn shared(&self) -> bool {
        matches!(self.mode.as_str(), "shared-world" | "co-op" | "versus")
    }
    /// A shared world played against each other: the standings say who won.
    pub fn versus(&self) -> bool {
        self.play.as_deref() == Some("versus") || (self.mode == "versus" && self.play.is_none())
    }
    /// This copy has a player of its own (not a hub-run server).
    pub fn playing(&self) -> bool {
        !self.server && self.players.iter().any(|p| p.id == self.me)
    }
    pub fn name(&self, id: u32) -> String {
        self.players.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default()
    }
    /// The player's colour as the hub shows it, or the usual seat colour.
    pub fn color(&self, id: u32) -> egui::Color32 {
        self.players
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| egui::Color32::from_hex(p.color.trim()).ok())
            .unwrap_or_else(|| crate::round::seat_color(id))
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
                "end" => {
                    Event::End { reason: a0.and_then(|v| v.get("reason")).and_then(|x| x.as_str()).unwrap_or("").to_string() }
                }
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
        #[wasm_bindgen(js_namespace = shardfallHub, js_name = momentStanding)]
        pub fn moment_standing(won: bool);
        #[wasm_bindgen(js_namespace = shardfallHub, js_name = settingsJson)]
        pub fn settings_json() -> String;
        #[wasm_bindgen(js_namespace = shardfallHub, js_name = takeSettingsChanged)]
        pub fn take_settings_changed() -> bool;
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
        serde_json::from_str(&text).map_err(|e| log::warn!("hub info: {e}")).ok()
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

/// Moments: what the page reports at time-up if the game can't answer in time (a slow frame).
pub fn moment_standing(won: bool) {
    #[cfg(target_arch = "wasm32")]
    js::moment_standing(won);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = won;
}

/// The player's 52in1 settings (`window.AIHubSettings`), as the hub hands them to every game it
/// opens. None outside 52in1 (the game's own page, a native build): the game plays as it is.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Music volume, 0 to 1 (Shardfall has no music: kept for what may come).
    pub music: f32,
    /// Every other sound, 0 to 1: multiplies the game's own effects volume.
    pub effects: f32,
    /// Controller vibration, 0 (off) to 1. The hub scales every rumble by it itself.
    pub vibration: f32,
    /// The player allowed games the microphone (before this game opened).
    pub mic: bool,
    pub mic_later: bool,
    pub muted: bool,
    pub dualsense: DualSense,
}

/// What a connected DualSense's extras allow (catalogue v1, "The DualSense").
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct DualSense {
    pub connected: bool,
    pub lightbar: bool,
    pub triggers: bool,
    pub motion: bool,
    pub touchpad: bool,
    pub speaker: bool,
    pub aim: bool,
}

/// The player's 52in1 settings, when the hub runs the game.
pub fn settings() -> Option<Settings> {
    #[cfg(target_arch = "wasm32")]
    {
        let text = js::settings_json();
        if text.is_empty() {
            return None;
        }
        serde_json::from_str(&text).map_err(|e| log::warn!("52in1 settings: {e}")).ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}

/// The player changed their 52in1 settings since the last call (they can open Settings over
/// the game from the hub's bar).
pub fn settings_changed() -> bool {
    #[cfg(target_arch = "wasm32")]
    return js::take_settings_changed();
    #[cfg(not(target_arch = "wasm32"))]
    false
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

    #[test]
    fn modes_and_settings_read() {
        let i: super::Info = serde_json::from_str(
            r##"{"me":1,"players":[{"id":0,"name":"A","color":"#e8443a"},{"id":1,"name":"B","color":"#3b82f6"}],"host":0,"isHost":false,"mode":"shared-world","play":"versus","round":2,"seed":9,"limitMs":480000}"##,
        )
        .unwrap();
        assert!(i.shared() && i.versus() && !i.moments());
        let m: super::Info = serde_json::from_str(r#"{"me":0,"mode":"moments","moment":"gold","seed":3}"#).unwrap();
        assert!(m.moments() && !m.shared() && m.moment.as_deref() == Some("gold"));
        let old: super::Info = serde_json::from_str(r#"{"me":0,"mode":"co-op"}"#).unwrap();
        assert!(old.shared() && !old.versus());
        let s: super::Settings = serde_json::from_str(
            r#"{"music":0.5,"effects":0.25,"vibration":1,"mic":false,"micLater":true,"muted":false,"dualsense":{"connected":true,"lightbar":true,"triggers":false,"motion":true,"touchpad":true,"speaker":false,"aim":true}}"#,
        )
        .unwrap();
        assert_eq!(s.effects, 0.25);
        assert!(s.mic_later && s.dualsense.connected && s.dualsense.lightbar && !s.dualsense.triggers);
    }
}
