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
    // The browser build always uses the browser's WebGPU.
    let backends = if cfg!(target_arch = "wasm32") { wgpu::Backends::BROWSER_WEBGPU } else { backend.backends() };
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
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
    pollster::block_on(request_adapter_async(instance, surface))
}

pub async fn request_adapter_async(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<wgpu::Adapter> {
    instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: surface,
            ..Default::default()
        })
        .await
        .map_err(|e| anyhow!("no suitable GPU adapter: {e}"))
}

pub fn request_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue)> {
    pollster::block_on(request_device_async(adapter))
}

pub async fn request_device_async(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue)> {
    let limits = wgpu::Limits::default().using_resolution(adapter.limits());
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("pavilion"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            ..Default::default()
        })
        .await
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
        let result = Self::with_backend(BackendChoice::Vulkan);
        // The window can use DX12 on a Windows system without a working Vulkan driver.
        // Its image tools create a separate device and must be able to use DX12 as well.
        #[cfg(windows)]
        let result = result.or_else(|vulkan_error| {
            log::info!("Vulkan capture GPU is unavailable; trying DirectX 12: {vulkan_error:#}");
            Self::with_backend(BackendChoice::Dx12)
                .with_context(|| format!("Vulkan capture GPU failed ({vulkan_error:#}); DirectX 12 fallback also failed"))
        });
        result
    }

    fn with_backend(backend: BackendChoice) -> Result<Self> {
        let instance = create_instance(backend);
        let adapter = request_adapter(&instance, None)?;
        let (device, queue) = request_device(&adapter)?;
        device.on_uncaptured_error(std::sync::Arc::new(|e| log::error!("GPU error: {e}")));
        Ok(Self { adapter, device, queue })
    }
}
