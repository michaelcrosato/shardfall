//! The language the game is shown in, and the fonts to draw it (AI Hub Languages v1,
//! docs/LANGUAGES.md). The lines themselves are `pav_core::lines` (`t!`).
//!
//! The language is the first of: the address's `?lang=` (the AI Hub adds it every time) or
//! `--lang`; the player's choice from the title screen's Language menu; the browser's or the
//! system's languages; the game's own (English). Test languages (`en-XA`, `zh-XC`, `ar-XB`)
//! come only from the address or `--lang`.

use pav_core::lines;

/// Reads the page's translations and chooses the language. Returns its code.
pub fn init(asked: &str, prefs: &crate::save::Prefs) -> String {
    pav_core::arpg::lines::install();
    lines::set_control_label(lines::keyboard_label);
    if let Some(t) = crate::platform::page_block("game-translations") {
        lines::set_translations(&t);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(t) = std::fs::read_to_string(crate::boot::exe_dir().join("shardfall_translations.json")) {
        lines::set_translations(&t);
    }
    let mut wanted = Vec::new();
    #[cfg(target_arch = "wasm32")]
    wanted.extend(crate::platform::saved_language());
    #[cfg(not(target_arch = "wasm32"))]
    wanted.extend(prefs.lang.clone());
    #[cfg(target_arch = "wasm32")]
    let _ = prefs;
    wanted.extend(crate::platform::system_languages());
    let asked = (!asked.is_empty()).then_some(asked);
    let code = lines::choose(asked, &wanted);
    #[cfg(target_arch = "wasm32")]
    crate::platform::set_page_language(&code);
    log::info!("language: {code} (of {})", lines::available().join(", "));
    code
}

/// Gives egui the fonts for the language in use, and for `names` (players' names from the AI
/// Hub, which can be in any script).
pub fn install_fonts(ctx: &egui::Context, names: &[String]) {
    let lang = lines::language();
    #[cfg(target_arch = "wasm32")]
    let set = pav_ui::fonts::install(ctx, &lang, names, &crate::platform::font);
    #[cfg(not(target_arch = "wasm32"))]
    let set = pav_ui::fonts::install_embedded(ctx, &lang, names);
    log::info!("fonts for {lang}: {}", set.join(", "));
}

/// The Language menu shows when the address doesn't name the language: with `?lang=` the AI
/// Hub's own menu is in charge.
pub fn menu_shown() -> bool {
    crate::platform::page_param("lang").is_none()
}

/// A choice from the Language menu: saved, then the game restarts in it (the browser reloads
/// the page; the native game switches at once, since every line is made where it is shown).
pub fn choose(ctx: &egui::Context, code: &str, prefs: &mut crate::save::Prefs) {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (ctx, prefs);
        crate::platform::save_language(code);
        crate::platform::reload();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        prefs.lang = Some(code.to_string());
        crate::save::save_prefs(prefs);
        lines::set_language(code);
        install_fonts(ctx, &[]);
    }
}
