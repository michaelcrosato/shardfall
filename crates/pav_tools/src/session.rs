use anyhow::Result;
use glam::Vec3;
use pav_core::params::{ParamVisitor, Tunable, nested};
use pav_core::{InputFrame, RenderFrame, Sim, SimConfig};
use pav_render::{Renderer, gpu::Headless};
use pav_view::{CameraParams, CameraRig, ViewBuilder, ViewSettings};

pub struct Gpu {
    pub headless: Headless,
    pub renderer: Renderer,
    pub builder: ViewBuilder,
}

/// One agent session: a simulation plus everything needed to look at it.
pub struct Session {
    pub sim: Sim,
    pub camera: CameraRig,
    pub view: ViewSettings,
    pub input: InputFrame,
    pub gpu: Option<Gpu>,
    pub prev_frame: RenderFrame,
    /// Room whose camera defaults were applied last, and the camera before entering it.
    room_cam: Option<(u16, CameraParams)>,
    cue_serial: u64,
    cue_base: Option<CameraParams>,
    /// Events since the last render (they become particles and shockwaves in captures).
    events: Vec<pav_core::frame::SimEvent>,
    /// View settings before the current room's `[view]` table / pad overrides.
    view_base: Option<ViewSettings>,
    view_room: Option<u16>,
    view_serial: u64,
    /// Running inside the game (live bridge): the game applies room cameras and views itself.
    pub(crate) live: bool,
}

/// All tunables reachable by path: `sim.*`, `camera.*`, `view.*`.
pub struct ParamsRoot<'a> {
    pub sim: &'a mut SimConfig,
    pub camera: &'a mut CameraParams,
    pub view: &'a mut ViewSettings,
}

impl Tunable for ParamsRoot<'_> {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        self.sim.visit_groups(v);
        nested(v, "camera", self.camera);
        nested(v, "view", self.view);
    }
}

impl Session {
    pub fn new(scene: &str, seed: u64) -> Result<Self> {
        let mut sim = Sim::new(scene, seed)?;
        let mut camera = CameraRig::default();
        camera.snap(sim.state.focus);
        let prev_frame = sim.frame();
        let mut s = Self {
            sim,
            camera,
            view: ViewSettings::default(),
            input: InputFrame::default(),
            gpu: None,
            prev_frame,
            room_cam: None,
            cue_serial: 0,
            cue_base: None,
            events: Vec::new(),
            view_base: None,
            view_room: None,
            view_serial: 0,
            live: false,
        };
        s.sync_camera();
        Ok(s)
    }

    /// Wraps the game's running simulation, camera and view for one live-bridge request.
    pub fn from_live(mut sim: Sim, camera: CameraRig, view: ViewSettings, gpu: Option<Gpu>) -> Self {
        let prev_frame = sim.frame();
        Self {
            sim,
            camera,
            view,
            input: InputFrame::default(),
            gpu,
            prev_frame,
            room_cam: None,
            cue_serial: 0,
            cue_base: None,
            events: Vec::new(),
            view_base: None,
            view_room: None,
            view_serial: 0,
            live: true,
        }
    }

    /// Hands everything back to the game after a live-bridge request.
    pub fn into_live(self) -> (Sim, CameraRig, ViewSettings, Option<Gpu>) {
        (self.sim, self.camera, self.view, self.gpu)
    }

    pub fn is_live(&self) -> bool {
        self.live
    }

    pub fn params(&mut self) -> ParamsRoot<'_> {
        ParamsRoot { sim: &mut self.sim.config, camera: &mut self.camera.params, view: &mut self.view }
    }

    pub fn gpu(&mut self) -> Result<&mut Gpu> {
        if self.gpu.is_none() {
            let headless = Headless::new()?;
            let renderer = Renderer::new(&headless.device, &headless.queue);
            self.gpu = Some(Gpu { headless, renderer, builder: ViewBuilder::new() });
        }
        Ok(self.gpu.as_mut().unwrap())
    }

    pub fn step(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.sim.step(&self.input);
            self.input.pressed = 0;
        }
        self.keep_events();
        self.sync_camera();
    }

    /// Moves the simulation's new events into the render queue (bounded).
    pub fn keep_events(&mut self) {
        let ev = self.sim.drain_events();
        self.events.extend(ev);
        if self.events.len() > 400 {
            let cut = self.events.len() - 400;
            self.events.drain(..cut);
        }
    }

    /// Applies a room's camera defaults when the player enters/leaves it, and level camera
    /// cues, the way the game does (without blending). Called after every step.
    pub fn sync_camera(&mut self) {
        use pav_core::params::{ParamValue, apply_map};
        if self.live {
            return;
        }
        let w = &self.sim.state.world;
        let now = w.current_room;
        let rot = |yaw: f64, q: u8| ParamValue::Float((yaw + q as f64 * 90.0 + 540.0).rem_euclid(360.0) - 180.0);
        let c = &self.sim.state.courses;
        if c.cue_serial != self.cue_serial {
            self.cue_serial = c.cue_serial;
            // Like the game: each cue applies on top of the camera from before the first cue;
            // sweeps start at their `from` value (captures are single moments).
            match c.cue() {
                Some(cue) => {
                    let q = now.and_then(|i| w.rooms.get(i as usize)).map(|r| r.place.quarters).unwrap_or(0);
                    let base = self.cue_base.get_or_insert_with(|| self.camera.params.clone()).clone();
                    let mut set = cue.set.clone();
                    for sw in &cue.sweep {
                        set.insert(sw.param.clone(), ParamValue::Float(sw.from as f64));
                    }
                    if let Some(y) = set.get("yaw").and_then(|v| v.as_f64()) {
                        set.insert("yaw".into(), rot(y, q));
                    }
                    self.camera.params = base;
                    apply_map(&mut self.camera.params, &set);
                }
                None => {
                    if let Some(b) = self.cue_base.take() {
                        self.camera.params = b;
                    }
                }
            }
        }
        // Room changes after cues (leaving a room clears its cue first, then restores the camera).
        if now != self.room_cam.as_ref().map(|r| r.0) {
            if let Some((_, saved)) = self.room_cam.take() {
                self.camera.params = saved;
            }
            if let Some(r) = now.and_then(|i| w.rooms.get(i as usize)).filter(|r| !r.def.camera.is_empty()) {
                let saved = self.camera.params.clone();
                let mut cam = r.def.camera.clone();
                let yaw = cam.get("yaw").and_then(|v| v.as_f64()).unwrap_or(0.0);
                cam.insert("yaw".into(), rot(yaw, r.place.quarters));
                apply_map(&mut self.camera.params, &cam);
                self.room_cam = Some((r.id, saved));
            }
        }
        self.sync_view();
    }

    /// Takes `params` as the base camera and applies the current room's `[camera]` on top.
    pub fn reset_camera(&mut self, params: CameraParams) {
        self.camera.params = params;
        self.room_cam = None;
        self.cue_base = None;
        self.sync_camera();
    }

    /// The camera without the current room's defaults.
    pub fn camera_base_or_current(&self) -> CameraParams {
        self.room_cam.as_ref().map(|(_, c)| c.clone()).unwrap_or_else(|| self.camera.params.clone())
    }

    /// The view settings without the current room's / pads' overrides.
    pub fn view_base_or_current(&self) -> ViewSettings {
        self.view_base.clone().unwrap_or_else(|| self.view.clone())
    }

    /// Takes `view` as the base settings and applies the current room's `[view]` table on top.
    pub fn reset_view(&mut self, view: ViewSettings) {
        self.view = view;
        self.view_base = None;
        self.view_room = None;
        self.view_serial = u64::MAX;
        self.sync_view();
    }

    /// Room `[view]` tables and pad `view.*` overrides, like the game applies them.
    fn sync_view(&mut self) {
        let w = &self.sim.state.world;
        let now = w.current_room;
        let serial = self.sim.state.courses.view_serial;
        if now == self.view_room && serial == self.view_serial {
            return;
        }
        if let Some(base) = self.view_base.take() {
            self.view = base;
        }
        self.view_room = now;
        self.view_serial = serial;
        let q = now.and_then(|i| w.rooms.get(i as usize)).map(|r| r.place.quarters).unwrap_or(0);
        let room =
            now.and_then(|i| w.rooms.get(i as usize)).map(|r| pav_view::build::room_view_map(&r.def.view, q)).unwrap_or_default();
        let pads = pav_view::build::room_view_map(&self.sim.state.courses.view, q);
        if room.is_empty() && pads.is_empty() {
            return;
        }
        self.view_base = Some(self.view.clone());
        self.view.apply(&room);
        self.view.apply(&pads);
    }

    /// Renders the current state to RGBA8 pixels.
    pub fn render(&mut self, width: u32, height: u32) -> Result<Vec<u8>> {
        let frame = self.sim.frame();
        let focus = frame.focus;
        self.camera.snap(focus);
        let camera = self.camera.clone();
        let view = self.view.clone();
        let events = std::mem::take(&mut self.events);
        let gpu = self.gpu()?;
        gpu.builder.add_events(&events);
        let scene = gpu.builder.build(&frame, &frame, 1.0, &camera, width as f32 / height as f32, &view, focus);
        pav_render::capture::render_to_rgba(&mut gpu.renderer, &scene, width, height)
    }

    pub fn focus(&self) -> Vec3 {
        self.sim.state.focus
    }
}
