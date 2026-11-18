//! Shout to roar, the game's voice feature (52in1 catalogue v1, "The DualSense", the
//! microphone): while it is on, a loud shout into the microphone casts the hero's War Cry, as a
//! press of its button on the skill bar does, when War Cry is on the bar and ready (otherwise
//! nothing happens). The button works as it always did, and the game plays the same without
//! the microphone: it is an extra.
//!
//! - Only in 52in1, once the player has allowed games the microphone there (`hub.mic`, 52in1's
//!   Settings, under Sound). Until then the game's Settings show it off, greyed, with a line
//!   saying where 52in1 allows the microphone (while `micLater`, allowed after the game
//!   opened: to open the game again instead), and the game never asks for it. Once allowed it
//!   starts on (`Prefs::shout_roar` None).
//! - Any controller, or the keyboard: the hub gives the DualSense's microphone when one is
//!   plugged in by USB and the device's own otherwise.
//! - The page listens (web/index.html, `shardfallHub.shout`): it asks the hub's
//!   `openMicrophone()` once while the setting is on, never `getUserMedia` itself; its ear
//!   (`window.shoutDetector`, checked by scripts/shout_check.mjs) learns the microphone's
//!   background and hears a shout as loudness (RMS) 2.6 times over it held for 0.25 s, then
//!   1.5 s pass before the next one counts. While `muted` (the DualSense's mute button) none
//!   counts. Turned off, or the microphone taken back, the page stops the stream.
//! - Natively there is no hub: nothing here runs.

use pav_core::arpg::HeroHud;
use pav_core::input::buttons;

/// The skill a shout casts (game/skills.toml).
pub const SKILL: &str = "war_cry";

/// The skill bar's buttons, slot by slot (what the pad, the keys and the mouse press).
const SLOT_BUTTONS: [u32; 6] =
    [buttons::PRIMARY, buttons::SECONDARY, buttons::SKILL3, buttons::SKILL4, buttons::SKILL5, buttons::SKILL6];

/// How long a shout's press waits to see War Cry go off (its cooldown start), in seconds.
const WAIT: f32 = 1.5;
/// For this long after a shout War Cry's button is pressed again each frame until it goes off:
/// a swing already under way takes a press only near its end.
const REPRESS: f32 = 0.7;

/// What the player's 52in1 settings say of the microphone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mic {
    /// Allowed (before the game opened): the game may ask the hub for it.
    Allowed,
    /// Allowed after the game opened: the game hears it once it is opened again.
    Later,
    /// Not allowed, or no hub.
    Off,
}

pub fn mic(hub: Option<&crate::hub::Settings>) -> Mic {
    match hub {
        Some(h) if h.mic => Mic::Allowed,
        Some(h) if h.mic_later => Mic::Later,
        _ => Mic::Off,
    }
}

/// Shout to roar is on (and the page holds the microphone): 52in1 allows it, and the game's
/// setting is on (until the player picks, on).
pub fn on(hub: Option<&crate::hub::Settings>, pref: Option<bool>) -> bool {
    mic(hub) == Mic::Allowed && pref.unwrap_or(true)
}

/// A shout counts: on, and the microphone not muted (the DualSense's mute button).
pub fn listening(hub: Option<&crate::hub::Settings>, pref: Option<bool>) -> bool {
    on(hub, pref) && !hub.is_some_and(|h| h.muted)
}

/// War Cry's slot on the hero's bar, when a press there casts it now: the hero alive, War Cry
/// off cooldown and affordable.
pub fn roar_slot(hero: &HeroHud) -> Option<usize> {
    if hero.dead {
        return None;
    }
    let slot = hero.slots.iter().take(SLOT_BUTTONS.len()).position(|s| s.key == SKILL)?;
    let s = &hero.slots[slot];
    // (Blood Magic has no mana, `mana_max` 0, and pays in life: the bar's "affordable" reads
    // mana, so it would say no where the button casts.)
    let affordable = s.affordable || hero.mana_max <= 0.0;
    (s.cooldown <= 0.0 && affordable).then_some(slot)
}

/// Shout to roar between frames: a shout's press, waiting to see War Cry go off.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Roar {
    /// The slot pressed, and the seconds left to see its cooldown start.
    pressed: Option<(usize, f32)>,
}

impl Roar {
    /// One frame: `shout` (a shout heard while the player can play), the hero's HUD and the
    /// frame's seconds. Returns the button to press this frame (0 for none, else War Cry's
    /// slot's: on the shout's frame, and again each frame for `REPRESS` seconds while it can
    /// still be cast and hasn't gone off) and whether War Cry went off after the last press
    /// (the HUD's "Roar!").
    pub fn step(&mut self, shout: bool, hero: Option<&HeroHud>, secs: f32) -> (u32, bool) {
        let mut roared = false;
        let mut press = 0;
        if let Some((slot, left)) = self.pressed.take() {
            match hero.and_then(|h| h.slots.get(slot)).filter(|s| s.key == SKILL) {
                Some(s) if s.cooldown > 0.0 => roared = true,
                Some(_) if left > secs => {
                    let left = left - secs;
                    self.pressed = Some((slot, left));
                    if WAIT - left < REPRESS && hero.and_then(roar_slot) == Some(slot) {
                        press = SLOT_BUTTONS[slot];
                    }
                }
                _ => {}
            }
        }
        if shout && self.pressed.is_none() {
            if let Some(slot) = hero.and_then(roar_slot) {
                press = SLOT_BUTTONS[slot];
                self.pressed = Some((slot, WAIT));
            }
        }
        (press, roared)
    }
}

/// Tells the page whether Shout to roar is on (it asks for the microphone the first time and
/// lets it go when off) and takes the shout it heard since the last call. False natively.
pub fn take_shout(on: bool) -> bool {
    #[cfg(target_arch = "wasm32")]
    return js::shout(on);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = on;
        false
    }
}

/// A shout cast War Cry: the page counts them (`shardfallHub.roars`, for checks).
pub fn roared() {
    #[cfg(target_arch = "wasm32")]
    js::roared();
}

#[cfg(target_arch = "wasm32")]
mod js {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn shout(on: bool) -> bool;
        #[wasm_bindgen(js_namespace = shardfallHub)]
        pub fn roared();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pav_core::arpg::SlotHud;

    fn hub(mic: bool, later: bool, muted: bool) -> crate::hub::Settings {
        crate::hub::Settings { mic, mic_later: later, muted, ..Default::default() }
    }

    #[test]
    fn gating_truth_table() {
        // No hub (a native build, the game's own page): never, whatever the setting.
        for pref in [None, Some(true), Some(false)] {
            assert!(!on(None, pref) && !listening(None, pref));
        }
        assert_eq!(mic(None), Mic::Off);
        // (mic, micLater, muted, the game's setting) -> (what the row says, on, listening).
        let table = [
            (true, false, false, None, Mic::Allowed, true, true),
            (true, false, false, Some(true), Mic::Allowed, true, true),
            (true, false, false, Some(false), Mic::Allowed, false, false),
            (true, false, true, None, Mic::Allowed, true, false),
            (true, false, true, Some(false), Mic::Allowed, false, false),
            // Allowed after the game opened: off until it is opened again.
            (false, true, false, None, Mic::Later, false, false),
            (false, true, false, Some(true), Mic::Later, false, false),
            // Not allowed: off, whatever the game's setting says.
            (false, false, false, None, Mic::Off, false, false),
            (false, false, false, Some(true), Mic::Off, false, false),
            (false, false, true, Some(true), Mic::Off, false, false),
            // `mic` says it is allowed now, whatever else.
            (true, true, false, None, Mic::Allowed, true, true),
        ];
        for (m, later, muted, pref, row, want_on, want_listen) in table {
            let h = hub(m, later, muted);
            let at = format!("mic {m} micLater {later} muted {muted} pref {pref:?}");
            assert_eq!(mic(Some(&h)), row, "{at}");
            assert_eq!(on(Some(&h), pref), want_on, "{at}");
            assert_eq!(listening(Some(&h), pref), want_listen, "{at}");
        }
    }

    fn slot(key: &str, cooldown: f32, affordable: bool) -> SlotHud {
        SlotHud { key: key.into(), cooldown, affordable, ..Default::default() }
    }

    fn hero(slots: Vec<SlotHud>) -> HeroHud {
        HeroHud { slots, mana_max: 50.0, ..Default::default() }
    }

    #[test]
    fn the_war_cry_slot() {
        // Kestrel's bar: War Cry is the fifth button.
        let keys = ["slash", "cleave", "leap_slam", "blade_dash", "war_cry", "earthsplitter"];
        let mut h = hero(keys.iter().map(|k| slot(k, 0.0, true)).collect());
        assert_eq!(roar_slot(&h), Some(4));
        // On cooldown, or too dear: not now.
        h.slots[4].cooldown = 3.0;
        assert_eq!(roar_slot(&h), None);
        h.slots[4].cooldown = 0.0;
        h.slots[4].affordable = false;
        assert_eq!(roar_slot(&h), None);
        h.slots[4].affordable = true;
        // Dead: nothing.
        h.dead = true;
        assert_eq!(roar_slot(&h), None);
        h.dead = false;
        // Not on the bar (the Wanderer's): nothing.
        let wanderer = ["slash", "fireball", "cleave", "leap_slam", "frost_nova", "blade_dash"];
        assert_eq!(roar_slot(&hero(wanderer.iter().map(|k| slot(k, 0.0, true)).collect())), None);
        // On the first button: the first's.
        let first = hero(vec![slot("war_cry", 0.0, true), slot("slash", 0.0, true)]);
        assert_eq!(roar_slot(&first), Some(0));
        assert_eq!(roar_slot(&hero(Vec::new())), None);
    }

    #[test]
    fn a_shout_presses_war_cry_once_and_says_roar_when_it_goes_off() {
        let keys = ["slash", "cleave", "leap_slam", "blade_dash", "war_cry", "earthsplitter"];
        let mut h = hero(keys.iter().map(|k| slot(k, 0.0, true)).collect());
        let mut r = Roar::default();
        // No shout: nothing.
        assert_eq!(r.step(false, Some(&h), 0.016), (0, false));
        // A shout: War Cry's button (the fifth: SKILL5), and again each frame while a swing under
        // way keeps it from going off.
        assert_eq!(r.step(true, Some(&h), 0.016), (buttons::SKILL5, false));
        assert_eq!(r.step(false, Some(&h), 0.016), (buttons::SKILL5, false));
        // Another shout while the first waits: the same press, not a second one.
        assert_eq!(r.step(true, Some(&h), 0.016), (buttons::SKILL5, false));
        // War Cry's cooldown starts: it went off.
        h.slots[4].cooldown = 12.0;
        assert_eq!(r.step(false, Some(&h), 0.016), (0, true));
        assert_eq!(r.step(false, Some(&h), 0.016), (0, false));
        // A shout while it cools down: nothing.
        assert_eq!(r.step(true, Some(&h), 0.016), (0, false));
        assert_eq!(r, Roar::default());

        // A press that War Cry doesn't answer (a stagger, a freeze): pressed again for REPRESS
        // seconds, then left alone (the player's own buttons win again), given up after WAIT.
        h.slots[4].cooldown = 0.0;
        assert_eq!(r.step(true, Some(&h), 0.1).0, buttons::SKILL5);
        let (mut frames, mut presses) = (0, 0);
        while r != Roar::default() {
            let (b, roared) = r.step(false, Some(&h), 0.1);
            assert!(!roared);
            presses += (b != 0) as u32;
            frames += 1;
            assert!(frames < 100);
        }
        assert!(frames as f32 * 0.1 >= WAIT - 0.11, "{frames}");
        assert!((5..=7).contains(&presses), "pressed again for REPRESS only: {presses}");
        // It can't be cast any more (mana spent): no more presses.
        h.slots[4].affordable = false;
        h.mana_max = 50.0;
        assert_eq!(r.step(true, Some(&h), 0.1).0, 0);
        h.slots[4].affordable = true;
        // Blood Magic (no mana, pays in life): the button casts, so a shout does.
        h.slots[4].affordable = false;
        h.mana_max = 0.0;
        assert_eq!(roar_slot(&h), Some(4));
        h.slots[4].affordable = true;
        let mut r = Roar::default();
        // Then a shout presses again.
        assert_eq!(r.step(true, Some(&h), 0.1).0, buttons::SKILL5);

        // No hero (the plaza, a menu scene): nothing, and a waiting press is dropped.
        assert_eq!(r.step(true, None, 0.016), (0, false));
        assert_eq!(r, Roar::default());
        // The bar changed under a waiting press: dropped, no "Roar!".
        assert_eq!(r.step(true, Some(&h), 0.016).0, buttons::SKILL5);
        let mut swapped = h.clone();
        swapped.slots[4] = slot("frost_nova", 6.0, true);
        assert_eq!(r.step(false, Some(&swapped), 0.016), (0, false));
        assert_eq!(r, Roar::default());
    }
}
