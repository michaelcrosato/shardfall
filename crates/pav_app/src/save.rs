//! Shardfall saves: the hero (level, gear, bags, stash, passives, potions, waypoints) and the
//! heroes of the other playable characters waiting in the Hall of Heroes, as JSON, next to the
//! executable (`shardfall_save.json`) or in the browser's local storage. Written on every
//! travel, every minute and on quit; read whenever a game scene starts.

use std::collections::BTreeMap;

use pav_core::arpg::hero::Hero;

#[derive(serde::Serialize, serde::Deserialize)]
struct SaveFile {
    version: u32,
    hero: Hero,
    /// The other characters' heroes, by character (saves before there were characters have none).
    #[serde(default)]
    roster: BTreeMap<String, Hero>,
}

#[cfg(not(target_arch = "wasm32"))]
fn path() -> std::path::PathBuf {
    crate::boot::exe_dir().join("shardfall_save.json")
}

#[cfg(target_arch = "wasm32")]
fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(target_arch = "wasm32")]
const KEY: &str = "shardfall_save";

fn read_text() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    return std::fs::read_to_string(path()).ok();
    #[cfg(target_arch = "wasm32")]
    return storage()?.get_item(KEY).ok()?;
}

/// The saved hero and the others waiting in the hall, if there is a save (and it reads).
pub fn load() -> Option<(Hero, BTreeMap<String, Hero>)> {
    let text = read_text()?;
    match serde_json::from_str::<SaveFile>(&text) {
        Ok(f) => Some((f.hero, f.roster)),
        Err(e) => {
            log::warn!("save unreadable ({e}); starting a new hero");
            None
        }
    }
}

/// Writes the hero and the others waiting in the hall (on disk atomically: a temp file renamed
/// over the old save).
pub fn write(hero: &Hero, roster: &BTreeMap<String, Hero>) {
    let f = SaveFile { version: 1, hero: hero.clone(), roster: roster.clone() };
    let Ok(text) = serde_json::to_string(&f) else { return };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let p = path();
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &p)).is_err() {
            log::warn!("could not write {}", p.display());
        }
    }
    #[cfg(target_arch = "wasm32")]
    if storage().and_then(|s| s.set_item(KEY, &text).ok()).is_none() {
        log::warn!("could not save to local storage");
    }
}

/// Forgets the hero (a new one starts).
pub fn erase() {
    #[cfg(not(target_arch = "wasm32"))]
    let _ = std::fs::remove_file(path());
    #[cfg(target_arch = "wasm32")]
    if let Some(s) = storage() {
        let _ = s.remove_item(KEY);
    }
}

/// Saves the running game's hero from the simulation thread.
pub fn save_from(sim: &pav_core::Sim) {
    if let Some(g) = sim.state.game.as_ref() {
        write(&g.hero, &g.roster);
    }
}

/// Puts the saved hero into a freshly built game scene.
pub fn restore_into(sim: &mut pav_core::Sim) -> Option<u32> {
    sim.state.game.as_ref()?;
    let (hero, roster) = load()?;
    let level = hero.level;
    sim.load_hero(hero).then(|| sim.load_roster(roster)).map(|_| level)
}

/// Preferences that outlive a session: the touch controls and the graphics quality.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Touch screens: the first skill strikes the nearest foe by itself.
    pub auto_attack: bool,
    pub quality: crate::quality::Quality,
    /// The touch tips were swiped away (they do not come back).
    pub tips_seen: bool,
    /// The language chosen in the Language menu (native builds; the browser keeps it under
    /// `lang:` and the page's path, as AI Hub Languages v1 asks).
    pub lang: Option<String>,
    /// Controller prompts: PlayStation's symbols (the default) or Xbox's letters.
    pub pad_style: pav_ui::glyph::PadStyle,
    /// The controller rumbles (52in1 scales it by the player's own vibration setting too).
    pub vibration: bool,
    /// The game's own sound effects volume, 0 to 1 (52in1's effects volume multiplies it).
    pub effects: f32,
    /// How big menus and the HUD are, on top of the screen's own scale: 0.85, 1 or 1.2.
    pub ui_size: f32,
    /// A DualSense in 52in1: the game's own Motion aim (turning the controller turns the aim).
    /// None until the player picks: on when their 52in1 Motion aim is (`motion::chosen`).
    pub motion_aim: Option<bool>,
    pub motion_sensitivity: crate::motion::Sensitivity,
    /// A DualSense in 52in1: short cues from the controller's speaker, where 52in1 allows it.
    pub pad_speaker: bool,
    /// In 52in1: Shout to roar (a shout into the microphone casts War Cry). None until the
    /// player picks: on once 52in1 allows games the microphone (`voice::on`).
    pub shout_roar: Option<bool>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            auto_attack: true,
            quality: Default::default(),
            tips_seen: false,
            lang: None,
            pad_style: Default::default(),
            vibration: true,
            effects: 1.0,
            ui_size: 1.0,
            motion_aim: None,
            motion_sensitivity: Default::default(),
            pad_speaker: true,
            shout_roar: None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn prefs_path() -> std::path::PathBuf {
    crate::boot::exe_dir().join("shardfall_prefs.json")
}

#[cfg(target_arch = "wasm32")]
const PREFS_KEY: &str = "shardfall_prefs";

pub fn load_prefs() -> Prefs {
    #[cfg(not(target_arch = "wasm32"))]
    let text = std::fs::read_to_string(prefs_path()).ok();
    #[cfg(target_arch = "wasm32")]
    let text = storage().and_then(|s| s.get_item(PREFS_KEY).ok().flatten());
    text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_prefs(p: &Prefs) {
    let Ok(text) = serde_json::to_string(p) else { return };
    #[cfg(not(target_arch = "wasm32"))]
    if std::fs::write(prefs_path(), text).is_err() {
        log::warn!("could not write {}", prefs_path().display());
    }
    #[cfg(target_arch = "wasm32")]
    if storage().and_then(|s| s.set_item(PREFS_KEY, &text).ok()).is_none() {
        log::warn!("could not save preferences");
    }
}
