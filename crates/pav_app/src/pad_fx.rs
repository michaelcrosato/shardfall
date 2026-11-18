//! A controller's extras, where they make sense for the game (52in1 catalogue v1, "The
//! DualSense"): short rumbles that say something (the hero hurt, a crit, a slam or a blast close
//! by, a level-up, a unique picked up, death, a car's gearbox shifting), the DualSense's light
//! bar showing the hero's life, R2's adaptive trigger telling by feel whether its skill is ready,
//! and the touchpad (its press opens the map, a swipe turns the hero's panels). Each one adds to
//! a control that works on its own: the game plays the same on any pad, and on keys, without
//! them.
//!
//! - Rumble: any pad that rumbles. In the browser the page plays `vibrationActuator`'s
//!   dual-rumble; natively gilrs's force feedback. Settings' vibration turns it off, and in 52in1
//!   the hub scales it by the player's own vibration setting.
//! - Light bar, adaptive triggers, touchpad swipes: a DualSense in 52in1, through the hub
//!   (`window.AIHubSettings.dualsense`), each only while the player allows it. The game never
//!   opens the controller itself. The touchpad's press is the standard mapping's button 17.
//! - The controller's speaker: a DualSense in 52in1 whose speaker the player allows
//!   (`dualsense.speaker`), with the game's Controller speaker setting on. Three short cues play
//!   from it on top of the game's own sounds (`speaker_cue`): a potion drunk, a rare or unique
//!   item picked up, a level-up. The page synthesizes them into the node the hub's
//!   `openSpeaker()` gives; without one nothing more plays.

use glam::Vec3;
use pav_core::arpg::GameFrame;
use pav_core::frame::SimEvent;

pub use crate::input::PadExtra;

/// What the extras need to know this frame.
pub struct Now<'a> {
    pub game: Option<&'a GameFrame>,
    pub events: &'a [SimEvent],
    pub hero: Option<Vec3>,
    /// The player is on a controller.
    pub pad: bool,
    /// Settings: vibration on.
    pub vibration: bool,
    pub hub: Option<&'a crate::hub::Settings>,
    /// A menu or window has the pad (the trigger lets go).
    pub menus: bool,
    /// The gear of the car the player drives (`pav_core::vehicle::Gearbox`).
    pub gear: Option<i8>,
    pub dt: f32,
    /// The controller speaker's volume (the game's effects times 52in1's), 0 while it is off
    /// (`speaker_on`).
    pub speaker: f32,
}

/// The player's 52in1 settings let the game play from the DualSense's speaker (a DualSense on a
/// cable, the microphone allowed under Sound).
pub fn speaker_allowed(hub: Option<&crate::hub::Settings>) -> bool {
    hub.is_some_and(|h| h.dualsense.connected && h.dualsense.speaker)
}

/// The speaker plays the game's cues: allowed, and the game's Controller speaker setting on.
pub fn speaker_on(hub: Option<&crate::hub::Settings>, pref: bool) -> bool {
    speaker_allowed(hub) && pref
}

/// The cue this frame plays from the controller's speaker (`shardfallHub.speaker` names them),
/// the rarest of what happened to the player's own hero: a unique or rare picked up (`pickup`,
/// its rarity), a level gained, a potion drunk. A party's others play none on this controller.
pub fn speaker_cue(drank: bool, levelled: bool, pickup: Option<u8>) -> Option<&'static str> {
    match pickup {
        Some(r) if r >= 3 => Some("unique"),
        _ if levelled => Some("level"),
        Some(2) => Some("rare"),
        _ => drank.then_some("potion"),
    }
}

/// An item picked up at `at` was the player's hero's: it is the nearest hero to it, within a
/// label click's reach (loot's `CLICK_REACH`, and a little).
fn picked_by_hero(g: &GameFrame, hero: Option<Vec3>, at: Vec3) -> bool {
    let Some(h) = hero else { return false };
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).length();
    let d = flat(at - h);
    d <= pav_core::arpg::loot::CLICK_REACH + 0.5
        && !g
            .actors
            .iter()
            .any(|a| a.team == pav_core::arpg::combat::Team::Hero && flat(a.feet - h) > 0.3 && flat(at - a.feet) < d)
}

#[derive(Default)]
pub struct PadFx {
    clock: f64,
    last_life: Option<f32>,
    dead: bool,
    last_gear: Option<i8>,
    /// The hero's potions and level last frame (a drink, a level gained: the player's own).
    last_potions: Option<u32>,
    last_level: Option<u32>,
    /// What the light bar and R2 were last told ("" = given back to the hub, let go).
    light: String,
    trigger: String,
    /// Rumble running until (clock seconds), and how strong.
    until: f64,
    strength: f32,
    #[cfg(not(target_arch = "wasm32"))]
    effect: Option<gilrs::ff::Effect>,
}

fn near(hero: Option<Vec3>, at: Vec3, reach: f32) -> bool {
    hero.is_some_and(|h| (h - at).length() < reach)
}

impl PadFx {
    /// One frame: what to rumble, the light bar's colour and R2's feel.
    pub fn update(&mut self, now: Now, gilrs: Option<&mut gilrs::Gilrs>) {
        self.clock += now.dt as f64;
        let mut want: Option<(f32, f32, u32)> = None;
        let mut add = |strong: f32, weak: f32, ms: u32| {
            want = Some(match want {
                Some((s, w, m)) => (s.max(strong), w.max(weak), m.max(ms)),
                None => (strong, weak, ms),
            });
        };
        let hero = now.game.and_then(|g| g.hero.as_ref());
        let (mut drank, mut levelled) = (false, false);
        match hero {
            Some(h) => {
                drank = self.last_potions.is_some_and(|p| h.potions < p);
                levelled = self.last_level.is_some_and(|l| h.level > l);
                self.last_potions = Some(h.potions);
                self.last_level = Some(h.level);
                let life = (h.life / h.life_max.max(1.0)).clamp(0.0, 1.0);
                if let Some(last) = self.last_life {
                    let lost = last - life;
                    if lost > 0.02 {
                        // Hurt: heavier the more it took.
                        add((lost * 3.0).min(1.0), (lost * 4.0).clamp(0.25, 1.0), 80 + (lost * 500.0) as u32);
                    }
                }
                if h.dead && !self.dead {
                    add(1.0, 0.6, 450);
                }
                self.last_life = Some(life);
                self.dead = h.dead;
            }
            None => {
                self.last_life = None;
                self.dead = false;
                self.last_potions = None;
                self.last_level = None;
            }
        }
        // The rarest item the hero itself picked up this frame.
        let pickup = now
            .game
            .map(|g| {
                now.events
                    .iter()
                    .filter_map(|e| match e {
                        SimEvent::Pickup { pos, rarity } if picked_by_hero(g, now.hero, *pos) => Some(*rarity),
                        _ => None,
                    })
                    .max()
            })
            .unwrap_or_default();
        if levelled {
            add(0.3, 0.7, 220);
        }
        if pickup.is_some_and(|r| r >= 3) {
            add(0.25, 0.8, 140);
        }
        if drank {
            add(0.0, 0.25, 60);
        }
        if hero.is_some() {
            for e in now.events {
                match e {
                    SimEvent::Strike { crit: true, pos, .. } if near(now.hero, *pos, 8.0) => add(0.0, 0.35, 45),
                    SimEvent::Slam { pos, radius } if near(now.hero, *pos, radius + 6.0) => add(0.8, 0.5, 160),
                    SimEvent::Blast { pos, .. } if near(now.hero, *pos, 10.0) => add(0.55, 0.55, 120),
                    SimEvent::Explosion { pos, radius } if near(now.hero, *pos, radius + 8.0) => add(0.7, 0.6, 150),
                    _ => {}
                }
            }
        }
        // At a car's wheel: a short kick each time the automatic gearbox shifts.
        if now.gear.is_some() && self.last_gear.is_some() && now.gear != self.last_gear {
            add(0.2, 0.5, 40);
        }
        self.last_gear = now.gear;
        if now.speaker > 0.0 && hero.is_some() {
            // One cue a frame: the rarest thing that happened.
            if let Some(c) = speaker_cue(drank, levelled, pickup) {
                play_speaker(c, now.speaker);
            }
        }
        if let Some((strong, weak, ms)) = want.filter(|_| now.pad && now.vibration) {
            // A rumble already running and as strong wins: short, distinct pulses say more.
            let s = strong.max(weak);
            if self.clock >= self.until || s > self.strength + 0.1 {
                self.until = self.clock + ms as f64 / 1000.0;
                self.strength = s;
                self.rumble(strong, weak, ms, gilrs);
            }
        }
        let ds = now.hub.map(|h| &h.dualsense).filter(|d| d.connected);
        // The light bar: the hero's life (green, amber, red, a red pulse near death); the hub's
        // own glow outside the game.
        let light = match (ds.filter(|d| d.lightbar), hero) {
            (Some(_), Some(h)) if h.dead => "#300000",
            (Some(_), Some(h)) => {
                let life = h.life / h.life_max.max(1.0);
                if life > 0.6 {
                    "#20c060"
                } else if life > 0.3 {
                    "#ffb020"
                } else if life > 0.15 || (self.clock * 3.0).fract() < 0.5 {
                    "#ff2a14"
                } else {
                    "#401006"
                }
            }
            _ => "",
        };
        if light != self.light {
            self.light = light.to_string();
            set_light(light);
        }
        // R2 (the sixth skill): a light click when its skill is ready, stiff while it can't be cast.
        let trigger = match (ds.filter(|d| d.triggers), hero) {
            (Some(_), Some(h)) if now.pad && !now.menus && !h.dead => match h.slots.get(5) {
                Some(s) if s.cooldown > 0.05 || !s.affordable => r#"{"kind":"resist","from":0.1,"strength":0.85}"#,
                Some(s) if !s.key.is_empty() => r#"{"kind":"weapon","from":0.25,"to":0.5,"strength":0.45}"#,
                _ => "",
            },
            _ => "",
        };
        if trigger != self.trigger {
            self.trigger = trigger.to_string();
            set_trigger("right", trigger);
        }
    }

    #[allow(unused_variables)]
    fn rumble(&mut self, strong: f32, weak: f32, ms: u32, gilrs: Option<&mut gilrs::Gilrs>) {
        #[cfg(target_arch = "wasm32")]
        js::rumble(strong as f64, weak as f64, ms);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(g) = gilrs {
            use gilrs::ff::{BaseEffect, BaseEffectType, EffectBuilder, Replay, Ticks};
            let ids: Vec<_> = g.gamepads().filter(|(_, p)| p.is_ff_supported()).map(|(id, _)| id).collect();
            if ids.is_empty() {
                return;
            }
            let replay = Replay { play_for: Ticks::from_ms(ms), ..Default::default() };
            let effect = EffectBuilder::new()
                .add_effect(BaseEffect {
                    kind: BaseEffectType::Strong { magnitude: (strong.clamp(0.0, 1.0) * 65535.0) as u16 },
                    scheduling: replay,
                    envelope: Default::default(),
                })
                .add_effect(BaseEffect {
                    kind: BaseEffectType::Weak { magnitude: (weak.clamp(0.0, 1.0) * 65535.0) as u16 },
                    scheduling: replay,
                    envelope: Default::default(),
                })
                .gamepads(&ids)
                .finish(g);
            // The effect plays while it is kept: the last one is.
            if let Ok(e) = effect {
                if e.play().is_ok() {
                    self.effect = Some(e);
                }
            }
        }
    }

    /// The game is going away (a round ended, the page closes): the light and the trigger
    /// go back to the hub.
    pub fn release(&mut self) {
        if !self.light.is_empty() {
            self.light.clear();
            set_light("");
        }
        if !self.trigger.is_empty() {
            self.trigger.clear();
            set_trigger("right", "");
        }
    }
}

fn set_light(color: &str) {
    #[cfg(target_arch = "wasm32")]
    js::light(color);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = color;
}

fn play_speaker(cue: &str, volume: f32) {
    #[cfg(target_arch = "wasm32")]
    js::speaker(cue, volume as f64);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (cue, volume);
}

fn set_trigger(which: &str, effect: &str) {
    #[cfg(target_arch = "wasm32")]
    js::trigger(which, effect);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (which, effect);
}

/// What the page reads of a controller that gilrs doesn't: the touchpad's press, and a swipe
/// on it (52in1's DualSense touches). Nothing natively.
pub fn read_extra() -> PadExtra {
    #[cfg(target_arch = "wasm32")]
    {
        let n = js::pad_extra();
        // Bit 0: the touchpad was pressed; bits 1-2: a swipe left (1) or right (2).
        PadExtra {
            touchpad_pressed: n & 1 != 0,
            swipe: match (n >> 1) & 3 {
                1 => -1,
                2 => 1,
                _ => 0,
            },
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    PadExtra::default()
}

#[cfg(target_arch = "wasm32")]
mod js {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn rumble(strong: f64, weak: f64, ms: u32);
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn light(color: &str);
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn trigger(which: &str, effect: &str);
        #[wasm_bindgen(js_namespace = shardfallHub, js_name = padExtra)]
        pub fn pad_extra() -> u32;
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn speaker(cue: &str, volume: f64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaker_gating_and_cues() {
        let mut h = crate::hub::Settings::default();
        assert!(!speaker_on(None, true));
        assert!(!speaker_on(Some(&h), true));
        h.dualsense.speaker = true;
        assert!(!speaker_on(Some(&h), true), "no DualSense connected");
        h.dualsense.connected = true;
        assert!(speaker_on(Some(&h), true) && !speaker_on(Some(&h), false));
        h.dualsense.speaker = false;
        assert!(!speaker_on(Some(&h), true) && !speaker_allowed(Some(&h)));

        // The rarest of what happened to the hero itself.
        assert_eq!(speaker_cue(true, false, None), Some("potion"));
        assert_eq!(speaker_cue(true, true, None), Some("level"));
        assert_eq!(speaker_cue(true, true, Some(2)), Some("level"));
        assert_eq!(speaker_cue(true, false, Some(2)), Some("rare"));
        assert_eq!(speaker_cue(true, true, Some(3)), Some("unique"));
        assert_eq!(speaker_cue(false, false, Some(1)), None);
        assert_eq!(speaker_cue(false, false, None), None);
    }
}
