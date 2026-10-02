//! Window-bound graphics: surface, device, scene renderer and egui renderer.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use pav_render::gpu::{self, BackendChoice};
use pav_render::wgpu;
use pav_render::{Renderer, Scene};
use winit::window::Window;

use crate::boot::{stage, stage_async};

pub struct Gfx {
    pub window: Arc<Window>,
    pub surface: wgpu::Surface<'static>,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub renderer: Renderer,
    pub egui: egui_wgpu::Renderer,
    pub vsync: bool,
    pub adapter_name: String,
    pub gpu_errors: Arc<std::sync::atomic::AtomicU32>,
}

impl Gfx {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(window: Arc<Window>, backend: BackendChoice, vsync: bool) -> Result<Self> {
        pollster_block(Self::create(window, backend, vsync))
    }

    /// Creates everything; only the adapter and device requests wait (in the browser).
    pub async fn create(window: Arc<Window>, backend: BackendChoice, vsync: bool) -> Result<Self> {
        let web = cfg!(target_arch = "wasm32");
        let instance = stage("graphics instance", || {
            Ok((gpu::create_instance(backend), format!("backend: {}", if web { "webgpu" } else { backend.name() })))
        })?;
        let surface = stage("window surface", || {
            let s = instance.create_surface(window.clone()).context("could not create a drawing surface for the window")?;
            Ok((s, String::new()))
        })?;
        let adapter = stage_async("graphics adapter", async {
            if !web {
                let all = gpu::list_adapters(&instance, backend);
                for a in &all {
                    log::info!("  found adapter: {}", gpu::describe(a));
                }
                if all.is_empty() {
                    return Err(anyhow!(
                        "no {} adapter found. Update your graphics driver{}",
                        backend.name(),
                        if backend == BackendChoice::Vulkan { ", or try backend = \"dx12\" in shardfall.toml" } else { "" }
                    ));
                }
            }
            let a = gpu::request_adapter_async(&instance, Some(&surface)).await.map_err(|e| {
                if web { anyhow!("{e:#}. This page needs a browser with WebGPU (a recent Chrome or Edge)") } else { e }
            })?;
            let d = gpu::describe(&a.get_info());
            Ok((a, d))
        })
        .await?;
        let (device, queue) = stage_async("graphics device", async {
            let (d, q) = gpu::request_device_async(&adapter).await?;
            Ok(((d, q), String::new()))
        })
        .await?;
        let gpu_errors = Arc::new(std::sync::atomic::AtomicU32::new(0));
        {
            let errs = gpu_errors.clone();
            device.on_uncaptured_error(Arc::new(move |e| {
                errs.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                log::error!("GPU error: {e}");
            }));
            device.set_device_lost_callback(|reason, msg| log::error!("GPU device lost ({reason:?}): {msg}"));
        }
        let size = window.inner_size();
        let config = stage("surface configure", || {
            let caps = surface.get_capabilities(&adapter);
            let mut c = surface
                .get_default_config(&adapter, size.width.max(1), size.height.max(1))
                .context("surface not supported by adapter")?;
            // Non-sRGB target: the post pass encodes sRGB itself, and egui expects a linear-blend target.
            c.format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(c.format);
            c.present_mode = present_mode(&caps, vsync);
            surface.configure(&device, &c);
            Ok((c.clone(), format!("{:?}, {:?}, {}x{}", c.format, c.present_mode, c.width, c.height)))
        })?;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let renderer = stage("renderer & shaders", || Ok((Renderer::new(&device, &queue), String::new())))?;
        if let Some(e) = scope.pop().await {
            return Err(anyhow!("shader/pipeline creation failed: {e}"));
        }
        let egui = stage("ui renderer", || {
            Ok((egui_wgpu::Renderer::new(&device, config.format, egui_wgpu::RendererOptions::default()), String::new()))
        })?;
        let adapter_name = gpu::describe(&adapter.get_info());
        Ok(Self { window, surface, adapter, device, queue, config, renderer, egui, vsync, adapter_name, gpu_errors })
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn set_vsync(&mut self, on: bool) {
        self.vsync = on;
        let caps = self.surface.get_capabilities(&self.adapter);
        self.config.present_mode = present_mode(&caps, on);
        self.surface.configure(&self.device, &self.config);
    }

    /// Acquires the next surface texture, reconfiguring when needed. None = skip this frame.
    pub fn acquire(&mut self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                // Use this frame, reconfigure for the next one.
                self.surface.configure(&self.device, &self.config);
                Some(t)
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                None
            }
            _ => None,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Renders the scene plus the egui overlay and presents. Texture updates are always
    /// applied (and the delta cleared), even when the frame is skipped.
    pub fn draw(&mut self, scene: &Scene, ui: Option<(&[egui::ClippedPrimitive], &mut egui::TexturesDelta, f32)>) {
        let mut ui = ui;
        if let Some((_, textures, _)) = ui.as_mut() {
            for (id, deltas) in &textures.set {
                for delta in deltas {
                    self.egui.update_texture(&self.device, &self.queue, *id, delta);
                }
            }
        }
        let frame = self.acquire();
        if let Some(frame) = frame {
            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
            let size = self.size();
            self.renderer.render(&mut encoder, scene, &view, self.config.format, size);
            let mut cmds = Vec::new();
            if let Some((prims, _, ppp)) = ui.as_ref() {
                let sd = egui_wgpu::ScreenDescriptor { size_in_pixels: [size.0, size.1], pixels_per_point: *ppp };
                cmds = self.egui.update_buffers(&self.device, &self.queue, &mut encoder, prims, &sd);
                let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                self.egui.render(&mut pass.forget_lifetime(), prims, &sd);
            }
            self.queue.submit(cmds.into_iter().chain([encoder.finish()]));
            self.window.pre_present_notify();
            self.queue.present(frame);
        }
        if let Some((_, textures, _)) = ui {
            for id in &textures.free {
                self.egui.free_texture(id);
            }
            textures.clear();
        }
    }
}

fn present_mode(caps: &wgpu::SurfaceCapabilities, vsync: bool) -> wgpu::PresentMode {
    let want: &[wgpu::PresentMode] = if vsync {
        &[wgpu::PresentMode::Fifo]
    } else {
        &[wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate, wgpu::PresentMode::Fifo]
    };
    want.iter().copied().find(|m| caps.present_modes.contains(m)).unwrap_or(caps.present_modes[0])
}

#[cfg(not(target_arch = "wasm32"))]
fn pollster_block<F: std::future::Future>(f: F) -> F::Output {
    // Minimal executor (the futures here resolve immediately on native).
    use std::task::{Context as Cx, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop(_: *const ()) {}
    fn clone(p: *const ()) -> RawWaker {
        RawWaker::new(p, &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Cx::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}
