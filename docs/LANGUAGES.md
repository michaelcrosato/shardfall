# Languages

Shardfall follows AI Hub Languages v1
(https://raw.githubusercontent.com/michaelcrosato/ai-hub/main/lib/hub/languages.md): it is built
in English and can be shown in any of the AI Hub's 21 languages once a translation is added to
the page, without changing the game.

## Lines
- **The line book.** Every word players read is a line under an id. `game/lines.toml` holds the
  menus, HUD, help and messages (with `[notes]` for context, `[max]` widths for tight labels, the
  proper `names` and the translator's `brief`); `pav_core/src/arpg/lines.rs` adds lines made from
  the game data (`skill.<key>.name`, `item.<key>.name`, `monster.<key>.name`, `stat.<key>`...),
  so a name written in `game/*.toml` is translatable without being written twice. `pav lines
  out=FILE` writes the whole book as JSON; the web page carries it as its `game-lines` block.
- **In code.** `pav_core::t!("hud.potions", count = n)`: the line in the language in use with its
  blanks filled. Whole sentences with `{blanks}`; `{count}` picks a plural form (`one`, `other`,
  and whatever a translation's language needs, from CLDR's rules); numbers are written for the
  language (`12,500`, `12.500`, `12 500`); `{key:use}` becomes the key for the action. Data names
  without a known id go through `lines::or_original`. Text that leaves the game (the hub's
  scoreboard labels) uses `lines::logical`.
- **Adding a line.** Add it to `game/lines.toml` under an id that says where it shows, and use it
  through `t!`. Keep an id once it's published: translations are filed under it. Developer text
  goes under `dev.` ids (translation skips them).

## Choosing the language
The first of: the address's `?lang=` (the AI Hub always adds it) or `--lang CODE` (native, or
`lang = "fr"` in shardfall.toml); the player's choice at the plaza's Language station
(saved under `lang:` and the page's path, or in the native preferences); the browser's or the
system's languages; English. A code matches in full, then by language and writing system, then
by language (`zh-TW` takes `zh-Hant`, `es-MX` takes `es-419`). The Language window (the
station's: the spec puts the menu where a restart costs nothing, and the plaza is where the game
opens) lists the languages the page has, each in its own name; when the address names the
language it says so and offers none.

## Translations
A translation step adds the page's `game-translations` block (one entry per language code, with
`by`, `names` and `lines`), right after `game-lines`, and changes nothing else. The game reads it
at start (native: `shardfall_translations.json` beside the executable). A missing, empty or
broken block or line shows the original English line; it never stops the game.

## Drawing
- **Fonts.** egui draws the lines. Its own font has the Latin alphabet, so `assets/fonts` carries
  Noto fonts cut down to each script's common characters (`scripts/fonts.py`: Latin with
  Vietnamese, Hausa and Yoruba letters, Cyrillic, Greek, Arabic, Devanagari, Bengali, Ethiopic,
  and Chinese, Japanese and Korean). A language's own font goes first, so a whole line shapes as
  one run (egui shapes with harfrust: Arabic letters join, Devanagari and Bengali letters are
  built). Native builds carry them; the page carries them zlib-compressed and the game decodes
  only the ones its language needs (`pav_ui::fonts`, `pav_app::lang`). Players' names from the
  hub bring the fonts their scripts need.
- **Right to left.** In Arabic and Urdu `t!` lays a line out for egui's shaper (numbers and Latin
  names kept reading left to right inside it, `lines::for_rtl_shaper`), and the plaza's cards,
  menus and tool tiles mirror (`pav_ui::directed`, `pav_ui::text_job`).
- **The world's text.** Labels drawn in the 3D world (the engine tour's signs) use the renderer's
  own font and stay English: they belong to the engine's demo, not the game. The plaza's station
  names, welcome and cards are lines drawn by egui over the world (`pav_ui::plaza`), so the first
  screen follows the language.

## Testing
`?lang=en-XA` (accented, bracketed, 40% longer: plain text on screen is a line missing from the
book), `?lang=zh-XC` (Chinese characters, no spaces: boxes mean a missing font, text running off
a wrap that waits for spaces) and `?lang=ar-XB` (Arabic letters, right to left) are made from
the English lines. Headless: `pav lines lang=zh-XC`, then `ui open=title image=true` (or any
window) draws the UI in it. `cargo test -p pav_core lines` checks plural rules, number formats,
language matching, the test languages and the book's validity.
