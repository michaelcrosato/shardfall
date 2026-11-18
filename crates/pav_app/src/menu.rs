//! The pause menu, the same on every device: a column of big buttons that reads from across a
//! room, and a page for each thing behind them (settings, controls, difficulty, the engine's
//! tools, the developer's switches). Options, Esc or the corner button opens it. A controller
//! moves through it with the D-pad (`pav_ui::padnav`), takes with Cross and goes back with
//! Circle; the mouse points and clicks; a finger taps. Nothing here is a dense data screen:
//! one thing to a row, big enough to read.

use egui::{Color32, RichText};
use pav_core::t;
use pav_ui::Device;
use pav_ui::glyph::{self, Glyph, PadStyle};

use crate::ui::MenuAction;

/// The menu's pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Main,
    Settings,
    Controls,
    Difficulty,
    Tools,
    Developer,
}

/// What the menu shows and changes in place.
pub struct Menu<'a> {
    pub page: &'a mut Page,
    pub prefs: &'a mut crate::save::Prefs,
    pub show_fps: &'a mut bool,
    /// Shardfall's difficulty (None outside the game).
    pub difficulty: Option<&'a mut pav_core::arpg::Difficulty>,
    /// Somewhere a town portal can leave from.
    pub portal: bool,
    /// The scene's resolution now (a fraction of the screen's).
    pub scale: f32,
    pub device: Device,
    pub scene: &'a str,
    /// The page runs inside 52in1 (its own settings apply on top of these).
    pub in_hub: bool,
    /// The player's 52in1 settings (a DualSense's extras: motion aim, its speaker; the
    /// microphone, for Shout to roar).
    pub hub: Option<&'a crate::hub::Settings>,
}

const GOLD: Color32 = Color32::from_rgb(240, 198, 112);

/// Draws the menu; returns what the player chose.
pub fn menu(ctx: &egui::Context, m: Menu) -> Option<MenuAction> {
    let Menu { page, prefs, show_fps, difficulty, portal, scale, device, scene, in_hub, hub } = m;
    let mut action = None;
    let screen = ctx.content_rect();
    let w = (screen.width() - 32.0).min(460.0);
    let game = difficulty.is_some();
    let title = match *page {
        Page::Main => t!("menu.paused"),
        Page::Settings => t!("menu.settings"),
        Page::Controls => t!("menu.controls"),
        Page::Difficulty => t!("menu.difficulty"),
        Page::Tools => t!("menu.tools"),
        Page::Developer => t!("menu.developer"),
    };
    egui::Window::new("pause_menu")
        .title_bar(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .min_width(w)
        .max_width(w)
        .show(ctx, |ui| {
            ui.set_width(w);
            ui.vertical_centered(|ui| ui.label(RichText::new(title).size(26.0).color(GOLD)));
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(screen.height() - 120.0).show(ui, |ui| {
                ui.set_width(w);
                match *page {
                    Page::Main => main_page(ui, w, game, portal, &mut action),
                    Page::Settings => settings_page(ui, w, prefs, show_fps, scale, device, in_hub, hub, &mut action),
                    Page::Controls => controls_page(ui, device, prefs.pad_style, game),
                    Page::Difficulty => {
                        if let Some(d) = difficulty {
                            crate::arpg_ui::difficulty_ui(ui, d);
                            ui.add_space(8.0);
                            if big(ui, w, &t!("menu.new_hero")).on_hover_text(t!("menu.new_hero.about")).clicked() {
                                action = Some(MenuAction::NewHero);
                            }
                            ui.label(RichText::new(t!("menu.saved")).weak());
                        }
                    }
                    Page::Tools => {
                        if let Some(tool) = pav_ui::directed(ui, |ui| pav_ui::tools::tools_panel(ui, 2)) {
                            action = Some(MenuAction::Tool(tool));
                        }
                    }
                    Page::Developer => developer_page(ui, w, scene, game, &mut action),
                }
            });
            if *page != Page::Main {
                ui.add_space(6.0);
                if big(ui, w, &t!("menu.back")).clicked() {
                    *page = Page::Main;
                }
            }
        });
    if let Some(MenuAction::Page(p)) = action {
        *page = p;
        return None;
    }
    action
}

/// A full-width button, tall enough to hit from the sofa.
fn big(ui: &mut egui::Ui, w: f32, text: &str) -> egui::Response {
    ui.add_sized([w, 46.0], egui::Button::new(RichText::new(text).size(20.0)))
}

fn main_page(ui: &mut egui::Ui, w: f32, game: bool, portal: bool, action: &mut Option<MenuAction>) {
    let resume = egui::Button::new(RichText::new(t!("menu.resume")).size(22.0).strong().color(Color32::BLACK)).fill(GOLD);
    if ui.add_sized([w, 52.0], resume).clicked() {
        *action = Some(MenuAction::Resume);
    }
    if game && portal && big(ui, w, &t!("menu.town_portal")).clicked() {
        *action = Some(MenuAction::TownPortal);
    }
    for (label, page, shown) in [
        (t!("menu.settings"), Page::Settings, true),
        (t!("menu.controls"), Page::Controls, true),
        (t!("menu.difficulty"), Page::Difficulty, game),
        (t!("menu.tools"), Page::Tools, true),
    ] {
        if shown && big(ui, w, &label).clicked() {
            *action = Some(MenuAction::Page(page));
        }
    }
    if big(ui, w, &t!("menu.pavilion")).clicked() {
        *action = Some(MenuAction::Pavilion);
    }
    if big(ui, w, &t!("menu.developer")).clicked() {
        *action = Some(MenuAction::Page(Page::Developer));
    }
    if cfg!(not(target_arch = "wasm32")) && big(ui, w, &t!("menu.quit")).clicked() {
        *action = Some(MenuAction::Quit);
    }
}

/// A row: its name on the left, its choices on the right, one picked.
fn choice<T: Copy + PartialEq>(ui: &mut egui::Ui, w: f32, label: &str, value: &mut T, options: &[(T, String)]) -> bool {
    let mut changed = false;
    ui.label(RichText::new(label).size(18.0));
    ui.horizontal(|ui| {
        let n = options.len().max(1) as f32;
        let bw = (w - (n - 1.0) * ui.spacing().item_spacing.x) / n;
        for (v, name) in options {
            if ui.add_sized([bw, 40.0], egui::Button::new(RichText::new(name).size(18.0)).selected(*value == *v)).clicked() {
                *value = *v;
                changed = true;
            }
        }
    });
    ui.add_space(6.0);
    changed
}

#[allow(clippy::too_many_arguments)]
fn settings_page(
    ui: &mut egui::Ui,
    w: f32,
    prefs: &mut crate::save::Prefs,
    show_fps: &mut bool,
    scale: f32,
    device: Device,
    in_hub: bool,
    hub: Option<&crate::hub::Settings>,
    action: &mut Option<MenuAction>,
) {
    let on_off = || [(true, t!("settings.on")), (false, t!("settings.off"))];
    choice(
        ui,
        w,
        &t!("settings.prompts"),
        &mut prefs.pad_style,
        &[(PadStyle::PlayStation, t!("settings.prompts.playstation")), (PadStyle::Xbox, t!("settings.prompts.xbox"))],
    );
    choice(ui, w, &t!("settings.vibration"), &mut prefs.vibration, &on_off());
    dualsense_rows(ui, w, prefs, hub);
    voice_row(ui, w, prefs, hub);
    ui.label(RichText::new(t!("settings.effects")).size(18.0));
    let mut percent = (prefs.effects * 100.0).round();
    ui.spacing_mut().slider_width = w - 80.0;
    if ui.add(egui::Slider::new(&mut percent, 0.0..=100.0).step_by(10.0).suffix("%")).changed() {
        prefs.effects = percent / 100.0;
    }
    ui.add_space(6.0);
    choice(
        ui,
        w,
        &t!("settings.ui_size"),
        &mut prefs.ui_size,
        &[(0.85, t!("settings.ui_size.small")), (1.0, t!("settings.ui_size.medium")), (1.2, t!("settings.ui_size.large"))],
    );
    if device == Device::Touch {
        choice(ui, w, &t!("settings.auto_attack"), &mut prefs.auto_attack, &on_off());
    }
    let percent = (scale * scale * 100.0).round() as i64;
    let qualities: Vec<_> = crate::quality::Quality::ALL.iter().map(|q| (*q, q.name())).collect();
    choice(ui, w, &t!("menu.graphics", percent = percent), &mut prefs.quality, &qualities);
    choice(ui, w, &t!("menu.show_fps"), show_fps, &on_off());
    ui.horizontal(|ui| {
        let bw = (w - ui.spacing().item_spacing.x) / 2.0;
        if ui.add_sized([bw, 40.0], egui::Button::new(RichText::new(t!("title.language")).size(18.0))).clicked() {
            *action = Some(MenuAction::Language);
        }
        if cfg!(target_arch = "wasm32")
            && ui.add_sized([bw, 40.0], egui::Button::new(RichText::new(t!("title.fullscreen")).size(18.0))).clicked()
        {
            *action = Some(MenuAction::Fullscreen);
        }
    });
    if in_hub {
        ui.add_space(6.0);
        ui.label(RichText::new(t!("settings.hub")).weak());
    }
}

/// A DualSense in 52in1: the game's own settings for the extras 52in1 lets it use (motion aim
/// and its sensitivity, the controller's speaker). Each starts on when 52in1 allows it; one it
/// doesn't shows off, with a line saying where 52in1 allows it. Nothing without a DualSense
/// (or outside 52in1, natively).
fn dualsense_rows(ui: &mut egui::Ui, w: f32, prefs: &mut crate::save::Prefs, hub: Option<&crate::hub::Settings>) {
    if !hub.is_some_and(|h| h.dualsense.connected) {
        return;
    }
    let on_off = [(true, t!("settings.on")), (false, t!("settings.off"))];
    // Motion aim.
    let allowed = crate::motion::allowed(hub);
    let mut motion = allowed && crate::motion::chosen(hub, prefs.motion_aim);
    let changed = ui.add_enabled_ui(allowed, |ui| choice(ui, w, &t!("settings.motion"), &mut motion, &on_off)).inner;
    if changed {
        prefs.motion_aim = Some(motion);
    }
    if !allowed {
        note(ui, t!("settings.motion.hub_off"));
    } else {
        let sensitivities: Vec<_> = crate::motion::Sensitivity::ALL.iter().map(|s| (*s, s.name())).collect();
        ui.add_enabled_ui(motion, |ui| {
            choice(ui, w, &t!("settings.motion.sensitivity"), &mut prefs.motion_sensitivity, &sensitivities)
        });
        if !motion && hub.is_some_and(|h| h.dualsense.aim) {
            note(ui, t!("settings.motion.hub_aim"));
        }
    }
    // The controller's speaker.
    let allowed = crate::pad_fx::speaker_allowed(hub);
    let mut speaker = allowed && prefs.pad_speaker;
    let changed = ui.add_enabled_ui(allowed, |ui| choice(ui, w, &t!("settings.speaker"), &mut speaker, &on_off)).inner;
    if changed {
        prefs.pad_speaker = speaker;
    }
    if !allowed {
        note(ui, t!("settings.speaker.hub_off"));
    }
}

/// A line under a row: why it is off, or what it does.
fn note(ui: &mut egui::Ui, text: String) {
    ui.label(RichText::new(text).size(16.0).weak());
    ui.add_space(6.0);
}

/// In 52in1: Shout to roar (`voice`), on by default once 52in1 allows games the microphone.
/// Until then it shows off, greyed, with a line saying where 52in1 allows it (or, allowed after
/// the game opened, to open the game again). Any controller or the keys: no DualSense needed.
fn voice_row(ui: &mut egui::Ui, w: f32, prefs: &mut crate::save::Prefs, hub: Option<&crate::hub::Settings>) {
    use crate::voice::Mic;
    let Some(h) = hub else { return };
    let on_off = [(true, t!("settings.on")), (false, t!("settings.off"))];
    let mic = crate::voice::mic(hub);
    let mut on = crate::voice::on(hub, prefs.shout_roar);
    let changed = ui.add_enabled_ui(mic == Mic::Allowed, |ui| choice(ui, w, &t!("settings.roar"), &mut on, &on_off)).inner;
    if changed {
        prefs.shout_roar = Some(on);
    }
    let skill = || pav_core::arpg::lines::skill_name_of(&pav_core::arpg::data::data(), crate::voice::SKILL);
    note(
        ui,
        match mic {
            Mic::Allowed if on && h.muted => t!("settings.roar.muted"),
            Mic::Allowed => t!("settings.roar.about", skill = skill()),
            Mic::Later => t!("settings.roar.hub_later"),
            Mic::Off => t!("settings.roar.hub_off"),
        },
    );
}

/// The controls for the device in use, one action to a row, its glyphs drawn.
fn controls_page(ui: &mut egui::Ui, device: Device, style: PadStyle, game: bool) {
    let label = match device {
        Device::KeyboardMouse => t!("menu.controls.keyboard"),
        Device::Gamepad => t!("menu.controls.gamepad"),
        Device::Touch => t!("menu.controls.touch"),
    };
    ui.label(RichText::new(label).size(18.0).strong());
    if device == Device::Touch {
        for (a, k) in crate::input::touch_guide() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(a).size(17.0).strong());
                ui.label(RichText::new(k).size(17.0));
            });
        }
        return;
    }
    let rows = if game { crate::input::game_rows(device) } else { crate::input::room_rows(device) };
    let h = 26.0;
    egui::Grid::new("controls_grid").spacing([18.0, 10.0]).show(ui, |ui| {
        for (glyphs, what) in rows {
            glyph_cell(ui, &glyphs, h, style);
            ui.label(RichText::new(what).size(18.0));
            ui.end_row();
        }
    });
    if device == Device::KeyboardMouse {
        ui.add_space(6.0);
        ui.label(RichText::new(t!("dev.menu.system_keys")).weak());
    }
}

/// A cell holding glyphs side by side.
pub fn glyph_cell(ui: &mut egui::Ui, glyphs: &[Glyph], h: f32, style: PadStyle) {
    let w = glyph::width_all(ui.painter(), glyphs, h, style);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w.max(h), h), egui::Sense::hover());
    glyph::draw_all(ui.painter(), rect.left_center(), glyphs, h, style);
}

fn developer_page(ui: &mut egui::Ui, w: f32, scene: &str, game: bool, action: &mut Option<MenuAction>) {
    let row = |ui: &mut egui::Ui, items: &[(String, MenuAction)], action: &mut Option<MenuAction>| {
        ui.horizontal(|ui| {
            let n = items.len().max(1) as f32;
            let bw = (w - (n - 1.0) * ui.spacing().item_spacing.x) / n;
            for (label, a) in items {
                if ui.add_sized([bw, 40.0], egui::Button::new(RichText::new(label).size(17.0))).clicked() {
                    *action = Some(a.clone());
                }
            }
        });
    };
    row(ui, &[(t!("dev.menu.look"), MenuAction::Look), (t!("dev.menu.tuning"), MenuAction::Tuning)], action);
    row(ui, &[(t!("dev.menu.rooms"), MenuAction::Rooms), (t!("dev.menu.room_info"), MenuAction::RoomCard)], action);
    row(ui, &[(t!("dev.menu.feel"), MenuAction::Feel), (t!("dev.menu.physics"), MenuAction::Physics)], action);
    row(ui, &[(t!("dev.menu.reset_scene"), MenuAction::Reset), (t!("menu.screenshot"), MenuAction::Screenshot)], action);
    if !game {
        row(
            ui,
            &[(t!("dev.menu.how_it_works"), MenuAction::Guide), (t!("dev.menu.field_guide"), MenuAction::FieldGuide)],
            action,
        );
    }
    ui.add_space(6.0);
    ui.label(RichText::new(t!("dev.menu.load_scene.now", scene = scene)).size(17.0).strong());
    for (name, about) in pav_core::scenes::menu() {
        if ui.add_sized([w, 38.0], egui::Button::new(RichText::new(name).size(17.0))).on_hover_text(about).clicked() {
            *action = Some(MenuAction::LoadScene(name.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Settings page drawn as the window draws it (a few passes, as a window settles): the
    /// last pass's output, with every texture change since the first.
    fn settings(
        hub: Option<&crate::hub::Settings>,
        prefs: &mut crate::save::Prefs,
        w: f32,
        h: f32,
    ) -> (egui::Context, egui::FullOutput) {
        let ctx = egui::Context::default();
        pav_ui::fonts::install_embedded(&ctx, "en", &[]);
        pav_ui::game_style(&ctx, false, false);
        let zoom = pav_ui::ten_foot_zoom(h, prefs.ui_size, false);
        ctx.set_zoom_factor(zoom);
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let mut textures = egui::TexturesDelta::default();
        let mut out = None;
        for pass in 0..8 {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h) / zoom)),
                time: Some(pass as f64 / 60.0),
                predicted_dt: 1.0 / 60.0,
                ..Default::default()
            };
            let mut o = ctx.run_ui(raw, |ui| {
                let mut page = Page::Settings;
                let mut fps = false;
                let m = Menu {
                    page: &mut page,
                    prefs,
                    show_fps: &mut fps,
                    difficulty: None,
                    portal: false,
                    scale: 1.0,
                    device: Device::Gamepad,
                    scene: "arena",
                    in_hub: hub.is_some(),
                    hub,
                };
                menu(ui.ctx(), m);
            });
            textures.append(std::mem::take(&mut o.textures_delta));
            out = Some(o);
        }
        let mut out = out.unwrap();
        out.textures_delta = textures;
        (ctx, out)
    }

    /// Every text the Settings page drew.
    fn texts(hub: Option<&crate::hub::Settings>, prefs: &mut crate::save::Prefs) -> Vec<String> {
        fn walk(s: &egui::Shape, out: &mut Vec<String>) {
            match s {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        // Tall enough that the page needs no scrolling.
        let (_, mut o) = settings(hub, prefs, 1280.0, 2400.0);
        o.textures_delta.clear();
        let mut out = Vec::new();
        o.shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    fn hub(motion: bool, aim: bool, speaker: bool) -> crate::hub::Settings {
        let mut s = crate::hub::Settings { effects: 1.0, vibration: 1.0, ..Default::default() };
        s.dualsense = crate::hub::DualSense { connected: true, lightbar: true, motion, aim, speaker, ..Default::default() };
        s
    }

    #[test]
    fn dualsense_rows_follow_52in1() {
        let has = |t: &[String], id: &str| t.iter().any(|x| *x == pav_core::t!(id));
        // Natively (no hub), or no DualSense: no rows.
        let mut prefs = crate::save::Prefs::default();
        for h in [None, Some(crate::hub::Settings::default())] {
            let t = texts(h.as_ref(), &mut prefs);
            assert!(has(&t, "settings.vibration"), "{t:?}");
            assert!(!has(&t, "settings.motion") && !has(&t, "settings.speaker"));
        }
        // Everything allowed: the rows, and no line saying where 52in1 allows them.
        let all = hub(true, true, true);
        let t = texts(Some(&all), &mut prefs);
        for id in ["settings.motion", "settings.motion.sensitivity", "settings.motion.medium", "settings.speaker"] {
            assert!(has(&t, id), "{id}: {t:?}");
        }
        assert!(!has(&t, "settings.motion.hub_off") && !has(&t, "settings.speaker.hub_off"));
        // Neither allowed: both rows off, each with its line; no sensitivity.
        let none = hub(false, false, false);
        let t = texts(Some(&none), &mut prefs);
        assert!(has(&t, "settings.motion") && has(&t, "settings.motion.hub_off"));
        assert!(has(&t, "settings.speaker") && has(&t, "settings.speaker.hub_off"));
        assert!(!has(&t, "settings.motion.sensitivity"));
        // The game's own motion turned off while 52in1's Motion aim is on: the line says so.
        prefs.motion_aim = Some(false);
        let t = texts(Some(&all), &mut prefs);
        assert!(has(&t, "settings.motion.hub_aim"));
        // Drawing the page changes no preference.
        assert_eq!(prefs, crate::save::Prefs { motion_aim: Some(false), ..Default::default() });
    }

    #[test]
    fn the_roar_row_follows_52in1s_microphone() {
        let has = |t: &[String], id: &str| t.iter().any(|x| *x == pav_core::t!(id));
        let about = pav_core::t!(
            "settings.roar.about",
            skill = pav_core::arpg::lines::skill_name_of(&pav_core::arpg::data::data(), crate::voice::SKILL)
        );
        let lines = ["settings.roar.hub_off", "settings.roar.hub_later", "settings.roar.muted"];
        let only = |t: &[String], want: Option<&str>| {
            for id in lines {
                assert_eq!(has(t, id), want == Some(id), "{id}: {t:?}");
            }
            assert_eq!(t.contains(&about), want.is_none(), "about: {t:?}");
        };
        let mut prefs = crate::save::Prefs::default();
        // Natively (no hub): no row.
        let t = texts(None, &mut prefs);
        assert!(!has(&t, "settings.roar"));
        // In 52in1, with no DualSense: the row, off, and where 52in1 allows the microphone.
        let mut h = crate::hub::Settings { effects: 1.0, vibration: 1.0, ..Default::default() };
        let t = texts(Some(&h), &mut prefs);
        assert!(has(&t, "settings.roar") && !has(&t, "settings.motion"));
        only(&t, Some("settings.roar.hub_off"));
        // Allowed after the game opened: open it again.
        h.mic_later = true;
        only(&texts(Some(&h), &mut prefs), Some("settings.roar.hub_later"));
        // Allowed: what it does.
        h.mic_later = false;
        h.mic = true;
        let t = texts(Some(&h), &mut prefs);
        assert!(has(&t, "settings.roar"));
        only(&t, None);
        // Muted (the DualSense's mute button): says so.
        h.muted = true;
        only(&texts(Some(&h), &mut prefs), Some("settings.roar.muted"));
        // Turned off by the player: what it does again, muted or not.
        prefs.shout_roar = Some(false);
        only(&texts(Some(&h), &mut prefs), None);
        // Drawing the page changes no preference.
        assert_eq!(prefs, crate::save::Prefs { shout_roar: Some(false), ..Default::default() });
    }

    /// The Settings page on a 1080p TV, as PNGs (lavapipe will do): the rows with everything
    /// allowed, and with nothing allowed. `MENU_CAPTURE_DIR` picks the folder (else the
    /// workspace's `target/menu`).
    #[test]
    #[ignore = "needs a GPU or lavapipe: cargo test -p pav_app settings_page_capture -- --ignored"]
    fn settings_page_capture() {
        let dir = std::env::var("MENU_CAPTURE_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/menu"));
        std::fs::create_dir_all(&dir).unwrap();
        let gpu = pav_render::gpu::Headless::new().unwrap();
        let mut renderer = pav_render::Renderer::new(&gpu.device, &gpu.queue);
        let scene = pav_render::Scene::default();
        let (w, h) = (1920u32, 1080u32);
        let mut overlay = pav_ui::headless::Overlay::new(&gpu.device, pav_render::capture::CAPTURE_FORMAT);
        for (name, hub) in [("allowed", hub(true, true, true)), ("not_allowed", hub(false, false, false))] {
            let mut prefs = crate::save::Prefs::default();
            let (ctx, out) = settings(Some(&hub), &mut prefs, w as f32, h as f32);
            let mut paint = pav_ui::headless::Paint {
                primitives: ctx.tessellate(out.shapes, out.pixels_per_point),
                textures: out.textures_delta,
                pixels_per_point: out.pixels_per_point,
            };
            let rgba = pav_render::capture::render_to_rgba_with(&mut renderer, &scene, w, h, |device, queue, encoder, target| {
                overlay.paint(device, queue, encoder, target, (w, h), &paint)
            })
            .unwrap();
            paint.textures.clear();
            let path = dir.join(format!("settings_{name}.png"));
            pav_render::capture::save_png(&path, w, h, &rgba).unwrap();
            println!("{}", path.display());
        }
    }
}
