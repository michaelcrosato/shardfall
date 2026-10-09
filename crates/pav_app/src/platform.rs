//! Tiny platform helpers: error pop-ups (a message box on Windows, the page in the browser) and
//! console attachment on Windows.

#[cfg(windows)]
pub fn error_box(title: &str, message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (t, m) = (wide(title), wide(message));
    unsafe {
        MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

#[cfg(all(not(windows), not(target_arch = "wasm32")))]
pub fn error_box(title: &str, message: &str) {
    eprintln!("== {title} ==\n{message}");
}

/// In the browser: replaces the page with the message.
#[cfg(target_arch = "wasm32")]
pub fn error_box(title: &str, message: &str) {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;");
    if let Some(body) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) {
        body.set_inner_html(&format!(
            "<div style='font:16px sans-serif;color:#eee;padding:32px;max-width:760px'><h2>{}</h2><pre style='white-space:pre-wrap'>{}</pre></div>",
            esc(title),
            esc(message)
        ));
    }
}

/// When started from a terminal, print there (the release .exe has no console of its own).
#[cfg(windows)]
pub fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
pub fn attach_console() {}

/// Whether this is a phone or tablet: the browser says its main pointer is a finger.
#[cfg(target_arch = "wasm32")]
pub fn touch_screen() -> bool {
    web_sys::window().and_then(|w| w.match_media("(pointer: coarse)").ok().flatten()).is_some_and(|m| m.matches())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn touch_screen() -> bool {
    false
}

/// Full screen on or off. In the browser the whole page goes full screen (the game's own
/// overlays stay on top of the canvas).
#[cfg(target_arch = "wasm32")]
pub fn toggle_fullscreen(_window: &winit::window::Window) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    if doc.fullscreen_element().is_some() {
        doc.exit_fullscreen();
    } else if let Some(el) = doc.document_element() {
        let _ = el.request_fullscreen();
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn toggle_fullscreen(window: &winit::window::Window) {
    let on = window.fullscreen().is_some();
    window.set_fullscreen(if on { None } else { Some(winit::window::Fullscreen::Borderless(None)) });
}
