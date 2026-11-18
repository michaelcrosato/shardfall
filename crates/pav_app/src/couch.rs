//! Same-screen co-op (52in1 catalogue v1, "Playing together": `same-screen co-op 2-4`): up to
//! three more players, each on a controller of their own, join the hero on one screen. A pad
//! that is not the player's own shows "✕ Join"; Cross or Options there adds a hero for it
//! (`Sim::add_party_hero`, at the hero's level: Kestrel, Ysolde, then the Wanderer), and holding
//! Options a moment takes it out again. Each local hero plays as the game's own does (the party
//! of the shared world, `pav_core::arpg::party`): monsters go for the nearest, kills count for
//! whoever made them, travel takes everyone. The screen keeps everyone in view: the camera
//! frames the party and draws back as it spreads, and a player who strays too far is brought
//! back to the hero. Local players don't trade or open the hero's windows; they fight, loot gold
//! and potions, and level up. Online rounds (52in1 sessions) have the hub's modes instead.

use glam::{Vec2, Vec3};
use pav_core::InputFrame;
use pav_core::arpg::GameFrame;
use pav_core::arpg::combat::Team;
use pav_core::input::buttons;
use pav_ui::glyph::{Glyph, Pad, PadStyle, Prompt};
use pav_view::camera::CameraRig;

use crate::input::ExtraPad;
use crate::simhost::SimHost;

/// The seats local players take (the game's own hero is seat 0) and who they play.
const SEATS: [(u32, &str); 3] = [(1, "kestrel"), (2, "ysolde"), (3, "wanderer")];
/// Farther than this from the hero, a local player is brought back beside them.
const LEASH: f32 = 15.0;
/// Seconds Options is held to leave.
const LEAVE_HOLD: f32 = 1.2;

/// A local player.
pub struct Local {
    pub seat: u32,
    /// Their controller (gilrs's id).
    pub pad: usize,
    leave: f32,
    /// Where they last aimed (a unit direction on the ground): kept while the right stick rests.
    aim: Vec2,
}

#[derive(Default)]
pub struct Couch {
    pub players: Vec<Local>,
    /// Controllers connected that have not joined.
    pub waiting: Vec<usize>,
    /// The camera's distance before the party spread it.
    base_distance: Option<f32>,
}

impl Couch {
    pub fn active(&self) -> bool {
        !self.players.is_empty()
    }

    /// One frame: joins, leaves, each local player's input, and stragglers brought back.
    /// `allowed`: the game is being played here (Shardfall, no menu, no 52in1 round). Returns
    /// what to tell the players.
    pub fn frame(
        &mut self,
        pads: &[ExtraPad],
        game: Option<&GameFrame>,
        host: &SimHost,
        rig: &CameraRig,
        dt: f32,
        allowed: bool,
    ) -> Vec<String> {
        let mut said = Vec::new();
        // Out of the game, or a controller unplugged: those players leave.
        let gone: Vec<u32> =
            self.players.iter().filter(|p| game.is_none() || !pads.iter().any(|x| x.id == p.pad)).map(|p| p.seat).collect();
        for seat in gone {
            self.remove(seat, host);
            said.push(pav_core::t!("couch.left", player = seat as i64 + 1));
        }
        self.waiting = pads.iter().map(|p| p.id).filter(|id| !self.players.iter().any(|p| p.pad == *id)).collect();
        let Some(g) = game else {
            self.base_distance = None;
            return said;
        };
        // Joining.
        for pad in pads.iter().filter(|p| self.waiting.contains(&p.id)) {
            let wants = pad.taps.iter().any(|b| matches!(b, Pad::Cross | Pad::Options));
            let Some(&(seat, character)) = SEATS.iter().find(|(s, _)| !self.players.iter().any(|p| p.seat == *s)) else {
                break;
            };
            if allowed && wants {
                let level = g.hero.as_ref().map(|h| h.level).unwrap_or(1);
                host.exec(move |sim| {
                    sim.add_party_hero(seat, character);
                    sim.set_party_level(seat, level);
                });
                self.players.push(Local { seat, pad: pad.id, leave: 0.0, aim: Vec2::Y });
                said.push(pav_core::t!("couch.joined", player = seat as i64 + 1));
            }
        }
        self.waiting.retain(|id| !self.players.iter().any(|p| p.pad == *id));
        let hero = g.actors.iter().find(|a| Some(a.id) == hero_id(g, 0)).map(|a| a.feet);
        let mut leaving = Vec::new();
        for p in self.players.iter_mut() {
            let Some(pad) = pads.iter().find(|x| x.id == p.pad) else { continue };
            let st = &pad.state;
            // Holding Options takes the player out.
            p.leave = if st.held.contains(&Pad::Options) { p.leave + dt } else { 0.0 };
            if p.leave >= LEAVE_HOLD {
                leaving.push(p.seat);
                continue;
            }
            let id = hero_id(g, p.seat);
            let feet = g.actors.iter().find(|a| Some(a.id) == id).map(|a| a.feet);
            let input = if allowed { local_input(st, &pad.taps, rig, feet, &mut p.aim) } else { InputFrame::default() };
            let seat = p.seat;
            host.exec(move |sim| sim.set_party_input(seat, input));
            // A straggler (or one left behind by the hero's travel) comes back beside the hero.
            if let (Some(id), Some(f), Some(h)) = (id, feet, hero) {
                if (f - h).length() > LEASH {
                    let at = h + Vec3::new(((seat as f32) - 2.0) * 1.2, 0.0, 1.0);
                    host.exec(move |sim| {
                        sim.set_position(id, at);
                    });
                }
            }
        }
        for seat in leaving {
            self.remove(seat, host);
            said.push(pav_core::t!("couch.left", player = seat as i64 + 1));
        }
        said
    }

    fn remove(&mut self, seat: u32, host: &SimHost) {
        self.players.retain(|p| p.seat != seat);
        host.exec(move |sim| sim.remove_party_hero(seat));
    }

    /// The camera: the party's middle, and how far back to stand so everyone fits (the
    /// distance grows with how spread out they are).
    pub fn camera(&mut self, g: &GameFrame, focus: Vec3, rig: &mut CameraRig) -> Vec3 {
        if !self.active() {
            if let Some(d) = self.base_distance.take() {
                rig.params.distance = d;
            }
            return focus;
        }
        let base = *self.base_distance.get_or_insert(rig.params.distance);
        let feet: Vec<Vec3> =
            g.heroes.iter().filter_map(|(_, id)| g.actors.iter().find(|a| a.id == *id && !a.dead).map(|a| a.feet)).collect();
        if feet.is_empty() {
            return focus;
        }
        let mid = feet.iter().copied().sum::<Vec3>() / feet.len() as f32;
        let spread = feet.iter().map(|f| (*f - mid).length()).fold(0.0, f32::max);
        rig.params.distance = base * (1.0 + (spread / 10.0).clamp(0.0, 0.7));
        Vec3::new(mid.x, focus.y, mid.z)
    }

    /// Each local player's card (top left, under the hero's line) and their name over their
    /// hero; the waiting controllers' "join" prompt.
    pub fn hud(&self, ctx: &egui::Context, g: &GameFrame, proj: &pav_ui::arpg_ui::Projector, style: PadStyle, allowed: bool) {
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("couch")));
        let screen = ctx.content_rect();
        for (k, pl) in self.players.iter().enumerate() {
            let color = crate::round::seat_color(pl.seat);
            let actor = hero_id(g, pl.seat).and_then(|id| g.actors.iter().find(|a| a.id == id));
            let name = pav_core::t!("couch.player", player = pl.seat as i64 + 1);
            let top = egui::pos2(14.0, 54.0 + k as f32 * 40.0);
            p.circle_filled(top + egui::vec2(8.0, 10.0), 7.0, color);
            let level = actor.map(|a| a.level).unwrap_or(1);
            p.text(
                top + egui::vec2(22.0, 0.0),
                egui::Align2::LEFT_TOP,
                pav_core::t!("couch.card", player = name.as_str(), level = level as i64),
                egui::FontId::proportional(16.0),
                egui::Color32::WHITE,
            );
            let bar = egui::Rect::from_min_size(top + egui::vec2(22.0, 21.0), egui::vec2(150.0, 7.0));
            p.rect_filled(bar.expand(1.5), 3.0, egui::Color32::from_black_alpha(200));
            let mut fill = bar;
            fill.set_width(bar.width() * actor.map(|a| if a.dead { 0.0 } else { a.life }).unwrap_or(0.0));
            p.rect_filled(fill, 2.0, egui::Color32::from_rgb(200, 40, 40));
            // Their name over their hero, in their colour.
            if let Some(a) = actor {
                if let Some(at) = proj.to_screen(a.feet + Vec3::Y * (a.height + 0.5)) {
                    p.text(
                        at + egui::vec2(1.0, 1.0),
                        egui::Align2::CENTER_BOTTOM,
                        &name,
                        egui::FontId::proportional(15.0),
                        egui::Color32::from_black_alpha(200),
                    );
                    p.text(at, egui::Align2::CENTER_BOTTOM, &name, egui::FontId::proportional(15.0), color);
                }
            }
        }
        if allowed && !self.waiting.is_empty() && self.players.len() < SEATS.len() {
            let prompts = [Prompt { glyphs: vec![Glyph::Pad(Pad::Cross)], label: pav_core::t!("couch.join") }];
            pav_ui::glyph::row(
                &p,
                screen.right_bottom() - egui::vec2(18.0, 18.0),
                egui::Align2::RIGHT_BOTTOM,
                &prompts,
                24.0,
                style,
                egui::Color32::from_white_alpha(220),
            );
        }
    }
}

/// A seat's hero in the game frame.
fn hero_id(g: &GameFrame, seat: u32) -> Option<pav_core::EntityId> {
    match g.heroes.iter().find(|(s, _)| *s == seat) {
        Some((_, id)) => Some(*id),
        // Outside a shared world the game's own hero has no seat listed: the first hero actor.
        None if seat == 0 => g.actors.iter().find(|a| a.team == Team::Hero).map(|a| a.id),
        None => None,
    }
}

/// A local player's input from their pad: the game's layout (`pav_ui::controls::GAME_PAD`),
/// moving and aiming as the camera sees it. Use (D-pad right) is the hero's alone.
fn local_input(st: &crate::input::PadState, taps: &[Pad], rig: &CameraRig, feet: Option<Vec3>, aim: &mut Vec2) -> InputFrame {
    let bit = |b: &Pad| pav_ui::controls::GAME_PAD.iter().find(|(p, _)| p == b).map(|(_, bit)| *bit).unwrap_or(0);
    let mut held = st.held.iter().map(bit).fold(0, |a, b| a | b);
    let mut pressed = taps.iter().map(bit).fold(0, |a, b| a | b);
    held &= !buttons::INTERACT;
    pressed &= !buttons::INTERACT;
    let move_dir = rig.relative_move(st.left);
    if st.right.length() > 0.2 {
        *aim = rig.relative_move(st.right).normalize_or(*aim);
    } else if move_dir.length() > 0.3 {
        *aim = move_dir.normalize_or(*aim);
    }
    InputFrame {
        move_dir,
        held: held | pressed,
        pressed,
        aim: feet.map(|f| f + Vec3::new(aim.x, 0.0, aim.y) * 5.0),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_pad_plays_the_games_layout_but_never_trades() {
        let rig = CameraRig::default();
        let st = crate::input::PadState { held: vec![Pad::Square, Pad::Right], ..Default::default() };
        let mut aim = Vec2::Y;
        let i = local_input(&st, &[Pad::Cross, Pad::Right], &rig, Some(Vec3::ZERO), &mut aim);
        assert_eq!(i.pressed, buttons::DODGE, "Cross dodges; use is the hero's");
        assert_ne!(i.held & buttons::PRIMARY, 0, "Square is the basic attack");
        assert_eq!(i.held & buttons::INTERACT, 0);
        assert!(i.aim.is_some());
    }
}
