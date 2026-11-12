//! Post-process pass. The scene is drawn into an offscreen texture, then this
//! pass copies it to the window through `shaders/post.wgsl`.

/// Format of the offscreen scene texture.
pub const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Per-frame inputs of the post shader. Layout matches `struct Post` in WGSL.
#[derive(Clone, Copy, Default)]
pub struct PostUniforms {
    pub resolution: [f32; 2],
    pub time: f32,
    pub glitch: f32,
    pub scanlines: f32,
    pub glow: f32,
    pub aberration: f32,
    pub vignette: f32,
    pub scan_beam: f32,
    pub noise: f32,
    pub scale: f32,
}

struct Target {
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

pub struct PostPipeline {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    sampler: wgpu::Sampler,
    target: Target,
}

impl PostPipeline {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/post.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("post"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post uniforms"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scene"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let target = Self::make_target(device, &pipeline, &uniforms, &sampler, width, height);
        Self { pipeline, uniforms, sampler, target }
    }

    fn make_target(
        device: &wgpu::Device,
        pipeline: &wgpu::RenderPipeline,
        uniforms: &wgpu::Buffer,
        sampler: &wgpu::Sampler,
        width: u32,
        height: u32,
    ) -> Target {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scene target"),
            size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SCENE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        });
        Target { view, bind_group }
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.target = Self::make_target(device, &self.pipeline, &self.uniforms, &self.sampler, width, height);
    }

    /// The texture view the scene pass must render into.
    pub fn scene_view(&self) -> &wgpu::TextureView {
        &self.target.view
    }

    pub fn set_uniforms(&self, queue: &wgpu::Queue, u: PostUniforms) {
        let data: [f32; 12] = [
            u.resolution[0], u.resolution[1], u.time, u.glitch,
            u.scanlines, u.glow, u.aberration, u.vignette,
            u.scan_beam, u.noise, u.scale, 0.0,
        ];
        queue.write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&data));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.target.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
