//! The fonts lines are drawn with, per language (AI Hub Languages v1, docs/LANGUAGES.md).
//!
//! egui's own font (Ubuntu Light) has the Latin alphabet; the AI Hub's languages also need
//! Arabic, Devanagari, Bengali, Ethiopic and Chinese, Japanese and Korean characters. The fonts
//! in `assets/fonts` (made by `scripts/fonts.py`) cover them. For a language, its own script's
//! font goes first, so a whole line, numbers and Latin names included, shapes as one run (egui
//! shapes with harfrust, which joins Arabic letters and builds Devanagari and Bengali ones);
//! the rest stay behind it for any character it lacks.
//!
//! Native builds carry the fonts (`embedded`); the browser build carries them in the page and
//! decodes only those the language needs (pav_app's `platform::font`).

/// Every font, by name (the file is `assets/fonts/<name>.ttf`).
pub const FONTS: [&str; 9] = [
    "NotoSans",
    "NotoSansArabic",
    "NotoSansDevanagari",
    "NotoSansBengali",
    "NotoSansEthiopic",
    "NotoSansSC",
    "NotoSansTC",
    "NotoSansJP",
    "NotoSansKR",
];

/// The font a language's lines are drawn with first, if not egui's own (English).
pub fn primary(lang: &str) -> Option<&'static str> {
    let l = lang.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    Some(match (l.as_str(), lang) {
        ("en", "en-XA") => "NotoSans",
        ("en", _) => return None,
        (_, "zh-Hant" | "zh-TW" | "zh-HK" | "zh-MO") => "NotoSansTC",
        ("zh", _) => "NotoSansSC",
        ("ja", _) => "NotoSansJP",
        ("ko", _) => "NotoSansKR",
        ("ar" | "ur" | "fa", _) => "NotoSansArabic",
        ("hi" | "mr" | "ne", _) => "NotoSansDevanagari",
        ("bn", _) => "NotoSansBengali",
        ("am", _) => "NotoSansEthiopic",
        _ => "NotoSans",
    })
}

/// The fonts to load for a language: its own first, then the fallbacks. `names` are other
/// text that will show (players' names from the AI Hub): the fonts their scripts need come too.
pub fn needed(lang: &str, names: &[String]) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = primary(lang).into_iter().collect();
    if !v.contains(&"NotoSans") {
        v.push("NotoSans");
    }
    for name in names {
        for c in name.chars() {
            let f = match c as u32 {
                0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => "NotoSansArabic",
                0x0900..=0x097F => "NotoSansDevanagari",
                0x0980..=0x09FF => "NotoSansBengali",
                0x1200..=0x139F | 0x2D80..=0x2DDF => "NotoSansEthiopic",
                0x3040..=0x30FF => "NotoSansJP",
                0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => "NotoSansKR",
                0x2E80..=0x9FFF | 0xF900..=0xFAFF => "NotoSansSC",
                _ => continue,
            };
            if !v.contains(&f) {
                v.push(f);
            }
        }
    }
    v
}

/// The fonts native builds carry.
#[cfg(not(target_arch = "wasm32"))]
pub fn embedded(name: &str) -> Option<&'static [u8]> {
    macro_rules! font {
        ($n:literal) => {
            include_bytes!(concat!("../../../assets/fonts/", $n, ".ttf")).as_slice()
        };
    }
    Some(match name {
        "NotoSans" => font!("NotoSans"),
        "NotoSansArabic" => font!("NotoSansArabic"),
        "NotoSansDevanagari" => font!("NotoSansDevanagari"),
        "NotoSansBengali" => font!("NotoSansBengali"),
        "NotoSansEthiopic" => font!("NotoSansEthiopic"),
        "NotoSansSC" => font!("NotoSansSC"),
        "NotoSansTC" => font!("NotoSansTC"),
        "NotoSansJP" => font!("NotoSansJP"),
        "NotoSansKR" => font!("NotoSansKR"),
        _ => return None,
    })
}

/// Sets egui's fonts for a language: its own script's font first, the others behind egui's.
/// `get` gives a font's bytes by name (None: not available, skipped). Returns the fonts set.
pub fn install(ctx: &egui::Context, lang: &str, names: &[String], get: &dyn Fn(&str) -> Option<Vec<u8>>) -> Vec<&'static str> {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut defs = FontDefinitions::default();
    let first = primary(lang);
    let mut set = Vec::new();
    for name in needed(lang, names) {
        let Some(bytes) = get(name) else {
            log::warn!("font {name} is missing: some letters may not show");
            continue;
        };
        defs.font_data.insert(name.to_string(), std::sync::Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let list = defs.families.entry(family.clone()).or_default();
            if Some(name) == first && family == FontFamily::Proportional {
                list.insert(0, name.to_string());
            } else {
                list.push(name.to_string());
            }
        }
        set.push(name);
    }
    ctx.set_fonts(defs);
    set
}

/// Installs the fonts native builds carry.
#[cfg(not(target_arch = "wasm32"))]
pub fn install_embedded(ctx: &egui::Context, lang: &str, names: &[String]) -> Vec<&'static str> {
    install(ctx, lang, names, &|n| embedded(n).map(|b| b.to_vec()))
}
