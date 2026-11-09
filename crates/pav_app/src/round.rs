//! A round on the AI Hub (Session Protocol v1, docs/AI_HUB.md), the app's side: joining,
//! building the round's world, holding it until the hub's clock reaches 0, reporting progress
//! and the result, and showing the other players.
//!
//! - Parallel: each copy plays its own copy of the round from the same start.
//! - Ghost: the same, and each copy sends its hero (where it is, its pose) about ten times a
//!   second; the others' heroes are drawn where they are when they're on the same depth. They
//!   are view objects only, so they never touch this copy's world.
//! - Shared world: `crate::coop` (several heroes in one world, kept in lockstep).

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::Vec3;
use pav_core::frame::{RenderFrame, RenderObject};
use pav_core::puppet::{PuppetDef, PuppetState};
use serde::{Deserialize, Serialize};

use crate::hub::{self, Event, Info};

/// Where the round is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for the hub's info.
    Joining,
    /// Built and ready: waiting for the start.
    Waiting,
    /// The start came: counting down to play.
    Countdown,
    Playing,
    /// Time is up (the result is in), waiting for the end.
    Over,
    /// The round is over: no input, no sound, nothing sent.
    Ended,
}

/// Another player's hero, as last heard.
pub struct Ghost {
    pub name: String,
    pub color: egui::Color32,
    def: Option<Arc<PuppetDef>>,
    feet_offset: f32,
    /// Where they are: a place code (`Place::code`).
    pub place: u32,
    /// Shown position (smoothed toward `target`), and the last reported one.
    pub pos: Vec3,
    target: Vec3,
    state: PuppetState,
    /// The walk cycle's speed, from the last two reports.
    phase_rate: f32,
    heard_at: f64,
    seen: bool,
}

/// A ghost's report.
#[derive(Serialize, Deserialize)]
struct Report {
    /// Kind: "g" (where and how), "look" (the hero's look, once).
    k: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pl: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    p: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    s: Option<PuppetState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    def: Option<PuppetDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fo: Option<f32>,
}

/// The seat colours the hub uses.
pub fn seat_color(id: u32) -> egui::Color32 {
    match id % 4 {
        0 => egui::Color32::from_rgb(0xe8, 0x44, 0x3a),
        1 => egui::Color32::from_rgb(0x3b, 0x82, 0xf6),
        2 => egui::Color32::from_rgb(0x22, 0xc5, 0x5e),
        _ => egui::Color32::from_rgb(0xf5, 0xb7, 0x00),
    }
}

/// What the app should do this frame.
#[derive(Default)]
pub struct RoundOut {
    /// Build the round's world: seed and the hero's character.
    pub build: Option<(u64, &'static str)>,
    /// Build the shared world (every player's hero) from this seed (`Round::coop`).
    pub build_shared: Option<u64>,
    /// Hold the world still.
    pub hold: bool,
    /// Let the player steer.
    pub input: bool,
    /// The round ended: silence.
    pub silent: bool,
}

pub struct Round {
    pub info: Option<Info>,
    pub phase: Phase,
    pub end_reason: String,
    built: bool,
    reported: bool,
    clock: f64,
    last_progress: f64,
    last_send: f64,
    look_sent: bool,
    pub ghosts: BTreeMap<u32, Ghost>,
    /// This player's depth and kills.
    pub standing: (u32, u64),
    /// Milliseconds on the hub's clock (negative in the countdown).
    pub hub_time: f64,
    /// A shared world (`crate::coop`).
    pub coop: Option<crate::coop::Coop>,
    /// Time is up in a shared world: the host finishes the round.
    pub finish_due: bool,
    /// The players' names arrived: the app loads the fonts their scripts need.
    pub fonts_due: bool,
}

impl Round {
    pub fn new() -> Self {
        Self {
            info: None,
            phase: Phase::Joining,
            end_reason: String::new(),
            built: false,
            reported: false,
            clock: 0.0,
            last_progress: -10.0,
            last_send: -10.0,
            look_sent: false,
            ghosts: BTreeMap::new(),
            standing: (1, 0),
            hub_time: f64::NEG_INFINITY,
            coop: None,
            finish_due: false,
            fonts_due: false,
        }
    }

    /// The round ended for good: the app stops.
    pub fn ended(&self) -> bool {
        self.phase == Phase::Ended
    }

    /// Before the start: the hub's card is over the game, so it needs few frames (and other
    /// players' copies may still be loading on the same machine).
    pub fn idle(&self) -> bool {
        matches!(self.phase, Phase::Joining | Phase::Waiting)
    }

    /// The round's world is built: the round can start (`ready`, once, with no key press).
    pub fn built(&mut self) {
        if !self.built {
            self.built = true;
            self.phase = Phase::Waiting;
            hub::ready();
        }
    }

    /// One frame: the hub's events, the clock, reports. `frame` is the newest render frame.
    pub fn update(&mut self, frame: &RenderFrame, dt: f32) -> RoundOut {
        self.clock += dt as f64;
        let mut out = RoundOut::default();
        if self.info.is_none() {
            if let Some(info) = hub::info() {
                log::info!("round {}: {} players, mode {}, seed {}", info.round, info.players.len(), info.mode, info.seed);
                let seat = info.players.iter().position(|p| p.id == info.me).unwrap_or(0);
                if info.shared() {
                    self.coop = Some(crate::coop::Coop::new(&info));
                    out.build_shared = Some(info.seed);
                } else {
                    out.build = Some((info.seed, pav_core::arpg::round::SEAT_CHARACTERS[seat % 4]));
                }
                for p in info.players.iter().filter(|p| p.id != info.me && !info.shared()) {
                    self.ghosts.insert(
                        p.id,
                        Ghost {
                            name: p.name.clone(),
                            color: info.color(p.id),
                            def: None,
                            feet_offset: 0.9,
                            place: u32::MAX,
                            pos: Vec3::ZERO,
                            target: Vec3::ZERO,
                            state: PuppetState::default(),
                            phase_rate: 0.0,
                            heard_at: -10.0,
                            seen: false,
                        },
                    );
                }
                self.info = Some(info);
                self.fonts_due = true;
            }
            out.hold = true;
            return out;
        }
        if let Some(g) = &frame.game {
            let depth = match g.place {
                pav_core::arpg::Place::Level(n) => n,
                _ => 0,
            };
            self.standing = (depth, g.hero.as_ref().map(|h| h.kills).unwrap_or(0));
        }
        for e in hub::events() {
            self.event(e);
        }
        self.hub_time = hub::time();
        if self.phase == Phase::Countdown && self.hub_time >= 0.0 {
            self.phase = Phase::Playing;
        }
        let info = self.info.clone().unwrap_or_default();
        // Progress from the moment this copy is ready: the hub shows it on the scoreboard.
        let live = matches!(self.phase, Phase::Waiting | Phase::Countdown | Phase::Playing);
        if live && !info.shared() && self.clock - self.last_progress >= 1.0 {
            self.last_progress = self.clock;
            let (depth, kills) = self.standing;
            let label = pav_core::lines::logical("session.progress", &[("depth", depth.into()), ("kills", kills.into())]);
            hub::progress(pav_core::arpg::round::score(depth, kills), &label);
        }
        if info.ghost() && live {
            self.send_ghost(frame);
        }
        self.smooth_ghosts(dt);
        // A shared world steps by the host's ticks (`coop`), never by the clock.
        out.hold = !matches!(self.phase, Phase::Playing) || self.coop.is_some();
        out.input = self.phase == Phase::Playing;
        out.silent = self.phase == Phase::Ended;
        out
    }

    fn event(&mut self, e: Event) {
        match e {
            Event::Start { in_ms } => {
                log::info!("round starts in {in_ms} ms");
                if matches!(self.phase, Phase::Joining | Phase::Waiting) {
                    self.phase = Phase::Countdown;
                }
            }
            Event::Message { data, from } => self.message(data, from),
            Event::Leave { id, reason } => {
                log::info!("player {id} left ({reason})");
                self.ghosts.remove(&id);
                if let Some(c) = self.coop.as_mut() {
                    c.leave(id);
                }
            }
            Event::TimeUp { grace_ms } => {
                log::info!("time is up ({grace_ms} ms to report)");
                self.finish_due = self.coop.is_some();
                self.report();
                if self.phase != Phase::Ended {
                    self.phase = Phase::Over;
                }
            }
            Event::End { reason, .. } => {
                log::info!("round ended: {reason}");
                self.end_reason = reason;
                self.phase = Phase::Ended;
            }
        }
    }

    /// Parallel and ghost: this player's result, once (the depth reached, then kills).
    fn report(&mut self) {
        if self.reported || self.info.as_ref().is_none_or(|i| i.shared()) {
            return;
        }
        self.reported = true;
        let (depth, kills) = self.standing;
        let depth_label = pav_core::lines::logical("session.stat.depth", &[]);
        let kills_label = pav_core::lines::logical("session.stat.kills", &[]);
        hub::result(&serde_json::json!({
            "score": pav_core::arpg::round::score(depth, kills),
            "stats": { depth_label: depth, kills_label: kills },
        }));
    }

    fn message(&mut self, data: serde_json::Value, from: u32) {
        if let Some(c) = self.coop.as_mut() {
            c.message(&data, from);
            return;
        }
        let Ok(r) = serde_json::from_value::<Report>(data) else { return };
        let now = self.clock;
        let Some(g) = self.ghosts.get_mut(&from) else { return };
        match r.k.as_str() {
            "look" => {
                g.def = r.def.map(Arc::new);
                if let Some(fo) = r.fo {
                    g.feet_offset = fo;
                }
            }
            "g" => {
                let (Some(pl), Some(p), Some(s)) = (r.pl, r.p, r.s) else { return };
                let p = Vec3::from(p);
                if !g.seen || pl != g.place || g.target.distance(p) > 6.0 {
                    // A first report, another place, or a jump (travel, respawn): snap.
                    g.pos = p;
                }
                let gap = (now - g.heard_at).clamp(0.02, 1.0) as f32;
                let mut dp = s.phase - g.state.phase;
                if dp < 0.0 {
                    dp += 1.0;
                }
                g.phase_rate = if g.seen { (dp / gap).clamp(0.0, 4.0) } else { 0.0 };
                g.place = pl;
                g.target = p;
                g.state = s;
                g.heard_at = now;
                g.seen = true;
            }
            _ => {}
        }
    }

    /// Ghost: this hero's look (once) and where it is and how it moves (ten times a second).
    fn send_ghost(&mut self, frame: &RenderFrame) {
        let Some(pid) = frame.player else { return };
        let Some(me) = frame.objects.iter().find(|o| o.id == pid) else { return };
        let Some(puppet) = &me.puppet else { return };
        let place = frame.game.as_ref().map(|g| g.place.code()).unwrap_or(0);
        if !self.look_sent {
            let def = puppet.def.as_deref().cloned().unwrap_or_else(|| frame.puppet_def.clone());
            let r = Report { k: "look".into(), pl: None, p: None, s: None, def: Some(def), fo: Some(puppet.feet_offset) };
            self.look_sent = hub::send(&serde_json::to_value(&r).unwrap_or_default(), None, true);
        }
        if self.clock - self.last_send < 0.1 {
            return;
        }
        self.last_send = self.clock;
        let r = Report { k: "g".into(), pl: Some(place), p: Some(me.pos.to_array()), s: Some(puppet.state), def: None, fo: None };
        hub::send(&serde_json::to_value(&r).unwrap_or_default(), None, false);
    }

    fn smooth_ghosts(&mut self, dt: f32) {
        let k = 1.0 - (-dt * 12.0).exp();
        for g in self.ghosts.values_mut() {
            g.pos += (g.target - g.pos) * k;
            g.state.phase = (g.state.phase + g.phase_rate * dt).fract();
            g.state.time += dt;
        }
    }

    /// The other players' heroes on this depth, as objects to draw (copied from this player's
    /// own hero, so they look and light the same).
    pub fn ghost_objects(&self, frame: &RenderFrame) -> Vec<RenderObject> {
        let Some(pid) = frame.player else { return Vec::new() };
        let Some(me) = frame.objects.iter().find(|o| o.id == pid) else { return Vec::new() };
        let place = frame.game.as_ref().map(|g| g.place.code()).unwrap_or(0);
        let mut out = Vec::new();
        for (id, g) in &self.ghosts {
            if !g.seen || g.place != place {
                continue;
            }
            let mut o = me.clone();
            o.id = pav_core::EntityId(u32::MAX - 16 - id);
            o.pos = g.pos;
            if let Some(p) = o.puppet.as_mut() {
                p.state = g.state;
                p.feet_offset = g.feet_offset;
                p.def = Some(g.def.clone().unwrap_or_else(|| Arc::new(frame.puppet_def.clone())));
                p.tint = Some(([g.color.r() as f32 / 255.0, g.color.g() as f32 / 255.0, g.color.b() as f32 / 255.0], 0.18));
            }
            out.push(o);
        }
        out
    }

    /// Ghosts' names over their heads, the countdown, and the wait before the start.
    pub fn overlay(&self, ctx: &egui::Context, frame: &RenderFrame, view_proj: glam::Mat4) {
        let screen = ctx.content_rect();
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("round")));
        let place = frame.game.as_ref().map(|g| g.place.code()).unwrap_or(0);
        for g in self.ghosts.values().filter(|g| g.seen && g.place == place) {
            let head = view_proj.project_point3(g.pos + Vec3::Y * 1.4);
            if head.z < 0.0 || head.z > 1.0 || head.x.abs() > 1.2 || head.y.abs() > 1.2 {
                continue;
            }
            let at = egui::pos2((head.x * 0.5 + 0.5) * screen.width(), (0.5 - head.y * 0.5) * screen.height());
            let name = if g.name.is_empty() { "?".to_string() } else { g.name.clone() };
            p.text(
                at + egui::vec2(1.0, 1.0),
                egui::Align2::CENTER_BOTTOM,
                &name,
                egui::FontId::proportional(14.0),
                egui::Color32::from_black_alpha(200),
            );
            p.text(at, egui::Align2::CENTER_BOTTOM, &name, egui::FontId::proportional(14.0), g.color);
        }
        // A shared world: the other players' names over their heroes.
        if let (Some(g), Some(info)) = (&frame.game, &self.info) {
            for (seat, id) in g.heroes.iter().filter(|(s, _)| Some(*s) != info.playing().then_some(info.me)) {
                let Some(a) = g.actors.iter().find(|a| a.id == *id) else { continue };
                let head = view_proj.project_point3(a.feet + Vec3::Y * (a.height + 0.4));
                if head.z < 0.0 || head.z > 1.0 || head.x.abs() > 1.2 || head.y.abs() > 1.2 {
                    continue;
                }
                let at = egui::pos2((head.x * 0.5 + 0.5) * screen.width(), (0.5 - head.y * 0.5) * screen.height());
                let name = info.name(*seat);
                p.text(
                    at + egui::vec2(1.0, 1.0),
                    egui::Align2::CENTER_BOTTOM,
                    &name,
                    egui::FontId::proportional(14.0),
                    egui::Color32::from_black_alpha(200),
                );
                p.text(at, egui::Align2::CENTER_BOTTOM, &name, egui::FontId::proportional(14.0), info.color(*seat));
            }
        }
        let center = screen.center();
        match self.phase {
            Phase::Joining | Phase::Waiting => {
                p.text(
                    center + egui::vec2(0.0, screen.height() * 0.25),
                    egui::Align2::CENTER_CENTER,
                    pav_core::t!("session.waiting"),
                    egui::FontId::proportional(22.0),
                    egui::Color32::from_rgb(240, 198, 112),
                );
            }
            Phase::Countdown => {
                let left = (-self.hub_time / 1000.0).ceil().max(1.0) as i64;
                p.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    pav_core::lines::format_int(&pav_core::lines::language(), left),
                    egui::FontId::proportional(96.0),
                    egui::Color32::from_rgb(240, 198, 112),
                );
            }
            _ => {}
        }
    }
}
