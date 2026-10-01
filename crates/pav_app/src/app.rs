//! The game window: owns graphics, UI, input, the simulation thread and the camera.

use std::sync::Arc;
use std::time::Instant;

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
use crate::hud::{CameraDirector, FeelOverlay, FeelSettings, HudCtx, Latency, LatencySample};
use crate::input::{Device, Input};
use crate::panel::{Panel, PanelAction};
use crate::rooms::{RoomEntry, RoomHud, RoomWatcher, TeleportTarget};
use crate::settings::Settings;
use crate::simhost::SimHost;
use crate::ui::{self, MenuAction};

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
    egui_state: Option<egui_winit::State>,
    host: Option<SimHost>,
    rig: CameraRig,
    view: ViewSettings,
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
    quit: bool,
    pub fatal: Option<String>,
}

impl App {
    fn new(settings: Settings) -> Self {
        let scene_name = settings.scene.clone();
        Self {
            gfx: None,
            egui_ctx: egui::Context::default(),
            egui_state: None,
            host: None,
            rig: CameraRig::default(),
            view: ViewSettings::default(),
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
            quit: false,
            fatal: None,
            settings,
        }
    }

    fn init(&mut self, el: &ActiveEventLoop) -> Result<()> {
        let s = self.settings.clone();
        let window = stage("window", || {
            let mut attrs = Window::default_attributes()
                .with_title("Pavilion")
                .with_inner_size(winit::dpi::LogicalSize::new(s.width, s.height));
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
        let gfx = Gfx::new(window.clone(), backend, s.vsync)?;
        let egui_state = stage("ui", || {
            let st = egui_winit::State::new(
                self.egui_ctx.clone(),
                egui::ViewportId::ROOT,
                &window,
                Some(window.scale_factor() as f32),
                None,
                Some(gfx.device.limits().max_texture_dimension_2d as usize),
            );
            Ok((st, String::new()))
        })?;
        self.audio = stage("audio", || match pav_audio::AudioOut::start() {
            Ok(a) => {
                let d = format!("{} @ {} Hz", a.device, a.sample_rate);
                Ok((Some(a), d))
            }
            Err(e) => Ok((None, format!("no sound ({e}); continuing silently"))),
        })?;
        let sim = stage("simulation", || {
            let sim = Sim::new(&s.scene, s.seed)?;
            let d = format!(
                "scene '{}', seed {}, {} entities, {} blocks",
                s.scene,
                s.seed,
                sim.state.entities.len(),
                sim.state.statics.block_count()
            );
            Ok((sim, d))
        })?;
        self.sim_config = sim.config.clone();
        self.rig.snap(sim.state.focus);
        self.hud.entries = room_entries(&sim);
        self.hud.errors = sim.state.world.errors.clone();
        self.watcher = RoomWatcher::start();
        self.host = Some(SimHost::start(sim));
        self.gfx = Some(gfx);
        self.egui_state = Some(egui_state);
        Ok(())
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
        host.exec(move |sim| match Sim::new(&scene, seed) {
            Ok(mut fresh) => {
                fresh.config = cfg;
                *sim = fresh;
            }
            Err(e) => log::error!("could not load scene: {e:#}"),
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
                if let Err(e) = sim.reset() {
                    log::error!("reset failed: {e:#}");
                }
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
        match code {
            KeyCode::Escape => {
                let open = !self.menu_open;
                self.set_menu(open);
            }
            KeyCode::F1 => self.panel.open = !self.panel.open,
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
            KeyCode::F10 => {
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
                if let Some(i) = digits.iter().position(|k| *k == code).filter(|i| *i < CameraParams::PRESETS.len()) {
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
        self.rig.params.yaw = (self.rig.params.yaw + self.input.pad.rotate * 90.0 * dt + 540.0).rem_euclid(360.0) - 180.0;
        self.rig.params.distance = (self.rig.params.distance * (1.0 + self.input.pad.zoom * dt)).clamp(2.0, 120.0);

        let host = self.host.as_ref().unwrap();
        let ui_wants_keys = self.egui_ctx.egui_wants_keyboard_input();
        let game_input = !self.menu_open && !ui_wants_keys;
        let rewinding = game_input && self.input.rewind_held();
        host.set_control(|c| c.rewinding = rewinding);

        let (prev, curr, curr_at, tick_wall) = host.frames();
        let room_before = self.hud.current.as_ref().map(|r| (r.id, std::sync::Arc::as_ptr(&r.def)));
        let quarters = curr.room.as_ref().map(|r| r.quarters).unwrap_or(0);
        self.director.update(&curr.hud, quarters, &mut self.rig);
        let room_id_before = self.hud.current.as_ref().map(|r| r.id);
        self.hud.update(&curr.room, &mut self.rig);
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
        }
        self.feel.record(curr.tick, curr.hud.feel.speed);
        if self.hud.current.as_ref().map(|r| (r.id, std::sync::Arc::as_ptr(&r.def))) != room_before {
            // Input switching: the room's key overrides apply while inside.
            let (b, errs) = match &self.hud.current {
                Some(r) => crate::input::Bindings::with_overrides(&r.def.keys),
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
        let alpha =
            if self.app_settings.smoothing { ((now - curr_at).as_secs_f32() / tick_wall.max(1e-4)).clamp(0.0, 1.0) } else { 1.0 };
        let focus = prev.focus.lerp(curr.focus, alpha);
        let player = curr.player.and_then(|id| curr.objects.iter().find(|o| o.id == id));
        let feet = player.and_then(|p| p.puppet.map(|pp| p.pos - Vec3::Y * pp.feet_offset)).unwrap_or(focus);
        let facing = player.and_then(|p| p.puppet.map(|pp| pp.state.facing)).unwrap_or(0.0);

        // Game input -> the simulation thread.
        let (w, h) = self.gfx.as_ref().unwrap().size();
        let size = Vec2::new(w as f32, h as f32);
        let over_ui = self.egui_ctx.egui_wants_pointer_input();
        let left_tap = self.input.take_mouse_tap(MouseButton::Left) && !over_ui;
        if over_ui {
            self.input.mouse_taps.clear();
        }
        let (mut held, mut pressed) = self.input.buttons();
        if pressed != 0 && self.input.last_device == Device::Gamepad {
            // Gamepad presses are timed from when they were polled (gilrs queues them).
            self.latency.press();
        }
        let room_id = self.hud.current.as_ref().map(|r| r.id);
        let left = self.input.mouse.contains(&MouseButton::Left) && !over_ui;
        if left_tap && !self.editor.on && game_input {
            pressed |= pav_core::input::buttons::PRIMARY;
        }
        if self.editor.update(host, &self.rig, self.input.cursor, size, left, left_tap, room_id) {
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
        };
        {
            let mut i = host.shared.input.lock().unwrap();
            if game_input {
                i.move_dir = self.rig.relative_move(self.input.move_axis());
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
        self.builder.add_events(&events);
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
        let mut scene = self.builder.build(&prev, &curr, alpha, &self.rig, w as f32 / h.max(1) as f32, &self.view, focus);
        self.editor.draw_preview(&mut scene);
        if self.app_settings.aim_marker && game_input && curr.player.is_some() && !self.editor.on {
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
        };
        if let Some(t) = self.boot_done_at {
            if t.elapsed().as_secs_f32() > 10.0 && self.show_boot {
                self.show_boot = false;
                self.boot_done_at = None;
            }
        }
        let toast = self.toast.as_ref().filter(|(_, t)| t.elapsed().as_secs_f32() < 3.0).map(|(m, _)| m.clone());
        let card_visible = self.hud.card_visible();
        let guide_visible = self.app_settings.show_guide && self.started.elapsed().as_secs_f32() < 25.0 && !card_visible;
        let device = self.input.last_device;
        let state = self.egui_state.as_mut().unwrap();
        let raw = state.take_egui_input(&gfx.window);
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
        let mut root = Root {
            sim: &mut self.sim_config,
            camera: &mut self.rig.params,
            view: &mut self.view,
            app: &mut self.app_settings,
            audio: &mut self.audio_settings,
        };
        let show_stats = root.app.show_stats;
        let feel = &mut self.feel;
        let latency = &self.latency;
        let hud_ctx = HudCtx { hud: &curr.hud, tick: curr.tick, dt: curr.dt };
        let out = self.egui_ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            if let Some(c) = &info.crash {
                ui::crash_banner(ui, c);
            }
            panel_actions = panel.ui(ui, &mut root, &mut ctl, &stats);
            let free = ui.available_rect_before_wrap();
            ui::boot_panel(&ctx, &mut show_boot);
            if show_stats {
                ui::stats_panel(&ctx, &info, free);
            }
            if guide_visible && !menu_open {
                ui::guide_panel(&ctx, device);
            }
            if card_visible && !menu_open {
                hud.card(&ctx, device);
            }
            crate::hud::course_hud(&ctx, &hud_ctx);
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
            save_room = editor.ui(&ctx, room_name.as_deref());
            if menu_open {
                menu_action = ui::pause_menu(&ctx, device, &scene_name);
            }
            if let Some(t) = &toast {
                ui::toast(&ctx, t);
            }
            ui::hint_bar(&ctx, if device == Device::Gamepad { "Start: menu" } else { "Esc menu · F1 tuning · F12 screenshot" });
        });
        self.show_boot = show_boot;
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
        state.handle_platform_output(&gfx.window, out.platform_output);
        let ppp = out.pixels_per_point;
        let prims = self.egui_ctx.tessellate(out.shapes, ppp);
        let mut textures = out.textures_delta;

        if self.screenshot_requested {
            self.screenshot_requested = false;
            let (w, h) = gfx.size();
            let dir = boot::exe_dir().join("screenshots");
            let path = dir.join(format!(
                "pavilion-{}.png",
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
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
        if let Some(a) = menu_action {
            self.menu_action(a);
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
            PanelAction::Status(s) => {
                self.panel.status(s.clone());
                self.toast(s);
            }
        }
    }

    fn menu_action(&mut self, a: MenuAction) {
        match a {
            MenuAction::Resume => self.set_menu(false),
            MenuAction::Reset => {
                self.set_menu(false);
                self.reset();
            }
            MenuAction::LoadScene(name) => {
                self.set_menu(false);
                self.load_scene(&name);
            }
            MenuAction::Tuning => {
                self.set_menu(false);
                self.panel.open = true;
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
            MenuAction::Quit => self.quit = true,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_some() || self.fatal.is_some() {
            return;
        }
        if let Err(e) = self.init(el) {
            self.fatal = Some(format!("{e:#}"));
            el.exit();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let consumed = match (&mut self.egui_state, &self.gfx) {
            (Some(st), Some(g)) => st.on_window_event(&g.window, &event).consumed,
            _ => false,
        };
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Focused(false) => self.input.clear(),
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
                if state == ElementState::Pressed && button == MouseButton::Left && !consumed {
                    self.latency.press();
                }
                if button == MouseButton::Right {
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

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
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
        })
        .collect();
    v.sort_by(|a, b| (&a.wing, &a.name).cmp(&(&b.wing, &b.name)));
    v
}
