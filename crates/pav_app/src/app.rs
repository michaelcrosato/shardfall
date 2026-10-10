//! The game window: owns graphics, UI, input, the simulation thread and the camera.

use std::sync::Arc;

use web_time::Instant;

use anyhow::{Result, anyhow};
use glam::{Mat4, Quat, Vec2, Vec3};
use pav_core::params::{ParamVisitor, Tunable, nested};
use pav_core::{InputFrame, Sim, SimConfig};
use pav_render::gpu::BackendChoice;
use pav_render::scene::{MeshInstance, MeshKey, Style, flags as rflags};
use pav_view::{CameraParams, CameraRig, ViewBuilder, ViewSettings};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::boot::{self, stage};
use crate::edit::Editor;
use crate::gfx::Gfx;
use crate::hud::{CameraDirector, FeelOverlay, FeelSettings, HudCtx, Latency, LatencySample, PhysicsOverlay};
use crate::input::{Device, Input};
use crate::panel::{Panel, PanelAction};
use crate::rooms::{RoomEntry, RoomHud, RoomWatcher, TeleportTarget};
use crate::settings::Settings;
use crate::simhost::SimHost;
use crate::ui::{self, MenuAction};
use crate::uiinput::UiInput;

#[cfg(not(target_arch = "wasm32"))]
pub fn run(settings: Settings) -> Result<()> {
    let event_loop = stage("event loop", || Ok((EventLoop::new()?, String::new())))?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(settings);
    event_loop.run_app(&mut app)?;
    match app.fatal.take() {
        Some(e) => Err(anyhow!(e)),
        None => Ok(()),
    }
}

/// In the browser the event loop runs from the page's animation frames; this returns at once.
#[cfg(target_arch = "wasm32")]
pub fn run(settings: Settings) -> Result<()> {
    use winit::platform::web::EventLoopExtWebSys;
    let event_loop = stage("event loop", || Ok((EventLoop::new()?, String::new())))?;
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.spawn_app(App::new(settings));
    Ok(())
}

/// App-level tunables (the "app" group in the panel).
#[derive(Clone, Debug, PartialEq)]
pub struct AppSettings {
    pub vsync: bool,
    pub smoothing: bool,
    pub show_stats: bool,
    pub show_guide: bool,
    pub aim_marker: bool,
}

impl Tunable for AppSettings {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.bool("vsync", &mut self.vsync, "Wait for the display refresh (off = lowest latency, may tear)");
        v.bool("smoothing", &mut self.smoothing, "Interpolate between simulation ticks");
        v.bool("show_stats", &mut self.show_stats, "FPS / tick overlay");
        v.bool("show_guide", &mut self.show_guide, "Control guide overlay");
        v.bool("aim_marker", &mut self.aim_marker, "Show where bombs will land");
    }
}

/// Sound settings (the "audio" group).
#[derive(Clone, Debug, PartialEq)]
pub struct AudioSettings {
    pub master: f32,
    pub sfx: f32,
    pub footsteps: bool,
}

impl Tunable for AudioSettings {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.float("master", &mut self.master, 0.0, 2.0, "Master volume");
        v.float("sfx", &mut self.sfx, 0.0, 2.0, "Sound effects volume");
        v.bool("footsteps", &mut self.footsteps, "Footstep sounds");
    }
}

struct Root<'a> {
    sim: &'a mut SimConfig,
    camera: &'a mut CameraParams,
    view: &'a mut ViewSettings,
    app: &'a mut AppSettings,
    audio: &'a mut AudioSettings,
}

impl Tunable for Root<'_> {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        self.sim.visit_groups(v);
        nested(v, "camera", self.camera);
        nested(v, "view", self.view);
        nested(v, "audio", self.audio);
        nested(v, "app", self.app);
    }
}

#[derive(Default)]
struct Mouse {
    right_down: bool,
    pos: Vec2,
}

struct Fps {
    frames: u32,
    since: Instant,
    fps: f32,
    frame_ms: f32,
}

pub struct App {
    settings: Settings,
    gfx: Option<Gfx>,
    egui_ctx: egui::Context,
    egui_state: Option<UiInput>,
    host: Option<SimHost>,
    rig: CameraRig,
    view: ViewSettings,
    /// View settings before the room's `[view]` table and pad overrides were applied, and
    /// which room / pad state they belong to.
    view_base: Option<ViewSettings>,
    view_key: (Option<u16>, u64),
    sim_config: SimConfig,
    app_settings: AppSettings,
    builder: ViewBuilder,
    input: Input,
    panel: Panel,
    menu_open: bool,
    paused_before_menu: bool,
    show_boot: bool,
    boot_done_at: Option<Instant>,
    started: Instant,
    last: Instant,
    fps: Fps,
    mouse: Mouse,
    screenshot_requested: bool,
    scene_name: String,
    toast: Option<(String, Instant)>,
    hud: RoomHud,
    watcher: Option<RoomWatcher>,
    editor: Editor,
    audio: Option<pav_audio::AudioOut>,
    audio_settings: AudioSettings,
    /// Tick at which the app last pushed its config (the sim's copy is adopted after that).
    config_push_tick: u64,
    latency: Latency,
    director: CameraDirector,
    feel: FeelOverlay,
    physics: PhysicsOverlay,
    quit: bool,
    /// Camera and look from before the game started (restored when leaving it).
    game_saved: Option<(CameraParams, ViewSettings)>,
    /// The Look & Filters window and the look layer over `view`.
    look_ui: crate::look_ui::LookUi,
    animation_ui: crate::animation_ui::AnimationUi,
    animation_was_open: bool,
    animation_watcher: Option<crate::animation_watch::AnimationWatcher>,
    animation_watch_attempted: bool,
    /// Station guide (H) and field guide.
    guide: crate::guide_ui::GuideUi,
    /// When the player last arrived somewhere new (another place, a teleport): the screen
    /// transition opens from then.
    arrival: Option<Instant>,
    /// Shardfall windows (inventory, vendor...), the latest game frame and the place whose
    /// look is applied.
    game_ui: crate::arpg_items::GameUi,
    /// When the hero was last saved.
    last_save: Instant,
    /// The gamepad's menu cursor (egui points) and the pointer events it makes this frame.
    pad_cursor: Option<Vec2>,
    pad_events: Vec<egui::Event>,
    pad_buttons: (bool, bool, bool),
    game_frame: Option<std::sync::Arc<pav_core::arpg::GameFrame>>,
    game_place: Option<pav_core::arpg::Place>,
    /// Phones and tablets: fingers, the on-screen controls and swipes (touch.rs).
    touch: crate::touch::Touch,
    /// The browser says this is a touch screen (or a finger has touched it).
    touch_screen: bool,
    /// The egui style has the touch sizes.
    touch_styled: bool,
    /// Touch screens draw at most 60 frames a second: when the next one is due.
    next_draw: Instant,
    /// Auto-attack, graphics quality, tips seen (saved).
    prefs: crate::save::Prefs,
    scaler: crate::quality::Scaler,
    #[cfg(not(target_arch = "wasm32"))]
    bridge: Option<crate::bridge::Bridge>,
    #[cfg(target_arch = "wasm32")]
    pending_gfx: Option<std::rc::Rc<std::cell::RefCell<Option<Result<Gfx>>>>>,
    #[cfg(target_arch = "wasm32")]
    audio_resumed: bool,
    init_started: bool,
    pub fatal: Option<String>,
}

impl App {
    fn new(settings: Settings) -> Self {
        let scene_name = settings.scene.clone();
        let touch_screen = crate::platform::touch_screen();
        let mut app = Self {
            gfx: None,
            egui_ctx: egui::Context::default(),
            egui_state: None,
            host: None,
            rig: CameraRig::default(),
            view: ViewSettings::default(),
            view_base: None,
            view_key: (None, 0),
            sim_config: SimConfig::default(),
            app_settings: AppSettings {
                vsync: settings.vsync,
                smoothing: true,
                show_stats: true,
                show_guide: true,
                aim_marker: true,
            },
            builder: ViewBuilder::new(),
            input: Input::new(),
            panel: Panel::new(),
            menu_open: false,
            paused_before_menu: false,
            show_boot: true,
            boot_done_at: None,
            started: Instant::now(),
            last: Instant::now(),
            fps: Fps { frames: 0, since: Instant::now(), fps: 0.0, frame_ms: 0.0 },
            mouse: Mouse::default(),
            screenshot_requested: false,
            scene_name,
            toast: None,
            hud: RoomHud::default(),
            watcher: None,
            editor: Editor::default(),
            audio: None,
            audio_settings: AudioSettings { master: 0.8, sfx: 1.0, footsteps: true },
            config_push_tick: 0,
            latency: Latency::default(),
            director: CameraDirector::default(),
            feel: FeelOverlay::default(),
            physics: PhysicsOverlay::default(),
            quit: false,
            game_saved: None,
            look_ui: crate::look_ui::LookUi::load(),
            animation_ui: crate::animation_ui::AnimationUi::new(settings.animation_studio),
            animation_was_open: false,
            animation_watcher: None,
            animation_watch_attempted: false,
            guide: Default::default(),
            arrival: None,
            game_ui: Default::default(),
            last_save: Instant::now(),
            pad_cursor: None,
            pad_events: Vec::new(),
            pad_buttons: (false, false, false),
            game_frame: None,
            game_place: None,
            touch: Default::default(),
            touch_screen,
            touch_styled: false,
            next_draw: Instant::now(),
            prefs: crate::save::load_prefs(),
            scaler: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            bridge: None,
            #[cfg(target_arch = "wasm32")]
            pending_gfx: None,
            #[cfg(target_arch = "wasm32")]
            audio_resumed: false,
            init_started: false,
            fatal: None,
            settings,
        };
        if touch_screen {
            // Phones: touch controls from the start, none of the desktop's diagnostics.
            app.input.last_device = Device::Touch;
            app.show_boot = false;
            app.app_settings.show_stats = false;
        }
        app
    }

    /// A finger on the screen: the touch layer decides whether it is the stick, a control, a
    /// swipe or egui's (and then egui gets the event).
    fn on_touch(&mut self, t: &winit::event::Touch, event: &WindowEvent) {
        #[cfg(target_arch = "wasm32")]
        self.resume_audio();
        let Some(g) = &self.gfx else { return };
        let ppp = self.egui_ctx.zoom_factor() * g.window.scale_factor() as f32;
        let pos = egui::pos2(t.location.x as f32 / ppp, t.location.y as f32 / ppp);
        self.input.last_device = Device::Touch;
        self.touch_screen = true;
        use winit::event::TouchPhase as P;
        let to_egui = match t.phase {
            P::Started => {
                // (The root layer is everywhere; only windows and labels over the game count.)
                let under = self.egui_ctx.layer_id_at(pos).filter(|l| *l != egui::LayerId::background()).map(|l| l.order);
                self.touch.down(t.id, pos, under)
            }
            P::Moved => self.touch.moved(t.id, pos),
            P::Ended => self.touch.up(t.id, pos, false),
            P::Cancelled => self.touch.up(t.id, pos, true),
        };
        if to_egui {
            if let (Some(st), Some(g)) = (&mut self.egui_state, &self.gfx) {
                st.on_window_event(&g.window, event);
            }
        }
    }

    /// A tap on the open game closes what is open over it, one thing at a time.
    fn close_on_tap(&mut self) {
        if self.menu_open {
            self.set_menu(false);
        } else if self.game_ui.any_open() {
            self.game_ui.close_all();
        } else if self.game_ui.map {
            self.game_ui.map = false;
        } else if self.look_ui.open {
            self.look_ui.open = false;
            self.look_ui.save_if_dirty(true);
        } else if self.guide.station || self.guide.field {
            self.guide.station = false;
            self.guide.field = false;
        } else if self.hud.teleport_open {
            self.hud.teleport_open = false;
        } else if self.hud.card_visible() {
            self.hud.hide_card();
        }
    }

    fn init(&mut self, el: &ActiveEventLoop) -> Result<()> {
        let s = self.settings.clone();
        let window = stage("window", || {
            let mut attrs = Window::default_attributes().with_title("Shardfall");
            #[cfg(not(target_arch = "wasm32"))]
            {
                attrs = attrs.with_inner_size(winit::dpi::LogicalSize::new(s.width, s.height));
            }
            // In the browser the window is a canvas filling the page (sized by the page's CSS).
            #[cfg(target_arch = "wasm32")]
            {
                use winit::platform::web::WindowAttributesExtWebSys;
                attrs = attrs.with_append(true);
            }
            if s.fullscreen {
                attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
            }
            let w = Arc::new(el.create_window(attrs)?);
            let sz = w.inner_size();
            let d = format!("{}x{} px, scale {:.2}", sz.width, sz.height, w.scale_factor());
            Ok((w, d))
        })?;
        let backend = BackendChoice::parse(&s.backend)
            .ok_or_else(|| anyhow!("unknown backend '{}' in settings (use vulkan or dx12)", s.backend))?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let gfx = Gfx::new(window, backend, s.vsync)?;
            self.finish_init(gfx)
        }
        // The browser hands out the GPU asynchronously: `about_to_wait` finishes the start.
        #[cfg(target_arch = "wasm32")]
        {
            let slot = std::rc::Rc::new(std::cell::RefCell::new(None));
            let fill = slot.clone();
            wasm_bindgen_futures::spawn_local(async move {
                *fill.borrow_mut() = Some(Gfx::create(window, backend, s.vsync).await);
            });
            self.pending_gfx = Some(slot);
            Ok(())
        }
    }

    /// The rest of the start, once graphics exist.
    fn finish_init(&mut self, gfx: Gfx) -> Result<()> {
        let s = self.settings.clone();
        let window = gfx.window.clone();
        let egui_state = stage("ui", || {
            let st = UiInput::new(&self.egui_ctx, &window, gfx.device.limits().max_texture_dimension_2d as usize);
            Ok((st, String::new()))
        })?;
        self.audio = stage("audio", || match pav_audio::AudioOut::start() {
            Ok(a) => {
                let d = format!("{} @ {} Hz", a.device, a.sample_rate);
                Ok((Some(a), d))
            }
            Err(e) => Ok((None, format!("no sound ({e}); continuing silently"))),
        })?;
        let mut sim = stage("simulation", || {
            let mut sim = Sim::new(&s.scene, s.seed)?;
            let mut d = format!(
                "scene '{}', seed {}, {} entities, {} blocks",
                s.scene,
                s.seed,
                sim.state.entities.len(),
                sim.state.statics.block_count()
            );
            if let Some(level) = crate::save::restore_into(&mut sim) {
                d += &format!(", saved hero (level {level})");
            }
            Ok((sim, d))
        })?;
        if s.animation_studio {
            // Read authored sets before selecting a clip. Keep the embedded library if a
            // file is invalid, so the studio can still open and report the error.
            #[cfg(not(target_arch = "wasm32"))]
            {
                let _ = pav_core::clips::library();
                if pav_core::anim::disk_dir().is_some() {
                    if let Err(e) = pav_core::anim::reload(true) {
                        self.animation_ui.report(Err(e));
                    }
                }
                if let Err(e) = pav_tools::animation_tools::reload_authored() {
                    self.animation_ui.report(Err(format!("{e:#}")));
                }
            }
            let name = if s.animation_clip.is_empty() { "QUATERNIUS/Idle_Loop" } else { &s.animation_clip };
            let mut session = pav_tools::Session::from_live(sim, self.rig.clone(), self.view.clone(), None);
            let args = serde_json::json!({"clip":name}).as_object().unwrap().clone();
            pav_tools::preview_tools::t_anim_preview(&mut session, &args)?;
            let (live, rig, view, _) = session.into_live();
            sim = live;
            self.rig = rig;
            self.view = view;
            self.show_boot = false;
            self.app_settings.show_stats = false;
            self.app_settings.show_guide = false;
        }
        self.sim_config = sim.config.clone();
        self.rig.snap(sim.state.focus);
        self.hud.entries = room_entries(&sim);
        self.hud.errors = sim.state.world.errors.clone();
        self.watcher = RoomWatcher::start();
        self.animation_watch_attempted = crate::animation_watch::AnimationWatcher::directory_exists();
        if self.animation_watch_attempted {
            self.animation_watcher = crate::animation_watch::AnimationWatcher::start();
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !s.pad_script.is_empty() {
            match std::fs::read_to_string(&s.pad_script)
                .map_err(|e| e.to_string())
                .and_then(|t| crate::input::PadScript::parse(&t))
            {
                Ok(p) => {
                    log::info!("pad script: {} steps from {}", p.steps.len(), s.pad_script);
                    self.input.pad_script = Some(p);
                }
                Err(e) => log::error!("pad script {}: {e}", s.pad_script),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !s.bridge.is_empty() {
            self.bridge = stage("agent bridge", || match crate::bridge::Bridge::start(&s.bridge) {
                Ok(b) => {
                    let d = format!("listening on {}", b.addr);
                    Ok((Some(b), d))
                }
                Err(e) => Ok((None, format!("could not listen on {} ({e}); continuing without it", s.bridge))),
            })?;
        }
        self.host = Some(SimHost::start(sim));
        self.gfx = Some(gfx);
        self.egui_state = Some(egui_state);
        Ok(())
    }

    /// Whether the gamepad drives a menu cursor now (a game window or the pause menu is open).
    fn pad_ui_active(&self) -> bool {
        self.input.last_device == Device::Gamepad
            && (self.menu_open || self.game_ui.any_open() || self.look_ui.open || self.guide.station || self.guide.field)
    }

    /// Gamepad in menus: D-pad down opens the hero's panels (LB / RB switch them), B closes;
    /// while a window is open the left stick (or D-pad) moves a cursor, A and X click left and
    /// right, the right stick scrolls, and the game gets no pad input.
    fn pad_menus(&mut self, dt: f32) {
        use gilrs::Button as B;
        let taps = std::mem::take(&mut self.input.pad.taps);
        let game = self.game_frame.is_some();
        if self.input.last_device != Device::Gamepad {
            self.pad_cursor = None;
            return;
        }
        let ui = &mut self.game_ui;
        if game && !self.menu_open && taps.contains(&B::DPadDown) && !ui.any_open() {
            ui.inventory = true;
            return;
        }
        if !self.pad_ui_active() {
            self.pad_cursor = None;
            if self.pad_buttons.2 {
                self.pad_events.push(egui::Event::ModifiersChanged(Default::default()));
            }
            self.pad_buttons = (false, false, false);
            return;
        }
        let ui = &mut self.game_ui;
        // Switch between the hero's panels.
        let cycle = taps.contains(&B::RightTrigger) as i32 - taps.contains(&B::LeftTrigger) as i32;
        if cycle != 0 && game && !self.menu_open {
            let now = [ui.inventory, ui.character, ui.skills, ui.tree.open].iter().position(|x| *x).unwrap_or(0) as i32;
            let next = (now + cycle).rem_euclid(4);
            ui.inventory = next == 0;
            ui.character = next == 1;
            ui.skills = next == 2;
            ui.tree.open = next == 3;
        }
        if taps.contains(&B::East) {
            if ui.any_open() {
                ui.close_all();
            } else if self.look_ui.open && !self.menu_open {
                self.look_ui.open = false;
                self.look_ui.save_if_dirty(true);
            } else if (self.guide.station || self.guide.field) && !self.menu_open {
                self.guide.station = false;
                self.guide.field = false;
            } else {
                self.set_menu(false);
            }
        }
        // The cursor.
        let screen = self.egui_ctx.content_rect();
        let mut p = self.pad_cursor.unwrap_or(Vec2::new(screen.center().x, screen.center().y));
        let stick = self.input.pad.left;
        let speed = 260.0 + 900.0 * stick.length().powi(2);
        p += Vec2::new(stick.x, -stick.y) * speed * dt;
        for (b, d) in [(B::DPadLeft, Vec2::NEG_X), (B::DPadRight, Vec2::X), (B::DPadUp, Vec2::NEG_Y), (B::DPadDown, Vec2::Y)] {
            if taps.contains(&b) {
                p += d * 40.0;
            }
        }
        p = p.clamp(Vec2::new(screen.min.x, screen.min.y), Vec2::new(screen.max.x - 2.0, screen.max.y - 2.0));
        let pos = egui::pos2(p.x, p.y);
        self.pad_events.push(egui::Event::PointerMoved(pos));
        let (a, x) = (self.input.pad.a_held, self.input.pad.x_held);
        let modifiers = egui::Modifiers { shift: self.input.pad.y_held, ..Default::default() };
        if self.input.pad.y_held != self.pad_buttons.2 {
            self.pad_events.push(egui::Event::ModifiersChanged(modifiers));
        }
        for (now, was, button) in
            [(a, self.pad_buttons.0, egui::PointerButton::Primary), (x, self.pad_buttons.1, egui::PointerButton::Secondary)]
        {
            if now != was {
                self.pad_events.push(egui::Event::PointerButton { pos, button, pressed: now, modifiers });
            }
        }
        self.pad_buttons = (a, x, self.input.pad.y_held);
        let scroll = self.input.pad.right.y;
        if scroll.abs() > 0.1 {
            self.pad_events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0.0, scroll * 900.0 * dt),
                modifiers: Default::default(),
            });
        }
        self.pad_cursor = Some(p);
        // Menus have the pad: nothing reaches the game.
        let pad = &mut self.input.pad;
        pad.left = Vec2::ZERO;
        pad.right = Vec2::ZERO;
        pad.held = 0;
        pad.zoom = 0.0;
        pad.rotate = 0.0;
    }

    fn toast(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        log::info!("{m}");
        self.toast = Some((m, Instant::now()));
    }

    fn load_scene(&mut self, name: &str) {
        let Some(host) = &self.host else { return };
        let (scene, seed) = (name.to_string(), self.settings.seed);
        let cfg = self.sim_config.clone();
        host.exec(move |sim| {
            // Keep the hero: save, build the scene, put them back in.
            crate::save::save_from(sim);
            match Sim::new(&scene, seed) {
                Ok(mut fresh) => {
                    fresh.config = cfg;
                    crate::save::restore_into(&mut fresh);
                    *sim = fresh;
                }
                Err(e) => log::error!("could not load scene: {e:#}"),
            }
        });
        self.scene_name = name.to_string();
        self.rig.params = CameraParams { yaw: self.rig.params.yaw, ..self.rig.params.clone() };
        self.toast(format!("loaded {}", self.scene_name));
    }

    /// Resets the whole scene (menu).
    fn reset(&mut self) {
        if let Some(host) = &self.host {
            let cfg = self.sim_config.clone();
            host.exec(move |sim| {
                sim.config = cfg;
                crate::save::save_from(sim);
                if let Err(e) = sim.reset() {
                    log::error!("reset failed: {e:#}");
                }
                crate::save::restore_into(sim);
            });
        }
        self.toast("scene reset");
    }

    /// Resets the room the player is in (F5).
    fn reset_room(&mut self) {
        let Some(host) = &self.host else { return };
        match &self.hud.current {
            Some(r) => {
                let id = r.id;
                host.exec(move |sim| sim.reset_room(id));
                let name = r.def.name.clone();
                self.toast(format!("reset {name}"));
            }
            None => self.toast("Not in a room (Esc, Reset scene resets everything)"),
        }
    }

    fn teleport(&mut self, t: TeleportTarget) {
        let Some(host) = &self.host else { return };
        self.arrival = Some(Instant::now());
        let msg = match t {
            TeleportTarget::Room(key) => {
                let k = key.clone();
                let ok = host.query(move |sim| sim.teleport_to_room(&k)).unwrap_or(false);
                if ok { format!("went to {key}") } else { format!("could not go to {key}") }
            }
            TeleportTarget::Hub => {
                host.exec(|sim| {
                    if let Some(pid) = sim.state.player {
                        sim.set_position(pid, Vec3::new(0.0, 0.0, 6.0));
                    }
                });
                "went to the plaza".into()
            }
            TeleportTarget::Wilderness => {
                let a = self.started.elapsed().as_secs_f32() * 7.3;
                let p = Vec3::new(a.cos(), 0.0, a.sin()) * 220.0 + Vec3::Y * 25.0;
                host.exec(move |sim| {
                    sim.state.world.interest.push(p);
                    sim.update_streaming(usize::MAX);
                    sim.state.world.interest.pop();
                    if let Some(pid) = sim.state.player {
                        sim.set_position(pid, p);
                    }
                });
                "went to the wilderness".into()
            }
        };
        self.toast(msg);
    }

    fn open_teleport(&mut self) {
        if let Some(host) = &self.host {
            if let Some(e) = host.query(|sim| room_entries(sim)) {
                self.hud.entries = e;
            }
        }
        self.hud.teleport_open = true;
    }

    /// Hot reload: re-read room files and rebuild what changed.
    fn reload_rooms(&mut self) {
        let Some(host) = &self.host else { return };
        let dir = self.watcher.as_ref().map(|w| w.dir.clone());
        let sources = pav_core::room::load_sources(dir.as_deref());
        let (defs, errors) = pav_core::room::parse_all(&sources);
        self.hud.errors = errors.iter().map(|(k, e)| format!("{k}: {e}")).collect();
        for e in &self.hud.errors {
            log::error!("room file: {e}");
        }
        let bad: Vec<String> = errors.into_iter().map(|(k, _)| k).collect();
        let changed = host
            .query(move |sim| {
                // Rooms whose file is broken keep their current definition.
                let mut defs = defs;
                for k in &bad {
                    if let Some(r) = sim.state.world.room(k) {
                        defs.push((k.clone(), (*r.def).clone()));
                    }
                }
                pav_tools::tools::reload_rooms(sim, defs)
            })
            .unwrap_or(0);
        if self.hud.errors.is_empty() {
            self.toast(format!("rooms reloaded ({changed} changed)"));
        }
    }

    fn set_menu(&mut self, open: bool) {
        let Some(host) = &self.host else { return };
        if open && !self.menu_open {
            self.paused_before_menu = host.control().paused;
            host.set_control(|c| c.paused = true);
        } else if !open && self.menu_open {
            let p = self.paused_before_menu;
            host.set_control(|c| c.paused = p);
        }
        self.menu_open = open;
        self.input.clear();
    }

    /// The fixed system layer: never rebinds, works in every room.
    fn handle_key(&mut self, code: KeyCode) {
        let Some(host) = &self.host else { return };
        if self.animation_was_open {
            if let Some(p) = host.frames().1.animation_preview.as_ref() {
                let args = match code {
                    KeyCode::F6 => Some(serde_json::json!({"playing": !p.playing})),
                    KeyCode::F7 => Some(serde_json::json!({"step": 1})),
                    KeyCode::F8 => Some(serde_json::json!({"speed": (p.speed * 0.5).max(0.05)})),
                    KeyCode::F9 => Some(serde_json::json!({"speed": (p.speed * 2.0).min(8.0)})),
                    _ => None,
                };
                if let Some(args) = args {
                    self.animation_command(crate::animation_ui::Command {
                        tool: "anim_preview",
                        args: args.as_object().unwrap().clone(),
                    });
                    return;
                }
            }
        }
        // Shardfall's menu keys.
        if self.input.game && !self.menu_open && !self.animation_was_open {
            let ui = &mut self.game_ui;
            match code {
                KeyCode::KeyI | KeyCode::Tab => {
                    ui.inventory = !ui.inventory;
                    return;
                }
                KeyCode::KeyC => {
                    ui.character = !ui.character;
                    return;
                }
                KeyCode::KeyK => {
                    ui.skills = !ui.skills;
                    return;
                }
                KeyCode::KeyP => {
                    ui.tree.open = !ui.tree.open;
                    return;
                }
                KeyCode::KeyM => {
                    ui.map = !ui.map;
                    return;
                }
                KeyCode::KeyT => {
                    // Town portal: home from anywhere.
                    if self.game_frame.as_ref().is_some_and(|g| g.place != pav_core::arpg::Place::Town) {
                        host.shared.command(pav_core::arpg::GameCmd::Travel(pav_core::arpg::Place::Town.code()));
                    }
                    return;
                }
                KeyCode::Escape if ui.any_open() => {
                    ui.close_all();
                    return;
                }
                _ => {}
            }
        }
        match code {
            KeyCode::Escape => {
                let open = !self.menu_open;
                self.set_menu(open);
            }
            KeyCode::F1 => self.panel.open = !self.panel.open,
            // How the room you are in works (outside the rooms: the field guide).
            KeyCode::KeyH if !self.input.game => {
                if self.hud.current.is_some() {
                    self.guide.station = !self.guide.station;
                } else {
                    self.guide.field = !self.guide.field;
                }
            }
            KeyCode::F2 => {
                if self.hud.teleport_open {
                    self.hud.teleport_open = false;
                } else {
                    self.open_teleport();
                }
            }
            KeyCode::F3 => self.show_boot = !self.show_boot,
            KeyCode::F4 => {
                host.exec(|sim| {
                    sim.leave_room();
                });
                self.toast("left the room");
            }
            KeyCode::F5 => self.reset_room(),
            KeyCode::F6 => host.set_control(|c| c.paused = !c.paused),
            KeyCode::F7 => host.set_control(|c| {
                c.paused = true;
                c.step_requests += 1;
            }),
            KeyCode::F8 => host.set_control(|c| c.speed = (c.speed * 0.5).max(0.0625)),
            KeyCode::F9 => host.set_control(|c| c.speed = (c.speed * 2.0).min(8.0)),
            KeyCode::F10 if !self.animation_was_open => {
                self.editor.on = !self.editor.on;
                self.editor.dragging = None;
                let on = self.editor.on;
                self.toast(if on { "edit mode on" } else { "edit mode off" });
            }
            KeyCode::F11 => {
                if let Some(g) = &self.gfx {
                    let fs = g.window.fullscreen().is_some();
                    g.window.set_fullscreen(if fs { None } else { Some(Fullscreen::Borderless(None)) });
                }
            }
            KeyCode::F12 => self.screenshot_requested = true,
            _ => {
                let digits = [
                    KeyCode::Digit1,
                    KeyCode::Digit2,
                    KeyCode::Digit3,
                    KeyCode::Digit4,
                    KeyCode::Digit5,
                    KeyCode::Digit6,
                    KeyCode::Digit7,
                    KeyCode::Digit8,
                ];
                // (In Shardfall the number keys are potions and belt items.)
                let digit =
                    digits.iter().position(|k| *k == code).filter(|i| *i < CameraParams::PRESETS.len() && !self.input.game);
                if let Some(i) = digit {
                    let yaw = self.rig.params.yaw;
                    self.rig.params = (CameraParams::PRESETS[i].1)();
                    if CameraParams::PRESETS[i].0 != "isometric" {
                        self.rig.params.yaw = yaw;
                    }
                    self.toast(format!("camera: {}", CameraParams::PRESETS[i].0));
                }
            }
        }
    }

    fn frame(&mut self) {
        if self.gfx.is_none() || self.host.is_none() {
            return;
        }
        // Touch screens draw at most 60 frames a second (a phone with a faster screen would
        // spend battery and heat on the rest).
        if self.touch_screen {
            let now = Instant::now();
            let step = std::time::Duration::from_micros(16_667);
            if now + std::time::Duration::from_micros(2_500) < self.next_draw {
                return;
            }
            self.next_draw = if now > self.next_draw + step { now + step } else { self.next_draw + step };
        }
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        self.fps.frames += 1;
        let since = self.fps.since.elapsed().as_secs_f32();
        if since >= 0.5 {
            self.fps.fps = self.fps.frames as f32 / since;
            self.fps.frame_ms = since * 1000.0 / self.fps.frames as f32;
            self.fps.frames = 0;
            self.fps.since = now;
        }

        // Devices.
        self.input.poll_gamepad();
        if self.input.pad.start_pressed {
            let open = !self.menu_open;
            self.set_menu(open);
        }
        if self.input.pad.map_pressed {
            self.game_ui.map = !self.game_ui.map;
        }
        self.pad_menus(dt);
        let pad = self.input.last_device == Device::Gamepad;
        self.game_ui.pad = pad;
        self.game_ui.tree.pad = pad;
        self.rig.params.yaw = (self.rig.params.yaw + self.input.pad.rotate * 90.0 * dt + 540.0).rem_euclid(360.0) - 180.0;
        self.rig.params.distance = (self.rig.params.distance * (1.0 + self.input.pad.zoom * dt)).clamp(2.0, 120.0);

        let host = self.host.as_ref().unwrap();
        host.pump();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(b) = &self.bridge {
            // Agent tools run on the simulation thread and can change the camera, view and look.
            if let Some(l) = b.poll(host, &mut self.rig, &mut self.view, self.look_ui.look()) {
                self.look_ui.set_look(l);
            }
        }
        let ui_wants_keys = self.egui_ctx.egui_wants_keyboard_input();
        let game_input = !self.menu_open && !ui_wants_keys;
        let rewinding = game_input && !self.animation_was_open && self.input.rewind_held();
        host.set_control(|c| c.rewinding = rewinding);

        let (prev, curr, curr_at, tick_wall) = host.frames();
        let animation_active = curr.animation_preview.is_some();
        let game_input = game_input && !animation_active;
        if animation_active != self.animation_was_open {
            self.builder = ViewBuilder::new();
            if let Some(gfx) = &mut self.gfx {
                gfx.renderer.clear_particles();
            }
            self.animation_was_open = animation_active;
        }
        if !animation_active {
            let room_before = self.hud.current.as_ref().map(|r| (r.id, std::sync::Arc::as_ptr(&r.def)));
            let quarters = curr.room.as_ref().map(|r| r.quarters).unwrap_or(0);
            self.director.update(&curr.hud, quarters, &mut self.rig);
            let room_id_before = self.hud.current.as_ref().map(|r| r.id);
            self.hud.update(&curr.room, &mut self.rig);
            // Rooms and pads can change view settings (night lighting, bloom, filters).
            let key = (curr.room.as_ref().map(|r| r.id), curr.hud.view_serial);
            if key != self.view_key {
                self.view_key = key;
                if let Some(b) = self.view_base.take() {
                    self.view = b;
                }
                let q = curr.room.as_ref().map(|r| r.quarters).unwrap_or(0);
                let room = curr.room.as_ref().map(|r| pav_view::build::room_view_map(&r.def.view, q)).unwrap_or_default();
                let pads = pav_view::build::room_view_map(&curr.hud.view, q);
                if !room.is_empty() || !pads.is_empty() {
                    self.view_base = Some(self.view.clone());
                    for u in self.view.apply(&room).into_iter().chain(self.view.apply(&pads)) {
                        log::warn!("unknown view setting '{u}'");
                    }
                }
            }
            if self.hud.current.as_ref().map(|r| r.id) != room_id_before {
                // Rooms can open HUD overlays (feel metrics) while you are inside.
                let wants = self.hud.current.as_ref().is_some_and(|r| r.def.overlays.iter().any(|o| o == "feel"));
                if wants && !self.feel.open {
                    self.feel.open = true;
                    self.feel.auto = true;
                } else if !wants && self.feel.auto {
                    self.feel.open = false;
                    self.feel.auto = false;
                }
                let wants = self.hud.current.as_ref().is_some_and(|r| r.def.overlays.iter().any(|o| o == "physics"));
                if wants && !self.physics.open {
                    self.physics.open = true;
                    self.physics.auto = true;
                } else if !wants && self.physics.auto {
                    self.physics.open = false;
                    self.physics.auto = false;
                }
            }
            self.feel.record(curr.tick, curr.hud.feel.speed);
            if self.hud.current.as_ref().map(|r| (r.id, std::sync::Arc::as_ptr(&r.def))) != room_before {
                // Input switching: the room's key overrides apply while inside.
                let (b, errs) = match &self.hud.current {
                    Some(r) => crate::input::Bindings::with_overrides(&r.def.keys),
                    None if curr.game.is_some() => (crate::input::Bindings::game(), Vec::new()),
                    None => (crate::input::Bindings::default(), Vec::new()),
                };
                self.input.bindings = b;
                for e in errs {
                    log::warn!("room keys: {e}");
                }
            }
            if *curr.config != self.sim_config && curr.tick > self.config_push_tick + 3 {
                // The simulation changed its configuration (room overrides): adopt it.
                self.sim_config = (*curr.config).clone();
            }
            // Entering or leaving Shardfall: its controls, camera and look.
            let game_mode = curr.game.is_some();
            if game_mode != self.input.game {
                self.input.game = game_mode;
                self.input.bindings = if game_mode { crate::input::Bindings::game() } else { crate::input::Bindings::default() };
                self.rig.blend_from_current(0.5);
                if game_mode {
                    self.game_saved = Some((self.rig.params.clone(), self.view.clone()));
                    self.rig.params = CameraParams {
                        tilt: 56.0,
                        yaw: 0.0,
                        distance: 15.5,
                        fov: 40.0,
                        ortho: false,
                        follow_lag: 0.06,
                        height_offset: 0.8,
                    };
                    let v = &mut self.view;
                    v.bloom = 0.55;
                    v.bloom_threshold = 1.0;
                    v.sky = "#14161c".into();
                    v.light.sun_elevation = 52.0;
                    v.light.sun_azimuth = 35.0;
                    v.light.sun_intensity = 0.95;
                    v.light.ambient = 0.5;
                    v.saturation = 1.08;
                    v.fog = false;
                } else if let Some((cam, view)) = self.game_saved.take() {
                    self.rig.params = cam;
                    self.view = view;
                }
            }
            self.game_frame = curr.game.clone();
            if !game_mode {
                self.game_ui.close_all();
                self.game_place = None;
            }
            if let Some(g) = &curr.game {
                if self.game_place != Some(g.place) {
                    self.game_place = Some(g.place);
                    self.arrival = Some(Instant::now());
                    self.game_ui.panel = None;
                    pav_view::arpg::place_look(g, &mut self.view);
                }
            }
        } else {
            self.game_frame = None;
        }
        let alpha =
            if self.app_settings.smoothing { ((now - curr_at).as_secs_f32() / tick_wall.max(1e-4)).clamp(0.0, 1.0) } else { 1.0 };
        let focus = prev.focus.lerp(curr.focus, alpha);
        let player = curr.player.and_then(|id| curr.objects.iter().find(|o| o.id == id));
        let feet = player.and_then(|p| p.puppet.as_ref().map(|pp| p.pos - Vec3::Y * pp.feet_offset)).unwrap_or(focus);
        let facing = player.and_then(|p| p.puppet.as_ref().map(|pp| pp.state.facing)).unwrap_or(0.0);

        // Game input -> the simulation thread.
        let (w, h) = self.gfx.as_ref().unwrap().size();
        let size = Vec2::new(w as f32, h as f32);
        let over_ui = self.egui_ctx.egui_wants_pointer_input();
        let left_tap = self.input.take_mouse_tap(MouseButton::Left) && !over_ui;
        if over_ui {
            self.input.mouse_taps.clear();
        }
        let (mut held, mut pressed) = self.input.buttons();
        // Touch: the stick, the buttons and auto-attack.
        let touch_game = curr.game.as_deref().filter(|_| game_input).map(|g| (g, feet, facing));
        let rig = &self.rig;
        let intent = self.touch.intent(touch_game, |v| rig.relative_move(v), self.prefs.auto_attack);
        if game_input {
            held |= intent.held;
            pressed |= intent.pressed;
        }
        if pressed & pav_core::input::buttons::INTERACT != 0 {
            if let Some(g) = &curr.game {
                if let Some(c) = self.game_ui.interact(g) {
                    host.shared.command(c);
                }
            }
        }
        if pressed != 0 && self.input.last_device == Device::Gamepad {
            // Gamepad presses are timed from when they were polled (gilrs queues them).
            self.latency.press();
        }
        let room_id = self.hud.current.as_ref().map(|r| r.id);
        let left = self.input.mouse.contains(&MouseButton::Left) && !over_ui;
        if left_tap && !self.editor.on && game_input {
            pressed |= pav_core::input::buttons::PRIMARY;
        }
        if !animation_active && self.editor.update(host, &self.rig, self.input.cursor, size, left, left_tap, room_id) {
            held &= !pav_core::input::buttons::PRIMARY;
            pressed &= !pav_core::input::buttons::PRIMARY;
        }
        let aim = match self.input.last_device {
            Device::KeyboardMouse => self.rig.ground_point(self.input.cursor, size, feet.y),
            Device::Gamepad => {
                let r = self.input.pad.right;
                if r.length() > 0.2 {
                    let d = self.rig.relative_move(r);
                    Some(feet + Vec3::new(d.x, 0.0, d.y) * self.sim_config.bombs.throw_range * r.length().min(1.0))
                } else {
                    Some(feet + Vec3::new(facing.sin(), 0.0, facing.cos()) * 4.0)
                }
            }
            Device::Touch => intent.aim.or(Some(feet + Vec3::new(facing.sin(), 0.0, facing.cos()) * 4.0)),
        };
        {
            let mut i = host.shared.input.lock().unwrap();
            if game_input {
                let axis = (self.input.move_axis() + intent.move_axis).clamp_length_max(1.0);
                i.move_dir = match intent.move_world {
                    // Auto-attack stepping in to a foe (only while the stick is still).
                    Some(m) if axis == Vec2::ZERO => m,
                    _ => self.rig.relative_move(axis),
                };
                i.held = held;
                i.pressed |= pressed;
                i.aim = aim;
                if let Some(t) = self.latency.pending.take() {
                    host.shared.input_stamp.lock().unwrap().get_or_insert(t);
                }
            } else {
                *i = InputFrame::default();
            }
        }

        self.rig.update(focus, dt);
        self.builder.now = self.started.elapsed().as_secs_f64();
        let events: Vec<_> = std::mem::take(&mut *host.shared.events.lock().unwrap());
        let events = if animation_active { Vec::new() } else { events };
        self.builder.add_events(&events);
        // Save the hero on every arrival somewhere, and once a minute while playing.
        let travelled = events.iter().any(|e| matches!(e, pav_core::SimEvent::Travel { .. }));
        if curr.game.is_some() && (travelled || self.last_save.elapsed().as_secs() >= 60) {
            self.last_save = Instant::now();
            host.exec(|sim| crate::save::save_from(sim));
        }
        if let Some(a) = &self.audio {
            let (_, right) = self.rig.ground_axes();
            let l = pav_audio::Listener { pos: self.rig.target, right };
            for e in &events {
                if matches!(e, pav_core::SimEvent::Step { .. }) && !self.audio_settings.footsteps {
                    continue;
                }
                a.play_event(e, &l, self.audio_settings.sfx);
            }
        }
        // The look layer (Look & Filters) over the scene's own settings.
        let mut view = self.view.clone();
        self.look_ui.apply(&mut view);
        self.prefs.quality.trim(self.touch_screen, &mut view);
        let mut scene = self.builder.build(&prev, &curr, alpha, &self.rig, w as f32 / h.max(1) as f32, &view, focus);
        // Arriving somewhere new: the screen opens with the chosen transition, on the player.
        if let Some(t0) = self.arrival {
            let t = t0.elapsed().as_secs_f32() / view.filter.transition_time.max(0.05);
            match view.filter.transition.kind() {
                Some(kind) if t < 1.0 => {
                    scene.filter.transition = 1.0 - t * t * (3.0 - 2.0 * t);
                    scene.filter.transition_kind = kind;
                    scene.filter.transition_center = pav_view::build::screen_uv(&scene.camera, feet + Vec3::Y * 0.9);
                }
                _ => self.arrival = None,
            }
        }
        if !animation_active {
            self.editor.draw_preview(&mut scene);
        }
        if let Some(a) = intent.aiming.filter(|_| game_input) {
            // A skill aimed by hand: a ring where it lands and dots on the way there.
            let mark = |at: Vec3, s: f32, color: Vec3| MeshInstance {
                mesh: MeshKey::Cylinder,
                transform: Mat4::from_scale_rotation_translation(Vec3::new(s, 0.02, s), Quat::IDENTITY, at + Vec3::Y * 0.04),
                color,
                emissive: 0.6,
                style: Style::Unlit,
                flags: rflags::NO_SHADOW | rflags::NO_CUT,
                group: 0,
            };
            let a = Vec3::new(a.x, feet.y, a.z);
            scene.meshes.push(mark(a, 0.9, Vec3::new(1.0, 0.78, 0.4)));
            let n = ((a - feet).length() / 0.8) as usize;
            for k in 1..n {
                scene.meshes.push(mark(feet.lerp(a, k as f32 / n as f32), 0.14, Vec3::new(1.0, 0.9, 0.7)));
            }
        }
        if self.app_settings.aim_marker && game_input && curr.player.is_some() && !self.editor.on && curr.game.is_none() {
            if let Some(a) = aim {
                let d = Vec2::new(a.x - feet.x, a.z - feet.z);
                let a = if d.length() > self.sim_config.bombs.throw_range {
                    let d = d.normalize() * self.sim_config.bombs.throw_range;
                    Vec3::new(feet.x + d.x, a.y, feet.z + d.y)
                } else {
                    a
                };
                scene.meshes.push(MeshInstance {
                    mesh: MeshKey::Cylinder,
                    transform: Mat4::from_scale_rotation_translation(
                        Vec3::new(0.5, 0.02, 0.5),
                        Quat::IDENTITY,
                        a + Vec3::Y * 0.03,
                    ),
                    color: Vec3::new(1.0, 0.95, 0.7),
                    emissive: 0.0,
                    style: Style::Unlit,
                    flags: rflags::NO_SHADOW | rflags::NO_CUT,
                    group: 0,
                });
            }
        }

        // UI.
        let stats = *host.shared.stats.lock().unwrap();
        let mut ctl = host.control();
        let ctl_before = ctl;
        let crash = host.shared.crashed.lock().unwrap().clone();
        self.panel.record_timing(dt * 1000.0, stats.tick_ms);
        let gfx = self.gfx.as_mut().unwrap();
        let (render_scale, lights) =
            self.scaler.update(self.prefs.quality, self.touch_screen, dt * 1000.0, gfx.size(), gfx.window.scale_factor() as f32);
        gfx.renderer.render_scale = render_scale;
        gfx.renderer.max_shadow_lights = lights;
        let info = ui::OverlayInfo {
            fps: self.fps.fps,
            frame_ms: self.fps.frame_ms,
            tps: stats.tps,
            tick_ms: stats.tick_ms,
            tick: stats.tick,
            entities: stats.entities,
            gpu_errors: gfx.gpu_errors.load(std::sync::atomic::Ordering::Relaxed),
            adapter: &gfx.adapter_name,
            crash: crash.clone(),
            paused: ctl.paused,
            speed: ctl.speed,
            rewinding,
            render_scale,
        };
        if let Some(t) = self.boot_done_at {
            if t.elapsed().as_secs_f32() > 10.0 && self.show_boot {
                self.show_boot = false;
                self.boot_done_at = None;
            }
        }
        let toast = self.toast.as_ref().filter(|(_, t)| t.elapsed().as_secs_f32() < 3.0).map(|(m, _)| m.clone());
        let card_visible = !animation_active && self.hud.card_visible();
        let guide_visible =
            !animation_active && self.app_settings.show_guide && self.started.elapsed().as_secs_f32() < 25.0 && !card_visible;
        let device = self.input.last_device;
        let touch_ui = device == Device::Touch;
        if touch_ui != self.touch_styled {
            self.touch_styled = touch_ui;
            ui::touch_style(&self.egui_ctx, touch_ui);
        }
        self.game_ui.touch = touch_ui;
        self.game_ui.tree.touch = touch_ui;
        self.touch.swipes.begin_frame();
        let touch = &mut self.touch;
        let auto_attack = self.prefs.auto_attack;
        let tips = touch_ui && !self.prefs.tips_seen && self.started.elapsed().as_secs_f32() < 45.0;
        let mut prefs = self.prefs.clone();
        let portal = self.game_frame.as_ref().is_some_and(|g| g.place != pav_core::arpg::Place::Town);
        let state = self.egui_state.as_mut().unwrap();
        let mut raw = state.take(&gfx.window);
        raw.events.append(&mut self.pad_events);
        let pad_cursor = self.pad_cursor;
        let mut show_boot = self.show_boot;
        let menu_open = self.menu_open;
        let scene_name = self.scene_name.clone();
        let mut menu_action = None;
        let mut panel_actions = Vec::new();
        let mut teleport = None;
        let mut save_room = false;
        let room_name = self.hud.current.as_ref().map(|r| r.key.clone());
        let editor = &mut self.editor;
        let hud = &mut self.hud;
        let config_before = self.sim_config.clone();
        let app_before = self.app_settings.clone();
        let panel = &mut self.panel;
        let master_before = self.audio_settings.master;
        let view_proj = self.rig.view_proj(w as f32 / h.max(1) as f32);
        let mut root = Root {
            sim: &mut self.sim_config,
            camera: &mut self.rig.params,
            view: &mut self.view,
            app: &mut self.app_settings,
            audio: &mut self.audio_settings,
        };
        let show_stats = root.app.show_stats;
        let feel = &mut self.feel;
        let physics = &mut self.physics;
        let latency = &self.latency;
        let hud_ctx = HudCtx { hud: &curr.hud, tick: curr.tick, dt: curr.dt };
        let game_frame = curr.game.clone();
        let game_ui = &mut self.game_ui;
        let look_ui = &mut self.look_ui;
        let animation_ui = &mut self.animation_ui;
        let mut animation_commands = Vec::new();
        #[cfg(not(target_arch = "wasm32"))]
        let animation_bridge = self.bridge.as_ref().map(|b| b.addr.as_str());
        #[cfg(target_arch = "wasm32")]
        let animation_bridge: Option<&str> = None;
        let mut look_msg = None;
        let guide = &mut self.guide;
        let mut guide_out = Vec::new();
        let mut game_cmds = Vec::new();
        let out = self.egui_ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            if let Some(c) = &info.crash {
                ui::crash_banner(ui, c);
            }
            panel_actions = panel.ui(ui, &mut root, &mut ctl, &stats);
            let free = ui.available_rect_before_wrap();
            ui::boot_panel(&ctx, &mut show_boot);
            if show_stats {
                ui::stats_panel(&ctx, &info, free, touch_ui);
            }
            if guide_visible && !menu_open && !touch_ui {
                ui::guide_panel(&ctx, device, game_frame.is_some());
            }
            if let Some(g) = &game_frame {
                let proj = crate::arpg_ui::Projector { vp: view_proj, size: ctx.content_rect().size() };
                crate::arpg_ui::hud(&ctx, g, &proj, device, game_ui.map, &mut touch.swipes);
                if !menu_open {
                    game_cmds = game_ui.ui(&ctx, g, &proj);
                }
            }
            if touch_ui {
                let map_hidden = touch.swipes.is_gone(egui::Id::new("minimap"), 0);
                let covered = menu_open || game_ui.any_open() || look_ui.open || guide.station || guide.field;
                let points = game_frame.as_ref().and_then(|g| g.inv.as_ref()).map(|i| i.points).unwrap_or(0);
                touch.draw(&ctx, &crate::touch::View { game: game_frame.as_deref(), auto_attack, map_hidden, points, covered });
                if tips && !covered {
                    ui::touch_tips(&ctx, game_frame.is_some(), &mut touch.swipes);
                }
            }
            if card_visible && !menu_open && hud.card(&ctx, device) {
                guide.station = true;
            }
            if !menu_open {
                crate::guide_ui::pad_note(&ctx, hud_ctx.hud, hud_ctx.tick, hud_ctx.dt, touch_ui.then_some(&mut touch.swipes));
            }
            crate::hud::course_hud(&ctx, &hud_ctx);
            if let Some(p) = pad_cursor {
                // The gamepad's menu cursor, above everything.
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("pad_cursor")));
                let tip = egui::Pos2::new(p.x, p.y);
                let pts = vec![tip, tip + egui::vec2(0.0, 22.0), tip + egui::vec2(6.0, 16.5), tip + egui::vec2(15.0, 16.0)];
                painter.add(egui::Shape::convex_polygon(
                    pts,
                    egui::Color32::from_rgb(255, 225, 150),
                    egui::Stroke::new(1.5, egui::Color32::BLACK),
                ));
            }
            physics.ui(&ctx, &hud_ctx, stats.tps, stats.tick_ms);
            crate::hud::hit_flash(&ctx, hud_ctx.hud.invuln);
            feel.ui(
                &ctx,
                &hud_ctx,
                latency,
                FeelSettings {
                    model: &mut root.sim.movement.model,
                    tick_rate: &mut root.sim.tick_rate,
                    vsync: &mut root.app.vsync,
                    smoothing: &mut root.app.smoothing,
                },
            );
            teleport = hud.teleport_menu(&ctx);
            hud.error_panel(&ctx);
            if !animation_active {
                save_room = editor.ui(&ctx, room_name.as_deref());
            }
            look_msg = look_ui.window(&ctx, root.view);
            animation_commands = animation_ui.window(&ctx, curr.animation_preview.as_ref(), animation_bridge);
            let here = hud.current.clone();
            guide_out.extend(guide.station(&ctx, here.as_ref(), &mut root));
            guide_out.extend(guide.field_guide(&ctx, &hud.entries));
            if menu_open {
                let difficulty = game_frame.is_some().then_some(&mut root.sim.difficulty);
                menu_action = if touch_ui {
                    let show_fps = &mut root.app.show_stats;
                    let m = ui::TouchMenu { prefs: &mut prefs, show_fps, difficulty, portal, scale: render_scale };
                    ui::touch_menu(&ctx, &scene_name, m)
                } else {
                    ui::pause_menu(&ctx, device, &scene_name, difficulty)
                };
            }
            if let Some(t) = &toast {
                if touch_ui {
                    ui::toast_touch(&ctx, t, &mut touch.swipes);
                } else {
                    ui::toast(&ctx, t);
                }
            }
            if !touch_ui {
                const GAME_KEYS: &str =
                    "Esc menu · I inventory · P passives · C character · K skills · T town · G use · Space dodge · 1 potion";
                ui::hint_bar(
                    &ctx,
                    match (device, game_frame.is_some()) {
                        (Device::Gamepad, _) => "Start: menu",
                        (_, true) => GAME_KEYS,
                        _ => "Esc menu · F1 tuning · F2 rooms · F12 screenshot",
                    },
                );
            }
        });
        if prefs != self.prefs {
            self.prefs = prefs;
            crate::save::save_prefs(&self.prefs);
        }
        // The map: a tap on the minimap opens the big one; a tap or a swipe closes that.
        let sw = &mut self.touch.swipes;
        if sw.take_tap(egui::Id::new("minimap")) {
            self.game_ui.map = true;
        }
        let big = egui::Id::new("big_map");
        if sw.take_tap(big) || sw.is_gone(big, 0) {
            self.game_ui.map = false;
            sw.restore(big);
        }
        if !self.prefs.tips_seen && sw.is_gone(egui::Id::new("touch_tips"), 0) {
            self.prefs.tips_seen = true;
            crate::save::save_prefs(&self.prefs);
        }
        self.show_boot = show_boot;
        for a in guide_out {
            match a {
                crate::guide_ui::GuideAction::Go(key) => teleport = Some(crate::rooms::TeleportTarget::Room(key)),
                crate::guide_ui::GuideAction::Toast(m) => self.toast = Some((m, Instant::now())),
            }
        }
        if let Some(m) = look_msg {
            self.toast = Some((m, Instant::now()));
        }
        self.look_ui.save_if_dirty(false);
        for c in game_cmds {
            host.shared.command(c);
        }
        if ctl.paused != ctl_before.paused
            || ctl.speed != ctl_before.speed
            || ctl.step_requests != ctl_before.step_requests
            || ctl.rewind_speed != ctl_before.rewind_speed
        {
            let c = ctl;
            host.set_control(|x| {
                x.paused = c.paused;
                x.speed = c.speed;
                x.step_requests = c.step_requests;
                x.rewind_speed = c.rewind_speed;
            });
        }
        if self.sim_config != config_before {
            let cfg = self.sim_config.clone();
            host.exec(move |sim| sim.config = cfg);
            self.config_push_tick = stats.tick.max(curr.tick);
        }
        if self.app_settings.vsync != app_before.vsync {
            gfx.set_vsync(self.app_settings.vsync);
        }
        if self.audio_settings.master != master_before {
            if let Some(a) = &self.audio {
                a.set_master(self.audio_settings.master);
            }
        }
        state.platform_output(&gfx.window, out.platform_output);
        let ppp = out.pixels_per_point;
        let prims = self.egui_ctx.tessellate(out.shapes, ppp);
        let mut textures = out.textures_delta;

        #[cfg(target_arch = "wasm32")]
        if self.screenshot_requested {
            // Reading pixels back waits on the GPU, which the browser does not allow here.
            self.screenshot_requested = false;
            self.toast = Some(("screenshots are not available in the browser".into(), Instant::now()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.screenshot_requested {
            self.screenshot_requested = false;
            let (w, h) = gfx.size();
            let dir = boot::exe_dir().join("screenshots");
            let path = dir.join(format!(
                "shardfall-{}.png",
                web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
            ));
            let r = pav_render::capture::render_to_rgba(&mut gfx.renderer, &scene, w, h)
                .and_then(|px| pav_render::capture::save_png(&path, w, h, &px));
            self.toast = Some((
                match r {
                    Ok(_) => format!("screenshot saved: {}", path.display()),
                    Err(e) => format!("screenshot failed: {e:#}"),
                },
                Instant::now(),
            ));
        }

        let first = self.boot_done_at.is_none()
            && boot::diag().stages.lock().map(|s| !s.iter().any(|s| s.name == "first frame")).unwrap_or(false);
        if first {
            let _ = stage("first frame", || {
                gfx.draw(&scene, Some((&prims, &mut textures, ppp)));
                Ok(((), format!("{} meshes, {} sdf", gfx.renderer.stats.mesh_instances, gfx.renderer.stats.sdf_instances)))
            });
            self.boot_done_at = Some(Instant::now());
            log::info!("boot complete in {:.0} ms", boot::diag().start.elapsed().as_secs_f64() * 1000.0);
        } else {
            gfx.draw(&scene, Some((&prims, &mut textures, ppp)));
        }
        // Input latency: the frame showing the tick that consumed a press is now submitted.
        {
            let mut probe = host.shared.latency_probe.lock().unwrap();
            if let Some((pressed_at, ticked_at, tick)) = *probe {
                if curr.tick >= tick {
                    let now = Instant::now();
                    self.latency.record(LatencySample {
                        to_tick: (ticked_at - pressed_at).as_secs_f32() * 1000.0,
                        to_screen: (now - ticked_at).as_secs_f32() * 1000.0,
                    });
                    *probe = None;
                }
            }
        }

        for a in panel_actions {
            self.panel_action(a);
        }
        if let Some(t) = teleport {
            self.teleport(t);
        }
        if save_room {
            if let (Some(host), Some(id)) = (&self.host, room_id) {
                let msg = match crate::edit::save_room(host, id) {
                    Ok(p) => format!("saved {}", p.display()),
                    Err(e) => format!("save failed: {e:#}"),
                };
                self.editor.status = msg.clone();
                self.toast(msg);
                if self.watcher.is_none() {
                    self.watcher = RoomWatcher::start();
                }
            }
        }
        if self.watcher.as_mut().is_some_and(|w| w.poll()) {
            self.reload_rooms();
        }
        if animation_active {
            self.ensure_animation_watcher();
        }
        let animation_updates = self.animation_watcher.as_mut().map(|w| w.poll()).unwrap_or_default();
        for update in animation_updates {
            match update {
                Ok(message) => {
                    log::info!("{message}");
                    self.animation_ui.status = message;
                    self.animation_ui.error = false;
                }
                Err(error) => {
                    log::warn!("animation reload: {error}");
                    self.animation_ui.report(Err(error));
                }
            }
        }
        for command in animation_commands {
            self.animation_command(command);
        }
        if let Some(a) = menu_action {
            self.menu_action(a);
        }
        // Touch: the bag, the menu, the map button, and taps on the open game.
        for c in &intent.ui {
            match c {
                crate::touch::Control::Bag => {
                    let ui = &mut self.game_ui;
                    if ui.any_open() {
                        ui.close_all();
                    } else {
                        ui.inventory = true;
                    }
                }
                crate::touch::Control::Menu => {
                    let open = !self.menu_open;
                    self.set_menu(open);
                }
                crate::touch::Control::Map => self.touch.swipes.restore(egui::Id::new("minimap")),
                _ => {}
            }
        }
        if intent.world_tap {
            self.close_on_tap();
        }
    }

    fn panel_action(&mut self, a: PanelAction) {
        let Some(host) = &self.host else { return };
        let dir = boot::exe_dir();
        match a {
            PanelAction::Reset => self.reset(),
            PanelAction::Scrub(t) => host.set_control(|c| {
                c.paused = true;
                c.scrub = Some(t);
            }),
            PanelAction::SaveSnapshot => {
                let path = dir.join("snapshots").join("quicksave.snap");
                let r = host.query(move |sim| sim.save_state(&path).map(|_| path.display().to_string()));
                match r {
                    Some(Ok(p)) => self.toast(format!("snapshot saved: {p}")),
                    Some(Err(e)) => self.toast(format!("snapshot failed: {e:#}")),
                    None => self.toast("snapshot timed out"),
                }
            }
            PanelAction::LoadSnapshot => {
                let path = dir.join("snapshots").join("quicksave.snap");
                let r = host.query(move |sim| sim.load_state(&path));
                match r {
                    Some(Ok(())) => self.toast("snapshot loaded"),
                    Some(Err(e)) => self.toast(format!("load failed: {e:#}")),
                    None => self.toast("load timed out"),
                }
            }
            PanelAction::SaveReplay => {
                let path = dir.join("replays").join("last.replay.json");
                let r = host.query(move |sim| -> Result<String> {
                    let mut rep = sim.recording.clone();
                    rep.final_hash = Some(format!("{:016x}", sim.state_hash()));
                    std::fs::create_dir_all(path.parent().unwrap())?;
                    std::fs::write(&path, serde_json::to_string(&rep)?)?;
                    Ok(path.display().to_string())
                });
                match r {
                    Some(Ok(p)) => self.toast(format!("replay saved: {p}")),
                    Some(Err(e)) => self.toast(format!("replay failed: {e:#}")),
                    None => self.toast("replay timed out"),
                }
            }
            PanelAction::Look => self.look_ui.open = true,
            PanelAction::Status(s) => {
                self.panel.status(s.clone());
                self.toast(s);
            }
        }
    }

    /// Use the registered tools for UI edits too. This keeps validation, undo, and
    /// persistence identical for a user and an LLM.
    fn animation_command(&mut self, command: crate::animation_ui::Command) {
        let Some(host) = &self.host else { return };
        let rig = self.rig.clone();
        let view = self.view.clone();
        let reply = host.query(move |sim| {
            let live = std::mem::replace(sim, Sim::empty(1));
            let mut session = pav_tools::Session::from_live(live, rig, view, None);
            let result = pav_tools::tools::call(&mut session, command.tool, &command.args)
                .map(|out| match out {
                    pav_tools::Output::Json(v) => v,
                    pav_tools::Output::Image { meta, .. } => meta,
                })
                .map_err(|e| format!("{e:#}"));
            let (live, rig, view, _) = session.into_live();
            *sim = live;
            (result, rig, view)
        });
        match reply {
            Some((result, rig, view)) => {
                self.rig = rig;
                self.view = view;
                self.animation_ui.report(result);
            }
            None => self.animation_ui.report(Err("The animation command did not answer. Check the game log.".into())),
        }
        self.ensure_animation_watcher();
    }

    fn ensure_animation_watcher(&mut self) {
        // An agent may create the first animation directory through the bridge. Attach
        // then, but do not retry a failed native watcher on every frame.
        if !self.animation_watch_attempted && crate::animation_watch::AnimationWatcher::directory_exists() {
            self.animation_watch_attempted = true;
            self.animation_watcher = crate::animation_watch::AnimationWatcher::start();
        }
    }

    fn menu_action(&mut self, a: MenuAction) {
        match a {
            MenuAction::Resume => self.set_menu(false),
            MenuAction::AnimationStudio => {
                self.set_menu(false);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    if let Err(error) = pav_tools::animation_tools::reload_authored() {
                        self.animation_ui.report(Err(format!("Could not load saved animations: {error:#}")));
                    }
                    if self.bridge.is_none() {
                        match crate::bridge::Bridge::start(pav_tools::bridge::DEFAULT_ADDR) {
                            Ok(bridge) => self.bridge = Some(bridge),
                            Err(error) => self.animation_ui.report(Err(format!("Could not open the live bridge: {error}"))),
                        }
                    }
                }
                self.animation_ui.open = true;
                self.animation_command(crate::animation_ui::Command {
                    tool: "anim_preview",
                    args: serde_json::json!({"action":"open", "clip":"QUATERNIUS/Idle_Loop"}).as_object().unwrap().clone(),
                });
            }
            MenuAction::Reset => {
                self.set_menu(false);
                self.reset();
            }
            MenuAction::LoadScene(name) => {
                self.set_menu(false);
                self.load_scene(&name);
            }
            MenuAction::NewHero => {
                self.set_menu(false);
                if let Some(host) = &self.host {
                    let (cfg, seed) = (self.sim_config.clone(), self.settings.seed);
                    host.exec(move |sim| {
                        crate::save::erase();
                        if let Ok(mut fresh) = Sim::new("town", seed) {
                            fresh.config = cfg;
                            *sim = fresh;
                        }
                    });
                }
                self.scene_name = "town".into();
                self.toast("a new hero arrives in Emberwatch");
            }
            MenuAction::Tuning => {
                self.set_menu(false);
                self.panel.open = true;
            }
            MenuAction::Look => {
                self.set_menu(false);
                self.look_ui.open = true;
            }
            MenuAction::Guide => {
                self.set_menu(false);
                if self.hud.current.is_some() {
                    self.guide.station = true;
                } else {
                    self.guide.field = true;
                }
            }
            MenuAction::FieldGuide => {
                self.set_menu(false);
                self.guide.field = true;
            }
            MenuAction::Rooms => {
                self.set_menu(false);
                self.open_teleport();
            }
            MenuAction::RoomCard => {
                self.set_menu(false);
                self.hud.show_card();
            }
            MenuAction::Screenshot => {
                self.set_menu(false);
                self.screenshot_requested = true;
            }
            MenuAction::Feel => {
                self.set_menu(false);
                self.feel.open = !self.feel.open;
                self.feel.auto = false;
            }
            MenuAction::Physics => {
                self.set_menu(false);
                self.physics.open = !self.physics.open;
                self.physics.auto = false;
            }
            MenuAction::Quit => {
                self.look_ui.save_if_dirty(true);
                self.quit = true;
            }
            MenuAction::TownPortal => {
                self.set_menu(false);
                if let Some(host) = &self.host {
                    host.shared.command(pav_core::arpg::GameCmd::Travel(pav_core::arpg::Place::Town.code()));
                }
            }
            MenuAction::Fullscreen => {
                if let Some(g) = &self.gfx {
                    crate::platform::toggle_fullscreen(&g.window);
                }
            }
        }
    }
}

impl App {
    /// The browser only allows sound after the first click or key press.
    #[cfg(target_arch = "wasm32")]
    fn resume_audio(&mut self) {
        if !self.audio_resumed {
            self.audio_resumed = true;
            if let Some(a) = &self.audio {
                a.resume();
            }
        }
    }

    /// A start-up failure: stop, and show why (the browser has no exit to report it).
    fn fail(&mut self, el: &ActiveEventLoop, e: anyhow::Error) {
        let msg = format!("{e:#}");
        #[cfg(target_arch = "wasm32")]
        crate::platform::error_box("Shardfall could not start", &msg);
        self.fatal = Some(msg);
        el.exit();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.init_started || self.fatal.is_some() {
            return;
        }
        self.init_started = true;
        if let Err(e) = self.init(el) {
            self.fail(el, e);
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::Touch(t) = &event {
            self.on_touch(t, &event);
            return;
        }
        let consumed = match (&mut self.egui_state, &self.gfx) {
            (Some(st), Some(g)) => st.on_window_event(&g.window, &event),
            _ => false,
        };
        match event {
            WindowEvent::CloseRequested => {
                if let Some(host) = &self.host {
                    host.exec(|sim| crate::save::save_from(sim));
                }
                self.look_ui.save_if_dirty(true);
                el.exit()
            }
            WindowEvent::Focused(false) => {
                self.input.clear();
                self.touch.clear();
            }
            WindowEvent::Resized(sz) => {
                if let Some(g) = &mut self.gfx {
                    g.resize(sz.width, sz.height);
                }
            }
            WindowEvent::RedrawRequested => {
                self.frame();
                if self.quit {
                    el.exit();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                #[cfg(target_arch = "wasm32")]
                self.resume_audio();
                if let PhysicalKey::Code(code) = event.physical_key {
                    let system = matches!(
                        code,
                        KeyCode::Escape
                            | KeyCode::F1
                            | KeyCode::F2
                            | KeyCode::F3
                            | KeyCode::F4
                            | KeyCode::F5
                            | KeyCode::F6
                            | KeyCode::F7
                            | KeyCode::F8
                            | KeyCode::F9
                            | KeyCode::F10
                            | KeyCode::F11
                            | KeyCode::F12
                    );
                    if event.state == ElementState::Pressed && !event.repeat && (system || !consumed) {
                        self.handle_key(code);
                        if !system && !consumed {
                            self.latency.press();
                        }
                    }
                    if !consumed || event.state == ElementState::Released {
                        self.input.key(code, event.state);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } if !consumed => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.rig.params.distance = (self.rig.params.distance * 0.9f32.powf(lines)).clamp(2.0, 120.0);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                #[cfg(target_arch = "wasm32")]
                self.resume_audio();
                if state == ElementState::Pressed && button == MouseButton::Left && !consumed {
                    self.latency.press();
                }
                // In the game the right button is a skill; the middle button turns the camera.
                let turn = if self.input.game { MouseButton::Middle } else { MouseButton::Right };
                if button == turn {
                    self.mouse.right_down = state == ElementState::Pressed && !consumed;
                } else if !consumed || state == ElementState::Released {
                    self.input.mouse_button(button, state);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = Vec2::new(position.x as f32, position.y as f32);
                if self.mouse.right_down {
                    let d = p - self.mouse.pos;
                    self.rig.params.yaw = (self.rig.params.yaw - d.x * 0.3 + 540.0).rem_euclid(360.0) - 180.0;
                    self.rig.params.tilt = (self.rig.params.tilt + d.y * 0.2).clamp(0.0, 90.0);
                }
                if (p - self.mouse.pos).length() > 2.0 {
                    self.input.last_device = Device::KeyboardMouse;
                }
                self.mouse.pos = p;
                self.input.cursor = p;
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        #[cfg(target_arch = "wasm32")]
        if let Some(r) = self.pending_gfx.as_ref().and_then(|slot| slot.borrow_mut().take()) {
            self.pending_gfx = None;
            if let Err(e) = r.and_then(|gfx| self.finish_init(gfx)) {
                self.fail(el, e);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = el;
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
    }
}

fn room_entries(sim: &Sim) -> Vec<RoomEntry> {
    let mut v: Vec<RoomEntry> = sim
        .state
        .world
        .rooms
        .iter()
        .map(|r| RoomEntry {
            key: r.key.clone(),
            name: if r.def.name.is_empty() { r.key.clone() } else { r.def.name.clone() },
            wing: r.def.wing.clone(),
            about: r.def.about.clone(),
            ask: r.def.learn.ask.clone(),
        })
        .collect();
    v.sort_by(|a, b| (&a.wing, &a.name).cmp(&(&b.wing, &b.name)));
    v
}
