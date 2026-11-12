//! wgpu device and swapchain setup.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use winit::window::Window;

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    /// True when the swapchain blends with the desktop behind the window.
    pub transparent: bool,
    /// Largest surface side the device can handle.
    max_size: u32,
}

impl Gpu {
    pub fn new(window: Arc<Window>, want_transparent: bool) -> Result<Self> {
        let size = window.inner_size();

        // DX12 first: with a DirectComposition swapchain it is the only backend
        // that gives per-pixel window transparency on Windows.
        let mut found = None;
        let mut last_err = None;
        for backends in [wgpu::Backends::DX12, wgpu::Backends::all()] {
            let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
            desc.backends = backends;
            if want_transparent {
                desc.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
            }
            let instance = wgpu::Instance::new(desc.with_env());
            let surface = match instance.create_surface(window.clone()) {
                Ok(s) => s,
                Err(e) => {
                    last_err = Some(anyhow!(e));
                    continue;
                }
            };
            let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                ..Default::default()
            }));
            match adapter {
                Ok(adapter) => {
                    found = Some((surface, adapter));
                    break;
                }
                Err(e) => last_err = Some(anyhow!(e)),
            }
        }
        let (surface, adapter) = found
            .ok_or_else(|| last_err.unwrap_or_else(|| anyhow!("no adapter")))
            .context("no usable graphics adapter found")?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("nullshell"),
            ..Default::default()
        }))
        .context("failed to create graphics device")?;

        // A wgpu validation error panics by default. Log it instead: a bad
        // frame is better than a dead terminal.
        device.on_uncaptured_error(Arc::new(|e| crate::log(&format!("gpu error: {e}"))));
        let max_size = device.limits().max_texture_dimension_2d;

        let caps = surface.get_capabilities(&adapter);
        // All color math is done in gamma space, like most terminals do, so a
        // non-sRGB surface format is wanted.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .context("surface has no supported formats")?;

        let alpha_mode = if want_transparent
            && caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto)
        };
        let transparent = alpha_mode == wgpu::CompositeAlphaMode::PreMultiplied;

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: Default::default(),
            width: size.width.clamp(1, max_size),
            height: size.height.clamp(1, max_size),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        Ok(Self { device, queue, surface, config, transparent, max_size })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width.min(self.max_size);
        self.config.height = height.min(self.max_size);
        self.surface.configure(&self.device, &self.config);
    }

    pub fn reconfigure(&self) {
        self.surface.configure(&self.device, &self.config);
    }
}
