//! Startup settings: `pavilion.toml` next to the executable, overridden by command-line flags.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "vulkan" (default) or "dx12".
    pub backend: String,
    pub vsync: bool,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    /// Scene or room to start in.
    pub scene: String,
    pub seed: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { backend: "vulkan".into(), vsync: true, width: 1600, height: 900, fullscreen: false, scene: "test".into(), seed: 1 }
    }
}

const HEADER: &str = "# Pavilion startup settings. Delete this file to restore defaults.\n# backend = \"vulkan\" or \"dx12\"\n";

impl Settings {
    /// Loads settings (creating the file with defaults on first run), then applies CLI flags.
    pub fn load(args: &[String]) -> (Self, String) {
        let path = crate::boot::exe_dir().join("pavilion.toml");
        let mut note = String::new();
        let mut s = match std::fs::read_to_string(&path) {
            Ok(text) => match toml_from_str(&text) {
                Ok(s) => {
                    note = format!("{}", path.display());
                    s
                }
                Err(e) => {
                    log::warn!("{} is invalid ({e}); using defaults", path.display());
                    Settings::default()
                }
            },
            Err(_) => {
                let s = Settings::default();
                let _ = std::fs::write(&path, format!("{HEADER}{}", toml_to_string(&s)));
                note = format!("{} (created)", path.display());
                s
            }
        };
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--backend" => s.backend = it.next().cloned().unwrap_or(s.backend),
                "--scene" | "--room" => s.scene = it.next().cloned().unwrap_or(s.scene),
                "--seed" => s.seed = it.next().and_then(|v| v.parse().ok()).unwrap_or(s.seed),
                "--no-vsync" => s.vsync = false,
                "--vsync" => s.vsync = true,
                "--fullscreen" => s.fullscreen = true,
                "--windowed" => s.fullscreen = false,
                _ => {}
            }
        }
        if let Ok(b) = std::env::var("PAV_BACKEND") {
            s.backend = b;
        }
        (s, note)
    }
}

fn toml_from_str(t: &str) -> Result<Settings, String> {
    // Tiny key = value parser (the file is flat), so the app does not need a TOML dependency.
    let mut s = Settings::default();
    for line in t.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim().trim_matches('"'));
        match k {
            "backend" => s.backend = v.into(),
            "vsync" => s.vsync = v == "true",
            "width" => s.width = v.parse().map_err(|_| "bad width")?,
            "height" => s.height = v.parse().map_err(|_| "bad height")?,
            "fullscreen" => s.fullscreen = v == "true",
            "scene" => s.scene = v.into(),
            "seed" => s.seed = v.parse().map_err(|_| "bad seed")?,
            _ => {}
        }
    }
    Ok(s)
}

fn toml_to_string(s: &Settings) -> String {
    format!(
        "backend = \"{}\"\nvsync = {}\nwidth = {}\nheight = {}\nfullscreen = {}\nscene = \"{}\"\nseed = {}\n",
        s.backend, s.vsync, s.width, s.height, s.fullscreen, s.scene, s.seed
    )
}
