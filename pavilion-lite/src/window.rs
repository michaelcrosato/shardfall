//! The play window, for people: winit + softbuffer showing the CPU renderer, a fixed 60 Hz
//! simulation, keyboard and mouse. `pav play <game> [seed=N] [scale=N] [size=1280x720]`.
//!
//! Keys: WASD / arrows move (relative to the camera), Space jump, J or left mouse fire,
//! K or right mouse alt, E use, C / Ctrl crouch, Shift dash; mouse aims.
//! System: Esc quit, F1 help, F5 restart, P pause, F7 step (paused), hold Backspace rewind,
//! F12 screenshot, Tab stats.

use std::collections::HashSet;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use glam::{Vec2, Vec3};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::input::{Input, buttons};
use crate::render::{self, Image};
use crate::sim::{GameDef, Sim};
use crate::tools::Args;
use crate::util::Color;
use crate::view::{self, Follow};

const TICK: f64 = 1.0 / 60.0;

pub fn play(def: &'static GameDef, args: &Args) -> Result<(), String> {
    let seed = args.get("seed").and_then(|v| v.as_u64()).unwrap_or(1);
    let scale = args.get("scale").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let size = args
        .get("size")
        .and_then(|v| v.as_str())
        .and_then(|s| s.split_once('x'))
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .unwrap_or((1280u32, 720u32));
    let hint =
        "on Linux a desktop session with libxkbcommon-x11 is needed; the headless tools (capture, filmstrip, ...) need nothing";
    let event_loop = EventLoop::new().map_err(|e| format!("no display ({e}); {hint}"))?;
    let mut app = App {
        def,
        seed,
        sim: Sim::new(def, seed),
        size,
        scale,
        window: None,
        surface: None,
        keys: HashSet::new(),
        pressed: 0,
        mouse: None,
        mouse_held: 0,
        last: Instant::now(),
        acc: 0.0,
        paused: false,
        step_once: false,
        follow: Follow::default(),
        help: true,
        help_pinned: false,
        started: Instant::now(),
        stats: false,
        frame_ms: 0.0,
        auto: 1,
        last_image: None,
        message: None,
    };
    // Some platform libraries are loaded at runtime and panic when missing: report that nicely.
    std::panic::set_hook(Box::new(|info| eprintln!("window failed: {info}")));
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| event_loop.run_app(&mut app)));
    let _ = std::panic::take_hook();
    match run {
        Ok(r) => r.map_err(|e| e.to_string()),
        Err(_) => Err(format!("the window could not start ({hint})")),
    }
}

struct App {
    def: &'static GameDef,
    seed: u64,
    sim: Sim,
    size: (u32, u32),
    /// Render at 1/scale resolution and enlarge (0 = choose by speed).
    scale: usize,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    keys: HashSet<KeyCode>,
    pressed: u32,
    mouse: Option<(f64, f64)>,
    mouse_held: u32,
    last: Instant,
    acc: f64,
    paused: bool,
    step_once: bool,
    follow: Follow,
    help: bool,
    help_pinned: bool,
    started: Instant,
    stats: bool,
    frame_ms: f32,
    /// Render scale chosen automatically when `scale` is 0.
    auto: usize,
    last_image: Option<Image>,
    message: Option<(String, Instant)>,
}

fn key_button(k: KeyCode) -> u32 {
    match k {
        KeyCode::Space => buttons::JUMP,
        KeyCode::KeyJ => buttons::FIRE,
        KeyCode::KeyK => buttons::ALT,
        KeyCode::KeyE => buttons::USE,
        KeyCode::KeyC | KeyCode::ControlLeft | KeyCode::ControlRight => buttons::CROUCH,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => buttons::DASH,
        _ => 0,
    }
}

impl App {
    fn input(&self) -> Input {
        let down = |k: &[KeyCode]| k.iter().any(|c| self.keys.contains(c));
        let x = down(&[KeyCode::KeyD, KeyCode::ArrowRight]) as i32 - down(&[KeyCode::KeyA, KeyCode::ArrowLeft]) as i32;
        let y = down(&[KeyCode::KeyW, KeyCode::ArrowUp]) as i32 - down(&[KeyCode::KeyS, KeyCode::ArrowDown]) as i32;
        let w = &self.sim.world;
        let yaw = w.camera.yaw.to_radians();
        let (fwd, right) = (Vec3::new(yaw.sin(), 0.0, -yaw.cos()), Vec3::new(yaw.cos(), 0.0, yaw.sin()));
        let m = right * x as f32 + fwd * y as f32;
        let held = self.keys.iter().fold(self.mouse_held, |b, k| b | key_button(*k));
        Input { move_dir: Vec2::new(m.x, m.z).normalize_or_zero(), aim: self.aim(), held, pressed: self.pressed }
    }

    /// The mouse on a plane through the player: horizontal at chest height, or vertical
    /// (x-y) when the camera looks from the side.
    fn aim(&self) -> Option<Vec3> {
        let (mx, my) = self.mouse?;
        let win = self.window.as_ref()?.inner_size();
        let w = &self.sim.world;
        let p = w.player()?;
        let cam = view::camera(w, self.follow.target);
        let aspect = win.width as f32 / win.height.max(1) as f32;
        let (u, v) = (mx as f32 / win.width.max(1) as f32 * 2.0 - 1.0, 1.0 - my as f32 / win.height.max(1) as f32 * 2.0);
        let (o, d) = cam.ray(u, v, aspect);
        let chest = p.pos.y + 1.0;
        if d.y.abs() > 0.2 {
            let t = (chest - o.y) / d.y;
            (t > 0.0).then(|| o + d * t)
        } else if d.z.abs() > 1e-3 {
            let t = (p.pos.z - o.z) / d.z;
            (t > 0.0).then(|| o + d * t)
        } else {
            None
        }
    }

    fn say(&mut self, text: String) {
        self.message = Some((text, Instant::now()));
    }

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64().min(0.25);
        self.last = now;
        let rewinding = self.keys.contains(&KeyCode::Backspace);
        if rewinding {
            self.sim.rewind(2);
            self.acc = 0.0;
        } else if !self.paused || self.step_once {
            self.acc += if self.step_once { TICK } else { dt };
            self.step_once = false;
            while self.acc >= TICK {
                let input = self.input();
                self.sim.step(&input);
                self.pressed = 0;
                self.acc -= TICK;
            }
        }
        self.follow.update(&self.sim.world, dt as f32);
        let (Some(window), Some(surface)) = (self.window.clone(), self.surface.as_mut()) else { return };
        let size = window.inner_size();
        let (ww, wh) = (size.width.max(1), size.height.max(1));
        if surface.resize(NonZeroU32::new(ww).unwrap(), NonZeroU32::new(wh).unwrap()).is_err() {
            return;
        }
        // Automatic scale: drop to half resolution when frames get slow, go back when there is
        // plenty of headroom (a quarter of the pixels costs about a quarter of the time).
        if self.auto == 1 && self.frame_ms > 15.0 && ww > 700 {
            self.auto = 2;
            self.frame_ms /= 4.0;
        } else if self.auto == 2 && self.frame_ms < 3.0 {
            self.auto = 1;
            self.frame_ms *= 4.0;
        }
        let scale = if self.scale > 0 { self.scale } else { self.auto };
        let (rw, rh) = ((ww as usize / scale).max(16), (wh as usize / scale).max(16));
        let t = Instant::now();
        let w = &self.sim.world;
        let scene = view::scene(w, self.sim.game.as_ref(), self.follow.target, rw, rh, None);
        let mut img = render::render(&scene, rw, rh, 1);
        self.frame_ms = self.frame_ms * 0.9 + t.elapsed().as_secs_f32() * 100.0;
        let s = (rh as f32 / 360.0).max(1.0);
        let si = s as usize;
        if self.help && self.started.elapsed() < Duration::from_secs(8) || self.help_pinned {
            let lines = [
                "WASD move  Space jump  J/LMB fire  K/RMB alt  E use  Shift dash",
                "Esc quit  F1 help  F5 restart  P pause  Backspace rewind  F12 shot",
            ];
            let hs = s.min(rw as f32 / (70.0 * 8.0)).max(1.0);
            for (i, l) in lines.iter().enumerate() {
                let y = rh as f32 - (2 - i) as f32 * 11.0 * hs - 4.0 * hs;
                img.text((8.0 * hs) as i32, y as i32, hs, Color::hex("#e8ecf2"), l);
            }
        }
        if self.stats || self.paused {
            let p = if self.paused { "  PAUSED" } else { "" };
            let line = format!("tick {}  {:.1} ms/frame  {}x{}{p}", w.tick, self.frame_ms, rw, rh);
            // Bottom right, above the help lines (the game's HUD owns the top).
            let y = rh as f32 - 34.0 * s;
            img.text(rw as i32 - crate::font::width(&line, s) - 8 * si as i32, y as i32, s, Color::hex("#9ef0ff"), &line);
        }
        if let Some((m, at)) = &self.message {
            if at.elapsed() < Duration::from_secs(3) {
                img.text(8 * si as i32, (rh / 2) as i32, s, Color::hex("#ffe14d"), m);
            }
        }
        let Ok(mut buf) = surface.buffer_mut() else { return };
        for y in 0..wh as usize {
            let sy = (y * rh / wh as usize).min(rh - 1);
            let src = &img.px[sy * rw..sy * rw + rw];
            let dst = &mut buf[y * ww as usize..(y + 1) * ww as usize];
            for (x, d) in dst.iter_mut().enumerate() {
                *d = src[(x * rw / ww as usize).min(rw - 1)];
            }
        }
        let _ = buf.present();
        self.last_image = Some(img);
    }

    fn key(&mut self, el: &ActiveEventLoop, code: KeyCode, down: bool, repeat: bool) {
        if down && !repeat {
            self.pressed |= key_button(code);
            match code {
                KeyCode::Escape => el.exit(),
                KeyCode::F1 => {
                    self.help_pinned = !(self.help_pinned || self.started.elapsed() < Duration::from_secs(8));
                    self.help = self.help_pinned;
                }
                KeyCode::Tab => self.stats = !self.stats,
                KeyCode::KeyP | KeyCode::F6 => self.paused = !self.paused,
                KeyCode::F7 => {
                    self.paused = true;
                    self.step_once = true;
                }
                KeyCode::F5 => {
                    self.sim = Sim::new(self.def, self.seed);
                    self.follow = Follow::default();
                    self.say("restarted".into());
                }
                KeyCode::F12 => {
                    if let Some(img) = &self.last_image {
                        let path = format!("out/screenshot-{}-{}.png", self.def.name, self.sim.world.tick);
                        let _ = std::fs::create_dir_all("out");
                        let msg = match std::fs::write(&path, img.png()) {
                            Ok(()) => format!("saved {path}"),
                            Err(e) => format!("screenshot failed: {e}"),
                        };
                        self.say(msg);
                    }
                }
                _ => {}
            }
        }
        if down {
            self.keys.insert(code);
        } else {
            self.keys.remove(&code);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(format!("Pavilion Lite - {}", self.def.name))
            .with_inner_size(winit::dpi::LogicalSize::new(self.size.0, self.size.1));
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("cannot open a window: {e}");
                el.exit();
                return;
            }
        };
        let surface = softbuffer::Context::new(window.clone()).and_then(|c| softbuffer::Surface::new(&c, window.clone()));
        match surface {
            Ok(s) => self.surface = Some(s),
            Err(e) => {
                eprintln!("cannot draw to the window: {e}");
                el.exit();
                return;
            }
        }
        self.window = Some(window);
        self.last = Instant::now();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::KeyboardInput {
                event: KeyEvent { physical_key: PhysicalKey::Code(code), state, repeat, .. }, ..
            } => self.key(el, code, state == ElementState::Pressed, repeat),
            WindowEvent::CursorMoved { position, .. } => self.mouse = Some((position.x, position.y)),
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => buttons::FIRE,
                    MouseButton::Right => buttons::ALT,
                    _ => 0,
                };
                if state == ElementState::Pressed {
                    self.mouse_held |= b;
                    self.pressed |= b;
                } else {
                    self.mouse_held &= !b;
                }
            }
            WindowEvent::Focused(false) => {
                self.keys.clear();
                self.mouse_held = 0;
            }
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        // Draw about 60 times a second (the simulation catches up by its own clock).
        el.set_control_flow(ControlFlow::WaitUntil(self.last + Duration::from_micros(16_000)));
    }
}
