//! Shardfall — the game executable.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod arpg_items;
mod arpg_tree;
mod arpg_ui;
mod boot;
#[cfg(not(target_arch = "wasm32"))]
mod bridge;
mod edit;
mod gfx;
mod guide_ui;
mod hud;
mod input;
mod look_ui;
mod panel;
mod platform;
mod rooms;
mod save;
mod settings;
mod simhost;
mod ui;
mod uiinput;

fn fatal(msg: &str) -> ! {
    let path = boot::diag().log_path.display().to_string();
    log::error!("fatal: {msg}");
    platform::error_box("Shardfall could not start", &format!("{msg}\n\nThe full log is in:\n{path}"));
    std::process::exit(1);
}

fn main() {
    platform::attach_console();
    let d = boot::init();
    boot::install_panic_hook();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "shardfall [--room NAME | --scene NAME] [--seed N] [--backend vulkan|dx12] [--no-vsync] [--fullscreen] [--bridge [ADDR]]\n\
             --bridge opens the live agent bridge (default 127.0.0.1:7878; drive it with `pav live`).\n\
             Startup settings live in shardfall.toml next to the executable."
        );
        return;
    }
    log::info!(
        "Shardfall {} ({} {}, {} build) — log file: {}",
        env!("CARGO_PKG_VERSION"),
        if cfg!(target_arch = "wasm32") { "browser" } else { std::env::consts::OS },
        std::env::consts::ARCH,
        if cfg!(debug_assertions) { "debug" } else { "release" },
        d.log_path.display()
    );
    let settings = match boot::stage("settings", || {
        let (s, note) = settings::Settings::load(&args);
        Ok((s, note))
    }) {
        Ok(s) => s,
        Err(e) => fatal(&format!("{e:#}")),
    };
    // The browser's event loop runs on after `run` returns; errors show on the page.
    #[cfg(target_arch = "wasm32")]
    if let Err(e) = app::run(settings) {
        fatal(&format!("{e:#}"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    match std::panic::catch_unwind(|| app::run(settings)) {
        Ok(Ok(())) => log::info!("clean exit"),
        Ok(Err(e)) => fatal(&format!("{e:#}")),
        Err(_) => {
            let msg = boot::LAST_CRASH.lock().ok().and_then(|c| c.clone()).unwrap_or_else(|| "unknown crash".into());
            fatal(&format!("Shardfall crashed:\n{msg}"));
        }
    }
}
