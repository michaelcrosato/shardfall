//! The game window: owns graphics, UI, the simulation thread and the camera.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Result, anyhow};
use glam::Vec2;
use pav_core::Sim;
use pav_render::gpu::BackendChoice;
use pav_view::{CameraParams, CameraRig, ViewBuilder, ViewSettings};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::boot::{self, stage};
use crate::gfx::Gfx;
use crate::settings::Settings;
use crate::simhost::SimHost;
use crate::ui;

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
    builder: ViewBuilder,
    show_boot: bool,
    boot_done_at: Option<Instant>,
    last: Instant,
    fps: Fps,
    mouse: Mouse,
    smoothing: bool,
    pub fatal: Option<String>,
}

impl App {
    fn new(settings: Settings) -> Self {
        Self {
            settings,
            gfx: None,
            egui_ctx: egui::Context::default(),
            egui_state: None,
            host: None,
            rig: CameraRig::default(),
            view: ViewSettings::default(),
            builder: ViewBuilder::new(),
            show_boot: true,
            boot_done_at: None,
            last: Instant::now(),
            fps: Fps { frames: 0, since: Instant::now(), fps: 0.0, frame_ms: 0.0 },
            mouse: Mouse::default(),
            smoothing: true,
            fatal: None,
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
        self.rig.snap(sim.state.focus);
        self.host = Some(SimHost::start(sim));
        self.gfx = Some(gfx);
        self.egui_state = Some(egui_state);
        Ok(())
    }

    fn handle_key(&mut self, el: &ActiveEventLoop, code: KeyCode) {
        let Some(host) = &self.host else { return };
        match code {
            KeyCode::F3 => self.show_boot = !self.show_boot,
            KeyCode::F11 => {
                if let Some(g) = &self.gfx {
                    let fs = g.window.fullscreen().is_some();
                    g.window.set_fullscreen(if fs { None } else { Some(Fullscreen::Borderless(None)) });
                }
            }
            KeyCode::F6 => host.set_control(|c| c.paused = !c.paused),
            KeyCode::F7 => host.set_control(|c| {
                c.paused = true;
                c.step_requests += 1;
            }),
            KeyCode::F8 => host.set_control(|c| c.speed = (c.speed * 0.5).max(0.0625)),
            KeyCode::F9 => host.set_control(|c| c.speed = (c.speed * 2.0).min(8.0)),
            KeyCode::F5 => {
                let (scene, seed) = (self.settings.scene.clone(), self.settings.seed);
                host.exec(move |sim| match Sim::new(&scene, seed) {
                    Ok(fresh) => *sim = fresh,
                    Err(e) => log::error!("reset failed: {e:#}"),
                });
            }
            KeyCode::KeyV => {
                if let Some(g) = &mut self.gfx {
                    let on = !g.vsync;
                    g.set_vsync(on);
                    log::info!("vsync {}", if on { "on" } else { "off" });
                }
            }
            KeyCode::KeyI => self.smoothing = !self.smoothing,
            KeyCode::Escape if self.gfx.as_ref().is_some_and(|g| g.window.fullscreen().is_some()) => {
                if let Some(g) = &self.gfx {
                    g.window.set_fullscreen(None);
                }
            }
            _ => {
                let presets = CameraParams::PRESETS;
                let digit = [
                    KeyCode::Digit1,
                    KeyCode::Digit2,
                    KeyCode::Digit3,
                    KeyCode::Digit4,
                    KeyCode::Digit5,
                    KeyCode::Digit6,
                    KeyCode::Digit7,
                    KeyCode::Digit8,
                ]
                .iter()
                .position(|k| *k == code);
                if let Some(i) = digit.filter(|i| *i < presets.len()) {
                    self.rig.params = (presets[i].1)();
                    log::info!("camera preset: {}", presets[i].0);
                }
            }
        }
        let _ = el;
    }

    fn frame(&mut self) {
        let (Some(gfx), Some(host)) = (&mut self.gfx, &self.host) else { return };
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

        let (prev, curr, curr_at, tick_wall) = host.frames();
        let alpha = if self.smoothing { ((now - curr_at).as_secs_f32() / tick_wall.max(1e-4)).clamp(0.0, 1.0) } else { 1.0 };
        let focus = prev.focus.lerp(curr.focus, alpha);
        self.rig.update(focus, dt);
        let (w, h) = gfx.size();
        let scene = self.builder.build(&prev, &curr, alpha, &self.rig, w as f32 / h.max(1) as f32, &self.view, focus);

        // UI.
        let stats = *host.shared.stats.lock().unwrap();
        let ctl = host.control();
        let crash = host.shared.crashed.lock().unwrap().clone();
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
        };
        if let Some(t) = self.boot_done_at {
            if t.elapsed().as_secs_f32() > 10.0 && self.show_boot {
                self.show_boot = false;
                self.boot_done_at = None;
            }
        }
        let state = self.egui_state.as_mut().unwrap();
        let raw = state.take_egui_input(&gfx.window);
        let mut show_boot = self.show_boot;
        let out = self.egui_ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            ui::boot_panel(&ctx, &mut show_boot);
            ui::stats_panel(&ctx, &info);
            if let Some(c) = &info.crash {
                ui::crash_banner(ui, c);
            }
            ui::hint_bar(
                &ctx,
                "wheel zoom · right-drag rotate · 1–8 camera presets · F6 pause · F7 step · F8/F9 speed · F5 reset · V vsync · I smoothing · F3 diagnostics · F11 fullscreen",
            );
        });
        self.show_boot = show_boot;
        state.handle_platform_output(&gfx.window, out.platform_output);
        let ppp = out.pixels_per_point;
        let prims = self.egui_ctx.tessellate(out.shapes, ppp);
        let mut textures = out.textures_delta;

        if self.boot_done_at.is_none()
            && self.show_boot
            && boot::diag().stages.lock().map(|s| !s.iter().any(|s| s.name == "first frame")).unwrap_or(false)
        {
            let _ = stage("first frame", || {
                gfx.draw(&scene, Some((&prims, &mut textures, ppp)));
                Ok(((), format!("{} meshes, {} sdf", gfx.renderer.stats.mesh_instances, gfx.renderer.stats.sdf_instances)))
            });
            self.boot_done_at = Some(Instant::now());
            log::info!("boot complete in {:.0} ms", boot::diag().start.elapsed().as_secs_f64() * 1000.0);
        } else {
            gfx.draw(&scene, Some((&prims, &mut textures, ppp)));
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
            WindowEvent::Resized(sz) => {
                if let Some(g) = &mut self.gfx {
                    g.resize(sz.width, sz.height);
                }
            }
            WindowEvent::RedrawRequested => self.frame(),
            WindowEvent::KeyboardInput { event, .. } if !consumed => {
                if event.state == ElementState::Pressed && !event.repeat {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        self.handle_key(el, code);
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
            WindowEvent::MouseInput { state, button: MouseButton::Right, .. } => {
                self.mouse.right_down = state == ElementState::Pressed && !consumed;
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = Vec2::new(position.x as f32, position.y as f32);
                if self.mouse.right_down {
                    let d = p - self.mouse.pos;
                    self.rig.params.yaw = (self.rig.params.yaw - d.x * 0.3 + 540.0).rem_euclid(360.0) - 180.0;
                    self.rig.params.tilt = (self.rig.params.tilt + d.y * 0.2).clamp(0.0, 90.0);
                }
                self.mouse.pos = p;
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
