//! Pavilion — the game executable.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod boot;
mod gfx;
mod platform;
mod settings;
mod simhost;
mod ui;

fn fatal(msg: &str) -> ! {
    let path = boot::diag().log_path.display().to_string();
    log::error!("fatal: {msg}");
    platform::error_box("Pavilion could not start", &format!("{msg}\n\nThe full log is in:\n{path}"));
    std::process::exit(1);
}

fn main() {
    platform::attach_console();
    let d = boot::init();
    boot::install_panic_hook();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "pavilion [--scene NAME] [--seed N] [--backend vulkan|dx12] [--no-vsync] [--fullscreen]\n\
             Startup settings live in pavilion.toml next to the executable."
        );
        return;
    }
    log::info!(
        "Pavilion {} ({} {}, {} build) — log file: {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
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
    match std::panic::catch_unwind(|| app::run(settings)) {
        Ok(Ok(())) => log::info!("clean exit"),
        Ok(Err(e)) => fatal(&format!("{e:#}")),
        Err(_) => {
            let msg = boot::LAST_CRASH.lock().ok().and_then(|c| c.clone()).unwrap_or_else(|| "unknown crash".into());
            fatal(&format!("Pavilion crashed:\n{msg}"));
        }
    }
}
