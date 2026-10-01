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
    /// Scene to start in (empty = Shardfall's town; `world` = the pavilion). Command line:
    /// --scene NAME or --room NAME.
    pub scene: String,
    pub seed: u64,
    /// Live agent bridge address ("" = off). Command line: --bridge [ADDR].
    pub bridge: String,
    /// A scripted gamepad to play (command line only: --pad-script FILE).
    #[serde(skip)]
    pub pad_script: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            backend: "vulkan".into(),
            vsync: true,
            width: 1600,
            height: 900,
            fullscreen: false,
            scene: String::new(),
            seed: 1,
            bridge: String::new(),
            pad_script: String::new(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
const HEADER: &str = "# Pavilion startup settings. Delete this file to restore defaults.\n# backend = \"vulkan\" or \"dx12\"\n";

impl Settings {
    /// In the browser: defaults, plus `?room=NAME&seed=N` from the page address.
    #[cfg(target_arch = "wasm32")]
    pub fn load(_args: &[String]) -> (Self, String) {
        let mut s = Settings::default();
        let query = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
        if let Ok(p) = web_sys::UrlSearchParams::new_with_str(&query) {
            if let Some(r) = p.get("room").filter(|r| !r.is_empty()) {
                s.scene = format!("world/{r}");
            }
            if let Some(r) = p.get("scene").filter(|r| !r.is_empty()) {
                s.scene = r;
            }
            if let Some(v) = p.get("seed").and_then(|v| v.parse().ok()) {
                s.seed = v;
            }
        }
        if s.scene.is_empty() {
            // Shardfall's town; the engine's pavilion is the `world` scene (pause menu).
            s.scene = "town".into();
        }
        (s, "page address (?room=NAME&seed=N)".into())
    }

    /// Loads settings (creating the file with defaults on first run), then applies CLI flags.
    #[cfg(not(target_arch = "wasm32"))]
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
        let mut it = args.iter().peekable();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--bridge" => {
                    s.bridge = match it.peek() {
                        Some(v) if !v.starts_with("--") => it.next().cloned().unwrap_or_default(),
                        _ => pav_tools::bridge::DEFAULT_ADDR.into(),
                    }
                }
                "--backend" => s.backend = it.next().cloned().unwrap_or(s.backend),
                "--scene" => s.scene = it.next().cloned().unwrap_or(s.scene),
                "--room" => s.scene = it.next().map(|r| format!("world/{r}")).unwrap_or(s.scene),
                "--seed" => s.seed = it.next().and_then(|v| v.parse().ok()).unwrap_or(s.seed),
                "--pad-script" => s.pad_script = it.next().cloned().unwrap_or_default(),
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
        if s.scene.is_empty() {
            // Shardfall's town; the engine's pavilion is the `world` scene (pause menu).
            s.scene = "town".into();
        }
        (s, note)
    }
}

#[cfg(not(target_arch = "wasm32"))]
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
            "start_room" if !v.is_empty() => s.scene = format!("world/{v}"),
            "seed" => s.seed = v.parse().map_err(|_| "bad seed")?,
            "bridge" => s.bridge = v.into(),
            _ => {}
        }
    }
    Ok(s)
}

#[cfg(not(target_arch = "wasm32"))]
fn toml_to_string(s: &Settings) -> String {
    format!(
        "backend = \"{}\"\nvsync = {}\nwidth = {}\nheight = {}\nfullscreen = {}\n# start_room = \"playground\"\nseed = {}\n# Live agent bridge (pav live / pav mcp --live), e.g. \"127.0.0.1:7878\"\nbridge = \"{}\"\n",
        s.backend, s.vsync, s.width, s.height, s.fullscreen, s.seed, s.bridge
    )
}
