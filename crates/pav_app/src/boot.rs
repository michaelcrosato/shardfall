//! Boot diagnostics and logging. Every startup stage is numbered and timed, and goes to the
//! terminal, the log file and the on-screen overlay. The logger also keeps recent lines for
//! the in-game console.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use web_time::Instant;

use anyhow::Result;

#[derive(Clone, Debug)]
pub enum StageStatus {
    Ok,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct Stage {
    pub num: u32,
    pub name: String,
    pub detail: String,
    pub ms: f64,
    pub status: StageStatus,
}

pub struct Diagnostics {
    pub start: Instant,
    pub log_path: PathBuf,
    file: Mutex<Option<File>>,
    pub stages: Mutex<Vec<Stage>>,
    pub recent: Mutex<VecDeque<(log::Level, String)>>,
    level: log::LevelFilter,
}

static DIAG: OnceLock<Diagnostics> = OnceLock::new();

pub fn diag() -> &'static Diagnostics {
    DIAG.get().expect("diagnostics not initialised")
}

/// Directory next to the executable (falls back to the working directory).
pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| PathBuf::from("."))
}

/// Sets up the log file (next to the executable, else the temp dir) and the logger.
pub fn init() -> &'static Diagnostics {
    #[cfg(not(target_arch = "wasm32"))]
    let (log_path, file) = {
        let mut log_path = exe_dir().join("pavilion.log");
        let file = File::create(&log_path).ok().or_else(|| {
            log_path = std::env::temp_dir().join("pavilion.log");
            File::create(&log_path).ok()
        });
        (log_path, file)
    };
    // In the browser the log goes to the developer console.
    #[cfg(target_arch = "wasm32")]
    let (log_path, file) = (PathBuf::from("the browser console"), None::<File>);
    let level = match std::env::var("PAV_LOG").ok().as_deref() {
        Some("trace") => log::LevelFilter::Trace,
        Some("debug") => log::LevelFilter::Debug,
        Some("warn") => log::LevelFilter::Warn,
        _ => log::LevelFilter::Info,
    };
    let d = DIAG.get_or_init(|| Diagnostics {
        start: Instant::now(),
        log_path,
        file: Mutex::new(file),
        stages: Mutex::new(Vec::new()),
        recent: Mutex::new(VecDeque::new()),
        level,
    });
    let _ = log::set_logger(&Logger);
    log::set_max_level(log::LevelFilter::Trace);
    d
}

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        let d = diag();
        let t = m.target();
        // Keep graphics-driver chatter out unless it is a warning.
        if t.starts_with("wgpu") || t.starts_with("naga") || t.starts_with("egui") || t.starts_with("winit") {
            return m.level() <= log::Level::Warn;
        }
        m.level() <= d.level
    }

    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let d = diag();
        let line = format!("[{:>8.3}s] {:<5} {}", d.start.elapsed().as_secs_f64(), r.level(), r.args());
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("{line}");
        #[cfg(target_arch = "wasm32")]
        {
            let js = wasm_bindgen::JsValue::from_str(&line);
            match r.level() {
                log::Level::Error => web_sys::console::error_1(&js),
                log::Level::Warn => web_sys::console::warn_1(&js),
                _ => web_sys::console::log_1(&js),
            }
        }
        if let Ok(mut f) = d.file.lock() {
            if let Some(f) = f.as_mut() {
                let _ = writeln!(f, "{line}");
                let _ = f.flush();
            }
        }
        if let Ok(mut q) = d.recent.lock() {
            q.push_back((r.level(), format!("{}", r.args())));
            while q.len() > 400 {
                q.pop_front();
            }
        }
    }

    fn flush(&self) {}
}

/// Runs one numbered boot stage, recording its duration and outcome.
pub fn stage<T>(name: &str, f: impl FnOnce() -> Result<(T, String)>) -> Result<T> {
    let t = Instant::now();
    let r = f();
    record(name, t, r)
}

/// `stage` for a step that has to wait (GPU setup in the browser).
pub async fn stage_async<T>(name: &str, f: impl std::future::Future<Output = Result<(T, String)>>) -> Result<T> {
    let t = Instant::now();
    let r = f.await;
    record(name, t, r)
}

fn record<T>(name: &str, t: Instant, r: Result<(T, String)>) -> Result<T> {
    let d = diag();
    let num = d.stages.lock().map(|s| s.len() as u32 + 1).unwrap_or(0);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let (status, detail, out) = match r {
        Ok((v, detail)) => (StageStatus::Ok, detail, Ok(v)),
        Err(e) => {
            let msg = format!("{e:#}");
            (StageStatus::Failed(msg.clone()), String::new(), Err(e))
        }
    };
    match &status {
        StageStatus::Ok => log::info!("[boot {num:02}] {name:<22} ok   {ms:>8.1} ms  {detail}"),
        StageStatus::Failed(m) => log::error!("[boot {num:02}] {name:<22} FAIL {ms:>8.1} ms  {m}"),
    }
    if let Ok(mut s) = d.stages.lock() {
        s.push(Stage { num, name: name.into(), detail, ms, status });
    }
    out
}

/// Installs a panic hook that writes a crash report to the log (with a backtrace).
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into());
        let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let bt = std::backtrace::Backtrace::force_capture();
        log::error!("CRASH in thread '{thread}' at {loc}: {msg}\n{bt}");
        if let Ok(mut c) = LAST_CRASH.lock() {
            *c = Some(format!("{msg}\n  at {loc} (thread '{thread}')"));
        }
        // The browser stops the game on a panic: say so on the page.
        #[cfg(target_arch = "wasm32")]
        crate::platform::error_box("Pavilion crashed", &format!("{msg}\n  at {loc}\n\nReload the page to start again."));
    }));
}

pub static LAST_CRASH: Mutex<Option<String>> = Mutex::new(None);
