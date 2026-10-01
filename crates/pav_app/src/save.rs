//! Shardfall saves: the hero (level, gear, bags, stash, passives, potions, waypoints) as JSON
//! next to the executable. Written on every travel, every minute and on quit; read whenever a
//! game scene starts. The browser build keeps no save (yet).

use pav_core::arpg::hero::Hero;

#[cfg(not(target_arch = "wasm32"))]
fn path() -> std::path::PathBuf {
    crate::boot::exe_dir().join("shardfall_save.json")
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SaveFile {
    version: u32,
    hero: Hero,
}

/// The saved hero, if there is one (and it reads).
pub fn load() -> Option<Hero> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let text = std::fs::read_to_string(path()).ok()?;
        match serde_json::from_str::<SaveFile>(&text) {
            Ok(f) => Some(f.hero),
            Err(e) => {
                log::warn!("save file unreadable ({e}); starting a new hero");
                None
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    None
}

/// Writes the hero (atomically: a temp file renamed over the old save).
pub fn write(hero: &Hero) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let f = SaveFile { version: 1, hero: hero.clone() };
        let Ok(text) = serde_json::to_string(&f) else { return };
        let p = path();
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &p)).is_err() {
            log::warn!("could not write {}", p.display());
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = hero;
}

/// Forgets the hero (a new one starts).
pub fn erase() {
    #[cfg(not(target_arch = "wasm32"))]
    let _ = std::fs::remove_file(path());
}

/// Saves the running game's hero from the simulation thread.
pub fn save_from(sim: &pav_core::Sim) {
    if let Some(g) = sim.state.game.as_ref() {
        write(&g.hero);
    }
}

/// Puts the saved hero into a freshly built game scene.
pub fn restore_into(sim: &mut pav_core::Sim) -> Option<u32> {
    if sim.state.game.is_none() {
        return None;
    }
    let hero = load()?;
    let level = hero.level;
    sim.load_hero(hero).then_some(level)
}
