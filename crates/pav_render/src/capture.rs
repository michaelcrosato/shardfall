//! Offscreen rendering to RGBA8 pixels and PNG encoding.

use anyhow::{Context, Result};

use crate::renderer::Renderer;
use crate::scene::Scene;

pub const CAPTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Renders `scene` at `width` x `height` and returns tightly packed RGBA8 pixels.
pub fn render_to_rgba(renderer: &mut Renderer, scene: &Scene, width: u32, height: u32) -> Result<Vec<u8>> {
    let device = renderer.device().clone();
    let queue = renderer.queue().clone();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("capture"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: CAPTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("capture") });
    renderer.render(&mut encoder, scene, &view, CAPTURE_FORMAT, (width, height));
    read_texture(&device, &queue, encoder, &texture, width, height)
}

/// Copies an RGBA8 texture back to the CPU (submits `encoder` first).
pub fn read_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mut encoder: wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let padded = (width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("capture readback"),
        size: (padded * height) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(height) },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).context("GPU poll failed")?;
    rx.recv().context("map callback dropped")?.context("buffer map failed")?;
    let data = slice.get_mapped_range().map_err(|e| anyhow::anyhow!("map range: {e:?}"))?;
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height as usize {
        let start = row * padded as usize;
        out.extend_from_slice(&data[start..start + width as usize * 4]);
    }
    drop(data);
    buffer.unmap();
    Ok(out)
}

pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header()?;
        w.write_image_data(rgba)?;
    }
    Ok(out)
}

pub fn save_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    std::fs::write(path, encode_png(width, height, rgba)?).with_context(|| format!("writing {}", path.display()))
}

/// Tiles equally sized RGBA frames into a grid image (a "filmstrip").
pub fn tile_frames(frames: &[Vec<u8>], width: u32, height: u32, columns: u32) -> (u32, u32, Vec<u8>) {
    let columns = columns.max(1).min(frames.len().max(1) as u32);
    let rows = (frames.len() as u32).div_ceil(columns).max(1);
    let (w, h) = (width * columns, height * rows);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for (i, f) in frames.iter().enumerate() {
        let (cx, cy) = (i as u32 % columns, i as u32 / columns);
        for y in 0..height {
            let src = (y * width * 4) as usize;
            let dst = (((cy * height + y) * w + cx * width) * 4) as usize;
            out[dst..dst + (width * 4) as usize].copy_from_slice(&f[src..src + (width * 4) as usize]);
        }
    }
    (w, h, out)
}
