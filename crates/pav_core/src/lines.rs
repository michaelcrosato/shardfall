//! Lines: the words players read, by id, in the player's language (AI Hub Languages v1,
//! https://raw.githubusercontent.com/michaelcrosato/ai-hub/main/lib/hub/languages.md).
//!
//! - The game's own lines (English) are a *line book*: `game/lines.json` (menus, HUD, help)
//!   plus lines made from the game data (item, skill and monster names...). The web page carries
//!   the same book as its `game-lines` block, for translators; `pav lines` prints it.
//! - Translations arrive later, as the page's `game-translations` block (one entry per language
//!   code). They are only ever read: a missing, empty or broken line shows the original
//!   (the page's **Critical** rule).
//! - `text(id, args)` (and the `t!` macro) gives a line in the chosen language with its
//!   `{blanks}` filled: numbers formatted for the language, `{count}` picking a plural form,
//!   `{key:action}` filled by the app's control labels, filled values isolated in
//!   right-to-left languages.
//! - Three test languages (`en-XA`, `zh-XC`, `ar-XB`) are made from the original lines when the
//!   address or `--lang` asks for one.
//!
//! The chosen language is process-wide, like the game's content caches. Gameplay never reads
//! it: the simulation keeps ids and numbers, and text is made where it is shown.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

/// A line: plain text, or plural forms (`zero`, `one`, `two`, `few`, `many`, `other`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Line {
    Text(String),
    Plural(BTreeMap<String, String>),
}

impl Line {
    fn usable(&self) -> bool {
        match self {
            Line::Text(s) => !s.is_empty(),
            Line::Plural(m) => !m.is_empty() && m.values().all(|s| !s.is_empty()),
        }
    }
}

/// The game's own lines: the `game-lines` block.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Book {
    #[serde(default)]
    pub version: u32,
    pub language: String,
    pub lines: BTreeMap<String, Line>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub notes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub max: BTreeMap<String, u32>,
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub brief: String,
}

impl Book {
    /// Adds a line made from data (an item's name...), with an optional note. An id already
    /// there keeps its text: the hand-written file wins.
    pub fn add(&mut self, id: impl Into<String>, text: impl Into<String>, note: Option<&str>) {
        let id = id.into();
        let text = text.into();
        if text.trim().is_empty() || !valid_id(&id) {
            return;
        }
        if let Some(n) = note {
            self.notes.entry(id.clone()).or_insert_with(|| n.to_string());
        }
        self.lines.entry(id).or_insert(Line::Text(text));
    }

    /// The block as the page carries it: JSON with every `<` written as `<`.
    pub fn to_block(&self) -> String {
        let mut v = self.clone();
        v.version = 1;
        block_json(&v)
    }
}

/// JSON for a `<script type="application/json">` block: `<` as `<`, so no line can end the
/// block early.
pub fn block_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default().replace('<', "\\u003c")
}

/// An id as the page's ids are: lowercase words joined by dots.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
}

/// One translation: the `game-translations` block's entry for a language.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Translation {
    #[serde(default)]
    pub by: String,
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    #[serde(default)]
    pub lines: BTreeMap<String, serde_json::Value>,
}

/// The languages the AI Hub uses, in its order (the ten most spoken, then the second set).
pub const LANGUAGES: [&str; 21] = [
    "en", "zh-Hans", "hi", "es-419", "ar", "fr", "bn", "pt-BR", "ru", "id", "ja", "de", "ko", "zh-Hant", "ur", "tr", "vi", "sw",
    "ha", "am", "yo",
];
/// The test languages, chosen only by the address (or `--lang`).
pub const TEST_LANGUAGES: [&str; 3] = ["en-XA", "zh-XC", "ar-XB"];

/// A value for a `{blank}`.
#[derive(Clone, Debug)]
pub enum Arg {
    Int(i64),
    Num(f64),
    Text(String),
}

macro_rules! arg_from_int {
    ($($t:ty),*) => {$(impl From<$t> for Arg { fn from(v: $t) -> Self { Arg::Int(v as i64) } })*};
}
arg_from_int!(i8, i16, i32, i64, u8, u16, u32, u64, usize, isize);
impl From<f32> for Arg {
    fn from(v: f32) -> Self {
        Arg::Num(v as f64)
    }
}
impl From<f64> for Arg {
    fn from(v: f64) -> Self {
        Arg::Num(v)
    }
}
impl From<&str> for Arg {
    fn from(v: &str) -> Self {
        Arg::Text(v.to_string())
    }
}
impl From<String> for Arg {
    fn from(v: String) -> Self {
        Arg::Text(v)
    }
}
impl From<&String> for Arg {
    fn from(v: &String) -> Self {
        Arg::Text(v.clone())
    }
}

/// A line in the player's language: `t!("menu.start")`, `t!("hud.potions", count = n)`.
#[macro_export]
macro_rules! t {
    ($id:expr) => {
        $crate::lines::text($id, &[])
    };
    ($id:expr, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::lines::text($id, &[$((stringify!($name), $crate::lines::Arg::from($val))),+])
    };
}

struct State {
    book: Arc<Book>,
    lang: String,
    table: BTreeMap<String, Line>,
    /// Languages with a translation, besides the book's own.
    translated: Vec<String>,
    control: fn(&str) -> String,
}

static STATE: RwLock<Option<State>> = RwLock::new(None);
static SOURCE: RwLock<Option<fn() -> Book>> = RwLock::new(None);
static TRANSLATIONS: RwLock<Option<BTreeMap<String, Translation>>> = RwLock::new(None);

/// Sets where the game's own lines come from (default: Shardfall's, `arpg::lines::book`).
pub fn set_source(f: fn() -> Book) {
    *SOURCE.write().unwrap() = Some(f);
    *STATE.write().unwrap() = None;
}

fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    if let Some(s) = STATE.read().unwrap().as_ref() {
        return f(s);
    }
    // Shardfall's book unless another game set its own.
    let book = Arc::new(SOURCE.read().unwrap().map(|f| f()).unwrap_or_else(crate::arpg::lines::book));
    let lang = book.language.clone();
    let state = State { book, lang, table: BTreeMap::new(), translated: Vec::new(), control: keyboard_label };
    let mut w = STATE.write().unwrap();
    let s = w.get_or_insert(state);
    rebuild(s);
    f(s)
}

fn rebuild(s: &mut State) {
    let tr = TRANSLATIONS.read().unwrap();
    s.translated = tr.as_ref().map(|t| t.keys().filter(|k| **k != s.book.language).cloned().collect()).unwrap_or_default();
    s.table = BTreeMap::new();
    if let Some(entry) = tr.as_ref().and_then(|t| t.get(&s.lang)) {
        for (id, v) in &entry.lines {
            // Anything that isn't text or plural forms of text is ignored (the original shows).
            if let Ok(line) = serde_json::from_value::<Line>(v.clone())
                && line.usable()
            {
                s.table.insert(id.clone(), line);
            }
        }
    }
}

/// Reads the `game-translations` block. A broken block is logged and ignored: every line then
/// shows the original.
pub fn set_translations(json: &str) {
    let parsed = match serde_json::from_str::<BTreeMap<String, serde_json::Value>>(json) {
        Ok(map) => map
            .into_iter()
            .filter_map(|(code, v)| match serde_json::from_value::<Translation>(v) {
                Ok(t) => Some((code, t)),
                Err(e) => {
                    log::warn!("game-translations: {code} ignored ({e})");
                    None
                }
            })
            .collect(),
        Err(e) => {
            log::warn!("game-translations ignored: {e}");
            BTreeMap::new()
        }
    };
    *TRANSLATIONS.write().unwrap() = Some(parsed);
    if let Some(s) = STATE.write().unwrap().as_mut() {
        rebuild(s);
    }
}

/// Chooses the language: the first of `wanted` (the address's `lang`, the player's saved
/// choice, the system's languages) that the game has, else the game's own. A test language
/// counts only when `asked` names it. Returns the code chosen.
pub fn choose(asked: Option<&str>, wanted: &[String]) -> String {
    let available = available();
    let code = pick(asked, wanted, &available, &own_language());
    set_language(&code);
    code
}

/// Sets the language directly (a code the game has, or a test language).
pub fn set_language(code: &str) {
    with_state(|_| ());
    if let Some(s) = STATE.write().unwrap().as_mut() {
        s.lang = code.to_string();
        rebuild(s);
    }
}

/// The game's own language and every language it has a translation for.
pub fn available() -> Vec<String> {
    with_state(|s| std::iter::once(s.book.language.clone()).chain(s.translated.iter().cloned()).collect())
}

pub fn own_language() -> String {
    with_state(|s| s.book.language.clone())
}

/// The language in use.
pub fn language() -> String {
    with_state(|s| s.lang.clone())
}

/// The game's own line book (for `pav lines` and the web page).
pub fn book() -> Arc<Book> {
    with_state(|s| s.book.clone())
}

/// Sets what fills `{key:action}`: the key, button or on-screen control for the action.
pub fn set_control_label(f: fn(&str) -> String) {
    with_state(|_| ());
    if let Some(s) = STATE.write().unwrap().as_mut() {
        s.control = f;
    }
}

/// The keyboard's key for an action, the default for `{key:action}` (a game can set its own
/// with `set_control_label`, for a gamepad or touch screen).
pub fn keyboard_label(action: &str) -> String {
    match action {
        "use" => "G",
        "dodge" => "Space",
        "potion" => "1",
        "map" => "M",
        "inventory" => "I",
        "character" => "C",
        "skills" => "K",
        "tree" => "P",
        "town" => "T",
        "menu" => "Esc",
        "tuning" => "F1",
        "rooms" => "F2",
        other => other,
    }
    .to_string()
}

/// Written right to left (Arabic, Urdu, Persian, Hebrew...).
pub fn is_rtl(code: &str) -> bool {
    let l = code.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    matches!(l.as_str(), "ar" | "fa" | "ur" | "he" | "ps" | "sd" | "ug" | "yi" | "ckb" | "dv")
}

/// The language in use is written right to left.
pub fn rtl() -> bool {
    is_rtl(&language())
}

/// The language's text runs without spaces between words (Chinese, Japanese, Thai).
pub fn unspaced(code: &str) -> bool {
    let l = code.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    matches!(l.as_str(), "zh" | "ja" | "th" | "lo" | "km" | "my")
}

/// A line in the language in use, its blanks filled, ready to draw: in a right-to-left
/// language, laid out for egui's shaper (`for_rtl_shaper`). An unknown id shows the id.
pub fn text(id: &str, args: &[(&str, Arg)]) -> String {
    let out = logical(id, args);
    if rtl() { for_rtl_shaper(&out) } else { out }
}

/// A line in the language in use, its blanks filled, in reading order: for text that leaves
/// the game (the AI Hub's scoreboard labels).
pub fn logical(id: &str, args: &[(&str, Arg)]) -> String {
    with_state(|s| {
        let translated = s.table.get(id).filter(|l| l.usable());
        let fake = TEST_LANGUAGES.contains(&s.lang.as_str());
        let (line, text_lang) = match translated {
            Some(l) => (l, s.lang.as_str()),
            None => match s.book.lines.get(id) {
                Some(l) => (l, s.book.language.as_str()),
                None => return id.to_string(),
            },
        };
        let count = args.iter().find(|(n, _)| *n == "count").map(|(_, a)| match a {
            Arg::Int(i) => *i as f64,
            Arg::Num(f) => *f,
            Arg::Text(t) => t.parse().unwrap_or(0.0),
        });
        let mut out = match line {
            Line::Text(t) => t.clone(),
            Line::Plural(forms) => {
                let n = count.unwrap_or(0.0);
                let form = plural_category(text_lang, n);
                forms
                    .get("zero")
                    .filter(|_| n == 0.0)
                    .or_else(|| forms.get(form))
                    .or_else(|| forms.get("other"))
                    .or_else(|| forms.values().next())
                    .cloned()
                    .unwrap_or_default()
            }
        };
        if fake {
            out = fake_line(&s.lang, &out, s.book.max.get(id).copied());
        }
        let rtl = is_rtl(&s.lang);
        fill(&out, args, text_lang, rtl, s.control)
    })
}

/// A line made at run time from data with a known id: its translation when there is one,
/// else `original` (the data's own text), through the test languages. For names that come
/// from data whose ids aren't all known when the book is made.
pub fn or_original(id: &str, original: &str) -> String {
    let out = or_original_logical(id, original);
    if rtl() { for_rtl_shaper(&out) } else { out }
}

/// `or_original` in reading order: for a blank in another line (the whole is laid out once).
pub fn or_original_logical(id: &str, original: &str) -> String {
    let known = with_state(|s| s.table.contains_key(id) || s.book.lines.contains_key(id));
    if known {
        return logical(id, &[]);
    }
    with_state(
        |s| {
            if TEST_LANGUAGES.contains(&s.lang.as_str()) { fake_line(&s.lang, original, None) } else { original.to_string() }
        },
    )
}

/// Text in reading order (a filled line, a name) laid out to draw: `for_rtl_shaper` in a
/// right-to-left language, as it is otherwise.
pub fn display(text: String) -> String {
    if rtl() { for_rtl_shaper(&text) } else { text }
}

fn fill(line: &str, args: &[(&str, Arg)], text_lang: &str, rtl: bool, control: fn(&str) -> String) -> String {
    let mut out = String::with_capacity(line.len() + 16);
    let mut rest = line;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let tag = &rest[open + 1..open + close];
        let whole = &rest[open..open + close + 1];
        if let Some(action) = tag.strip_prefix("key:") {
            out.push_str(&control(action));
        } else if let Some((_, v)) = args.iter().find(|(n, _)| *n == tag) {
            let s = match v {
                Arg::Int(i) => format_int(text_lang, *i),
                Arg::Num(f) => format_number(text_lang, *f),
                Arg::Text(t) => t.clone(),
            };
            if rtl {
                out.push('\u{2068}');
                out.push_str(&s);
                out.push('\u{2069}');
            } else {
                out.push_str(&s);
            }
        } else {
            out.push_str(whole);
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

/// The CLDR plural category of `n` in a language: `zero`, `one`, `two`, `few`, `many` or
/// `other` (cardinal numbers, as `Intl.PluralRules` gives them).
pub fn plural_category(code: &str, n: f64) -> &'static str {
    let l = code.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    let whole = n.fract() == 0.0;
    let i = n.abs().trunc() as u64;
    let (i10, i100) = (i % 10, i % 100);
    match l.as_str() {
        "zh" | "ja" | "ko" | "id" | "vi" | "yo" | "th" | "ms" => "other",
        "ar" => match (whole, i, i100) {
            (true, 0, _) => "zero",
            (true, 1, _) => "one",
            (true, 2, _) => "two",
            (true, _, 3..=10) => "few",
            (true, _, 11..=99) => "many",
            _ => "other",
        },
        "ru" | "uk" | "be" => {
            if !whole {
                "other"
            } else if i10 == 1 && i100 != 11 {
                "one"
            } else if (2..=4).contains(&i10) && !(12..=14).contains(&i100) {
                "few"
            } else {
                "many"
            }
        }
        // 0 and 1 (and 0.x, 1.x) take "one".
        "fr" | "pt" => {
            if i <= 1 {
                "one"
            } else if whole && i != 0 && i.is_multiple_of(1_000_000) {
                "many"
            } else {
                "other"
            }
        }
        // i = 0 or n = 1.
        "hi" | "bn" | "am" | "gu" | "kn" | "fa" | "zu" => {
            if i == 0 || n == 1.0 {
                "one"
            } else {
                "other"
            }
        }
        "es" | "it" => {
            if n == 1.0 {
                "one"
            } else if whole && i != 0 && i.is_multiple_of(1_000_000) {
                "many"
            } else {
                "other"
            }
        }
        _ => {
            if n == 1.0 {
                "one"
            } else {
                "other"
            }
        }
    }
}

/// How a language writes numbers: decimal mark, group mark, Indian grouping, digits.
fn number_style(code: &str) -> (char, Option<char>, bool, Option<char>) {
    let l = code.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    match l.as_str() {
        "de" | "id" | "tr" | "vi" | "pt" | "it" | "nl" | "da" => (',', Some('.'), false, None),
        "fr" => (',', Some('\u{202F}'), false, None),
        "ru" | "uk" | "pl" | "cs" | "sv" | "fi" | "nb" => (',', Some('\u{00A0}'), false, None),
        "ar" => ('\u{066B}', Some('\u{066C}'), false, Some('\u{0660}')),
        "bn" => ('.', Some(','), true, Some('\u{09E6}')),
        "hi" => ('.', Some(','), true, None),
        // Spanish groups only from five digits.
        "es" => ('.', Some(','), false, None),
        _ => ('.', Some(','), false, None),
    }
}

/// A whole number written for a language (`12,500`, `12.500`, `12 500`).
pub fn format_int(code: &str, n: i64) -> String {
    let (_, group, indian, zero) = number_style(code);
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    let len = digits.len();
    let spanish = code.starts_with("es");
    for (k, c) in digits.chars().enumerate() {
        let left = len - k;
        if k > 0 && !(spanish && len <= 4) {
            let cut = if indian { left == 3 || (left > 3 && (left - 3).is_multiple_of(2)) } else { left.is_multiple_of(3) };
            if cut && let Some(g) = group {
                out.push(g);
            }
        }
        out.push(c);
    }
    let out = match zero {
        Some(z) => out.chars().map(|c| c.to_digit(10).and_then(|d| char::from_u32(z as u32 + d)).unwrap_or(c)).collect(),
        None => out,
    };
    if n < 0 { format!("-{out}") } else { out }
}

/// A number written for a language, with up to two decimals (no trailing zeros).
pub fn format_number(code: &str, n: f64) -> String {
    if !n.is_finite() {
        return n.to_string();
    }
    let rounded = (n * 100.0).round() / 100.0;
    if rounded.fract() == 0.0 {
        return format_int(code, rounded as i64);
    }
    let (dec, _, _, zero) = number_style(code);
    let whole = format_int(code, rounded.trunc() as i64);
    let whole = if rounded < 0.0 && !whole.starts_with('-') { format!("-{whole}") } else { whole };
    let frac = format!("{:.2}", rounded.abs().fract());
    let frac = frac.trim_start_matches('0').trim_start_matches('.').trim_end_matches('0');
    let frac: String = match zero {
        Some(z) => frac.chars().map(|c| c.to_digit(10).and_then(|d| char::from_u32(z as u32 + d)).unwrap_or(c)).collect(),
        None => frac.to_string(),
    };
    format!("{whole}{dec}{frac}")
}

/// A language's own name, in that language, for the Language menu.
pub fn language_name(code: &str) -> String {
    let name = match code {
        "en" => "English",
        "zh-Hans" => "简体中文",
        "hi" => "हिन्दी",
        "es-419" => "Español latinoamericano",
        "ar" => "العربية",
        "fr" => "Français",
        "bn" => "বাংলা",
        "pt-BR" => "Português (Brasil)",
        "ru" => "Русский",
        "id" => "Indonesia",
        "ja" => "日本語",
        "de" => "Deutsch",
        "ko" => "한국어",
        "zh-Hant" => "繁體中文",
        "ur" => "اردو",
        "tr" => "Türkçe",
        "vi" => "Tiếng Việt",
        "sw" => "Kiswahili",
        "ha" => "Hausa",
        "am" => "አማርኛ",
        "yo" => "Èdè Yorùbá",
        "en-XA" => "[Éñglísh ······]",
        "zh-XC" => "中文 (测试)",
        "ar-XB" => "عربي (اختبار)",
        other => other,
    };
    name.to_string()
}

/// The language's subtags: language and writing system, filled in for the codes the hub uses
/// and common regional codes (`zh-TW` is written in Traditional characters).
fn parts(code: &str) -> (String, String) {
    let mut it = code.split(['-', '_']);
    let lang = it.next().unwrap_or("").to_ascii_lowercase();
    let rest: Vec<String> = it.map(|s| s.to_string()).collect();
    let script = rest.iter().find(|s| s.len() == 4).map(|s| s.to_ascii_lowercase());
    let region = rest.iter().find(|s| s.len() == 2 || s.len() == 3).map(|s| s.to_ascii_uppercase());
    let script = script.unwrap_or_else(|| match (lang.as_str(), region.as_deref()) {
        ("zh", Some("TW" | "HK" | "MO")) => "hant".into(),
        ("zh", _) => "hans".into(),
        ("ar" | "ur" | "fa", _) => "arab".into(),
        ("ru" | "uk" | "be" | "bg" | "sr", _) => "cyrl".into(),
        ("hi" | "mr" | "ne", _) => "deva".into(),
        ("bn", _) => "beng".into(),
        ("ja", _) => "jpan".into(),
        ("ko", _) => "kore".into(),
        ("am", _) => "ethi".into(),
        _ => "latn".into(),
    });
    (lang, script)
}

/// The language to show: the first wanted code the game has, matched in full, then by
/// language and writing system, then by language. A test language only when `asked` is one.
pub fn pick(asked: Option<&str>, wanted: &[String], available: &[String], own: &str) -> String {
    if let Some(a) = asked
        && TEST_LANGUAGES.contains(&a)
    {
        return a.to_string();
    }
    let list: Vec<&str> = asked.into_iter().chain(wanted.iter().map(|s| s.as_str())).filter(|s| !s.is_empty()).collect();
    for want in list {
        let (l, s) = parts(want);
        let hit = available
            .iter()
            .find(|a| a.eq_ignore_ascii_case(want))
            .or_else(|| available.iter().find(|a| parts(a) == (l.clone(), s.clone())))
            .or_else(|| available.iter().find(|a| parts(a).0 == l));
        if let Some(h) = hit {
            return h.clone();
        }
    }
    own.to_string()
}

/// A line's width for `max`: letters, a Chinese, Japanese or Korean character counting two;
/// tags count nothing.
pub fn width(line: &str) -> usize {
    let mut n = 0;
    let mut depth = 0;
    for c in line.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = (depth - 1).max(0),
            _ if depth > 0 => {}
            _ if is_combining(c) => {}
            _ => n += if wide(c) { 2 } else { 1 },
        }
    }
    n
}

fn wide(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x11FF | 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x2FFFF)
}

fn is_combining(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// A test language made from a line before its blanks are filled: accented and longer
/// (`en-XA`), Chinese characters without spaces (`zh-XC`), right-to-left letters (`ar-XB`).
/// The same as the page's example `fake()`.
pub fn fake_line(lang: &str, line: &str, max: Option<u32>) -> String {
    const HAN: &str = "的一是不了人我在有他这中大来上国个到说们为子和你地出道也时年得就那要下以生会自着去之过家学对";
    const ARABIC: &str = "ابتثجحخدذرزسشصضطظعغفقكلمنهوي";
    let han: Vec<char> = HAN.chars().collect();
    let arabic: Vec<char> = ARABIC.chars().collect();
    let mut out = String::new();
    for (i, part) in split_tags(line).into_iter().enumerate() {
        if i % 2 == 1 {
            out.push_str(part);
            continue;
        }
        match lang {
            "zh-XC" => {
                let mut word: Vec<char> = Vec::new();
                let flush = |word: &mut Vec<char>, out: &mut String| {
                    if word.is_empty() {
                        return;
                    }
                    let n = word.len().div_ceil(2);
                    for k in 0..n {
                        let code = word[k] as usize;
                        out.push(han[(code * 7 + k) % han.len()]);
                    }
                    word.clear();
                };
                for c in part.chars() {
                    if c.is_alphabetic() || is_combining(c) {
                        word.push(c);
                        continue;
                    }
                    flush(&mut word, &mut out);
                    match c {
                        ',' => out.push('，'),
                        '.' => out.push('。'),
                        '!' => out.push('！'),
                        '?' => out.push('？'),
                        ':' => out.push('：'),
                        ';' => out.push('；'),
                        ' ' | '\t' => {}
                        c => out.push(c),
                    }
                }
                flush(&mut word, &mut out);
            }
            "ar-XB" => {
                for c in part.chars() {
                    if c.is_alphabetic() {
                        out.push(arabic[c as usize % arabic.len()]);
                    } else if !is_combining(c) {
                        out.push(c);
                    }
                }
            }
            _ => {
                for c in part.chars() {
                    out.push(match c {
                        'a' => 'á',
                        'e' => 'é',
                        'i' => 'í',
                        'o' => 'ó',
                        'u' => 'ú',
                        'n' => 'ñ',
                        'c' => 'ç',
                        'A' => 'Á',
                        'E' => 'É',
                        'I' => 'Í',
                        'O' => 'Ó',
                        'U' => 'Ú',
                        'N' => 'Ñ',
                        c => c,
                    });
                }
            }
        }
    }
    if lang != "en-XA" {
        return out;
    }
    if let Some(m) = max {
        let pad = (m as usize).saturating_sub(width(&out) + 2);
        return format!("[{out}{}]", "·".repeat(pad));
    }
    out.split('\n')
        .map(|p| format!("[{p} {}]", "·".repeat((width(p) as f32 * 0.4).ceil() as usize)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Text and `{tags}` alternately (even = text, odd = tag).
fn split_tags(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    loop {
        match (rest.find('{'), rest.find('}')) {
            (Some(a), Some(b)) if b > a && !rest[a + 1..b].contains('{') => {
                out.push(&rest[..a]);
                out.push(&rest[a..=b]);
                rest = &rest[b + 1..];
            }
            _ => {
                out.push(rest);
                return out;
            }
        }
    }
}

/// For drawing a right-to-left line with a shaper that lays a whole run out right to left
/// (egui's harfrust guesses the direction from the first strong letter): every left-to-right
/// stretch (Latin words, numbers) is turned round, so that turning the whole line round puts
/// it back. Lines that start with a left-to-right letter are left alone.
pub fn for_rtl_shaper(line: &str) -> String {
    // Arabic-Indic digits read left to right inside a right-to-left line, like Latin ones.
    let digit = |c: char| matches!(c as u32, 0x0660..=0x0669 | 0x06F0..=0x06F9);
    // The Arabic decimal and thousands marks sit inside numbers: neutral, like Latin ones.
    let strong_rtl = |c: char| {
        matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
            && !digit(c)
            && !matches!(c as u32, 0x066B | 0x066C)
    };
    let strong_ltr = |c: char| (c.is_alphanumeric() && !strong_rtl(c)) || digit(c);
    // harfrust takes the run's direction from its first character with a script of its own:
    // a letter, or an Arabic-Indic digit (Arabic), never a European digit or punctuation. So a
    // line such as "12 دينار" is laid out right to left too.
    let directing = |c: &char| c.is_alphabetic() || digit(*c);
    if !line.chars().find(directing).is_some_and(|c| strong_rtl(c) || digit(c)) {
        return line.to_string();
    }
    let chars: Vec<char> = line.chars().filter(|c| !matches!(*c, '\u{2066}'..='\u{2069}')).collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        if strong_ltr(chars[i]) {
            // A left-to-right stretch: letters and digits with the marks between them.
            let mut j = i;
            let mut end = i;
            while j < chars.len() && !strong_rtl(chars[j]) {
                if strong_ltr(chars[j]) {
                    end = j;
                }
                j += 1;
            }
            out.extend(chars[i..=end].iter().rev());
            i = end + 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plural_forms() {
        assert_eq!(plural_category("en", 1.0), "one");
        assert_eq!(plural_category("en", 2.0), "other");
        assert_eq!(plural_category("ru", 21.0), "one");
        assert_eq!(plural_category("ru", 22.0), "few");
        assert_eq!(plural_category("ru", 25.0), "many");
        assert_eq!(plural_category("ru", 12.0), "many");
        assert_eq!(plural_category("ar", 0.0), "zero");
        assert_eq!(plural_category("ar", 2.0), "two");
        assert_eq!(plural_category("ar", 105.0), "few");
        assert_eq!(plural_category("ar", 111.0), "many");
        assert_eq!(plural_category("fr", 0.0), "one");
        assert_eq!(plural_category("zh-Hans", 1.0), "other");
        assert_eq!(plural_category("hi", 0.0), "one");
    }

    #[test]
    fn numbers() {
        assert_eq!(format_int("en", 12500), "12,500");
        assert_eq!(format_int("de", 12500), "12.500");
        assert_eq!(format_int("fr", 12500), "12\u{202F}500");
        assert_eq!(format_int("hi", 1250000), "12,50,000");
        assert_eq!(format_int("es-419", 2500), "2500");
        assert_eq!(format_int("es-419", 12500), "12,500");
        assert_eq!(format_int("ar", 12), "١٢");
        assert_eq!(format_number("de", 1.5), "1,5");
        assert_eq!(format_number("en", -3.25), "-3.25");
    }

    #[test]
    fn picking() {
        let have: Vec<String> = ["en", "zh-Hant", "zh-Hans", "es-419", "pt-BR"].iter().map(|s| s.to_string()).collect();
        let w = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(pick(None, &w(&["zh-TW"]), &have, "en"), "zh-Hant");
        assert_eq!(pick(None, &w(&["zh-CN"]), &have, "en"), "zh-Hans");
        assert_eq!(pick(None, &w(&["es-MX"]), &have, "en"), "es-419");
        assert_eq!(pick(None, &w(&["pt-PT"]), &have, "en"), "pt-BR");
        assert_eq!(pick(None, &w(&["ko"]), &have, "en"), "en");
        assert_eq!(pick(Some("ar-XB"), &[], &have, "en"), "ar-XB");
        assert_eq!(pick(Some("de"), &w(&["es"]), &have, "en"), "es-419");
    }

    #[test]
    fn fakes() {
        assert_eq!(fake_line("en-XA", "Stay on my wing", None), "[Stáy óñ my wíñg ······]");
        assert!(fake_line("en-XA", "Start", Some(12)).chars().count() == 12);
        let zh = fake_line("zh-XC", "Hello, {player}!", None);
        assert!(zh.contains("{player}") && !zh.contains(' ') && zh.contains('，'));
        let ar = fake_line("ar-XB", "Hi {n}", None);
        assert!(ar.contains("{n}") && ar.chars().next().is_some_and(|c| ('\u{0600}'..='\u{06FF}').contains(&c)));
    }

    #[test]
    fn rtl_shaping_order() {
        // "ab 12 cd" in Arabic letters with a number: the number is turned round for the shaper.
        assert_eq!(for_rtl_shaper("اب 12 تث"), "اب 21 تث");
        assert_eq!(for_rtl_shaper("abc 12"), "abc 12");
        assert_eq!(for_rtl_shaper("اب ١٢٬٥ تث"), "اب ٥٬٢١ تث");
        // A line that starts with a number still runs right to left (its first letter is).
        assert_eq!(for_rtl_shaper("12% تث"), "21% تث");
        assert_eq!(for_rtl_shaper("12% abc"), "12% abc");
    }

    #[test]
    fn ids() {
        assert!(valid_id("menu.start") && valid_id("item.sword_1.name"));
        assert!(!valid_id("Menu.start") && !valid_id("menu..start") && !valid_id(""));
    }
}
