use std::sync::Arc;

use anyhow::Result;
use tracing::instrument;
use winit::window::Window;

/// GPU renderer managing wgpu surface, device, and queue.
pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
}

impl Renderer {
    /// Initialize wgpu: request adapter, device, and configure the surface.
    #[instrument(skip_all)]
    pub async fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = instance
            .create_surface(window)
            .map_err(|e| anyhow::anyhow!("Failed to create wgpu surface: {e}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to find a suitable GPU adapter: {e}"))?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("surfterm_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create wgpu device: {e}"))?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        tracing::info!(
            width = size.width,
            height = size.height,
            format = ?surface_format,
            "wgpu renderer initialized"
        );

        Ok(Self {
            device,
            queue,
            surface,
            config,
            size,
        })
    }

    /// Reconfigure the surface after a window resize.
    #[instrument(skip(self))]
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            tracing::debug!(
                width = new_size.width,
                height = new_size.height,
                "Surface resized"
            );
        }
    }

    /// Render a frame: clear the screen with the dark theme background color (#1e1e2e).
    #[instrument(skip(self))]
    pub fn render(&mut self) -> Result<()> {
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                tracing::warn!("Surface texture is suboptimal, consider reconfiguring");
                texture
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                return Err(anyhow::anyhow!("Timeout acquiring surface texture"));
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                return Err(anyhow::anyhow!("Surface texture is outdated"));
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                return Err(anyhow::anyhow!("Surface texture is lost"));
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                return Err(anyhow::anyhow!("Surface is occluded"));
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(anyhow::anyhow!("Surface texture validation error"));
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("surfterm_encoder"),
            });

        // Dark theme background: #1e1e2e (Catppuccin Mocha base)
        let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0x1e as f64 / 255.0,
                        g: 0x1e as f64 / 255.0,
                        b: 0x2e as f64 / 255.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        drop(_render_pass);

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

#[cfg(test)]
mod tests {}
