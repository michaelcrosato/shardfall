//! Shardfall saves: the hero (level, gear, bags, stash, passives, potions, waypoints) as JSON,
//! next to the executable (`shardfall_save.json`) or in the browser's local storage. Written
//! on every travel, every minute and on quit; read whenever a game scene starts.

use pav_core::arpg::hero::Hero;

#[derive(serde::Serialize, serde::Deserialize)]
struct SaveFile {
    version: u32,
    hero: Hero,
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

/// The saved hero, if there is one (and it reads).
pub fn load() -> Option<Hero> {
    let text = read_text()?;
    match serde_json::from_str::<SaveFile>(&text) {
        Ok(f) => Some(f.hero),
        Err(e) => {
            log::warn!("save unreadable ({e}); starting a new hero");
            None
        }
    }
}

/// Writes the hero (on disk atomically: a temp file renamed over the old save).
pub fn write(hero: &Hero) {
    let f = SaveFile { version: 1, hero: hero.clone() };
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
        write(&g.hero);
    }
}

/// Puts the saved hero into a freshly built game scene.
pub fn restore_into(sim: &mut pav_core::Sim) -> Option<u32> {
    sim.state.game.as_ref()?;
    let hero = load()?;
    let level = hero.level;
    sim.load_hero(hero).then_some(level)
}
