//! GPU device creation. Vulkan by default; DX12 selectable on Windows.

use anyhow::{Context, Result, anyhow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendChoice {
    Vulkan,
    Dx12,
}

impl BackendChoice {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "vulkan" | "vk" => Some(Self::Vulkan),
            "dx12" | "d3d12" => Some(Self::Dx12),
            _ => None,
        }
    }
    pub fn backends(self) -> wgpu::Backends {
        match self {
            Self::Vulkan => wgpu::Backends::VULKAN,
            Self::Dx12 => wgpu::Backends::DX12,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Vulkan => "vulkan",
            Self::Dx12 => "dx12",
        }
    }
}

pub fn create_instance(backend: BackendChoice) -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: backend.backends(),
        flags: wgpu::InstanceFlags::from_env_or_default(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
}

pub fn describe(info: &wgpu::AdapterInfo) -> String {
    format!("{} ({:?}, {:?}, driver: {} {})", info.name, info.backend, info.device_type, info.driver, info.driver_info)
}

/// Lists every adapter the instance can see (for diagnostics).
pub fn list_adapters(instance: &wgpu::Instance, backend: BackendChoice) -> Vec<wgpu::AdapterInfo> {
    pollster::block_on(instance.enumerate_adapters(backend.backends())).iter().map(|a| a.get_info()).collect()
}

pub fn request_adapter(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<wgpu::Adapter> {
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: surface,
        ..Default::default()
    }))
    .map_err(|e| anyhow!("no suitable GPU adapter: {e}"))
}

pub fn request_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue)> {
    let limits = wgpu::Limits::default().using_resolution(adapter.limits());
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("pavilion"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        ..Default::default()
    }))
    .context("GPU device creation failed")
}

/// A device without a window, for captures and tests.
pub struct Headless {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Headless {
    pub fn new() -> Result<Self> {
        let instance = create_instance(BackendChoice::Vulkan);
        let adapter = request_adapter(&instance, None)?;
        let (device, queue) = request_device(&adapter)?;
        device.on_uncaptured_error(std::sync::Arc::new(|e| log::error!("GPU error: {e}")));
        Ok(Self { adapter, device, queue })
    }
}
