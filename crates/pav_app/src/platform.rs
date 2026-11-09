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

/// A parameter of the page's address (`?lang=fr`).
#[cfg(target_arch = "wasm32")]
pub fn page_param(name: &str) -> Option<String> {
    let query = web_sys::window()?.location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&query).ok()?.get(name).filter(|v| !v.is_empty())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn page_param(_name: &str) -> Option<String> {
    None
}

/// The text of a block in the page (`<script type="application/json" id="…">`).
#[cfg(target_arch = "wasm32")]
pub fn page_block(id: &str) -> Option<String> {
    web_sys::window()?.document()?.get_element_by_id(id)?.text_content()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn page_block(_id: &str) -> Option<String> {
    None
}

/// A font the page carries (`<script type="application/octet-stream" id="font-NAME">`, the
/// font deflated with zlib, in base64; scripts/pack_single_html.py writes them).
#[cfg(target_arch = "wasm32")]
pub fn font(name: &str) -> Option<Vec<u8>> {
    let text = page_block(&format!("font-{name}"))?;
    let packed = base64_decode(&text)?;
    miniz_oxide::inflate::decompress_to_vec_zlib(&packed).ok()
}

#[cfg(target_arch = "wasm32")]
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in text.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The languages the browser or the system asks for, most wanted first.
#[cfg(target_arch = "wasm32")]
pub fn system_languages() -> Vec<String> {
    let Some(nav) = web_sys::window().map(|w| w.navigator()) else { return Vec::new() };
    let list: Vec<String> = nav.languages().iter().filter_map(|v| v.as_string()).collect();
    if list.is_empty() { nav.language().into_iter().collect() } else { list }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn system_languages() -> Vec<String> {
    // LANGUAGE (a list), then LC_ALL / LC_MESSAGES / LANG: "fr_FR.UTF-8" -> "fr-FR".
    let mut v = Vec::new();
    if let Ok(l) = std::env::var("LANGUAGE") {
        v.extend(l.split(':').filter(|s| !s.is_empty()).map(|s| s.to_string()));
    }
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(l) = std::env::var(key) {
            v.push(l);
        }
    }
    v.into_iter()
        .map(|s| s.split(['.', '@']).next().unwrap_or("").replace('_', "-"))
        .filter(|s| !s.is_empty() && s != "C" && s != "POSIX")
        .collect()
}

/// The player's saved language choice (the Language menu), under `lang:` and the page's path.
#[cfg(target_arch = "wasm32")]
pub fn saved_language() -> Option<String> {
    let w = web_sys::window()?;
    let key = format!("lang:{}", w.location().pathname().ok()?);
    w.local_storage().ok()??.get_item(&key).ok()?
}

#[cfg(target_arch = "wasm32")]
pub fn save_language(code: &str) {
    let Some(w) = web_sys::window() else { return };
    let key = format!("lang:{}", w.location().pathname().unwrap_or_default());
    if let Some(s) = w.local_storage().ok().flatten() {
        let _ = s.set_item(&key, code);
    }
}

/// `<html lang>` follows the language in use.
#[cfg(target_arch = "wasm32")]
pub fn set_page_language(code: &str) {
    if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
        let _ = el.set_attribute("lang", code);
    }
}

/// Restarts the page (a language chosen in the Language menu).
#[cfg(target_arch = "wasm32")]
pub fn reload() {
    if let Some(w) = web_sys::window() {
        let _ = w.location().reload();
    }
}
