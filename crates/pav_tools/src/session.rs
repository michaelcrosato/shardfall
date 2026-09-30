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
        let sim = Sim::new(scene, seed)?;
        let mut camera = CameraRig::default();
        camera.snap(sim.state.focus);
        let prev_frame = sim.frame();
        Ok(Self { sim, camera, view: ViewSettings::default(), input: InputFrame::default(), gpu: None, prev_frame })
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
        self.sim.drain_events();
    }

    /// Renders the current state to RGBA8 pixels.
    pub fn render(&mut self, width: u32, height: u32) -> Result<Vec<u8>> {
        let frame = self.sim.frame();
        let focus = frame.focus;
        self.camera.snap(focus);
        let camera = self.camera.clone();
        let view = self.view.clone();
        let gpu = self.gpu()?;
        let scene = gpu.builder.build(&frame, &frame, 1.0, &camera, width as f32 / height as f32, &view, focus);
        pav_render::capture::render_to_rgba(&mut gpu.renderer, &scene, width, height)
    }

    pub fn focus(&self) -> Vec3 {
        self.sim.state.focus
    }
}
