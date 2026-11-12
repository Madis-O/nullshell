//! The scene: a flat, ordered list of quads, plus the pipeline that draws it.
//!
//! Layers (terminal grid, HUD, anything added later) only push quads into a
//! [`Scene`]; they never touch wgpu directly.

use bytemuck::{Pod, Zeroable};

use super::post::SCENE_FORMAT;

/// Corner bits for [`Scene::cut_rect`].
pub mod corner {
    pub const TL: u8 = 1;
    pub const TR: u8 = 2;
    pub const BR: u8 = 4;
    pub const BL: u8 = 8;
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Instance {
    pos: [f32; 2],
    size: [f32; 2],
    uv: [f32; 4],
    color: [f32; 4],
    params: [f32; 4],
}

/// Quads are drawn in the order they were pushed. Colors are straight alpha
/// `[r, g, b, a]` in 0..1, positions are pixels from the top left.
#[derive(Default)]
pub struct Scene {
    instances: Vec<Instance>,
}

impl Scene {
    pub fn clear(&mut self) {
        self.instances.clear();
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.instances.push(Instance { pos: [x, y], size: [w, h], uv: [0.0; 4], color, params: [0.0; 4] });
    }

    /// A glyph from the atlas. `colored` glyphs (emoji) ignore the color's rgb.
    pub fn glyph(&mut self, x: f32, y: f32, w: f32, h: f32, uv: [f32; 4], color: [f32; 4], colored: bool) {
        let mode = if colored { 2.0 } else { 1.0 };
        self.instances.push(Instance { pos: [x, y], size: [w, h], uv, color, params: [mode, 0.0, 0.0, 0.0] });
    }

    /// Circle stroke clipped to the given rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn arc(&mut self, x: f32, y: f32, w: f32, h: f32, center: [f32; 2], radius: f32, thickness: f32, color: [f32; 4]) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.instances.push(Instance {
            pos: [x, y],
            size: [w, h],
            uv: [center[0], center[1], 0.0, 0.0],
            color,
            params: [3.0, radius, thickness, 0.0],
        });
    }

    /// Rectangle with 45 degree cuts on the chosen corners.
    /// `border` > 0 draws only an outline of that thickness.
    #[allow(clippy::too_many_arguments)]
    pub fn cut_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4], cut: f32, corners: u8, border: f32) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.instances.push(Instance {
            pos: [x, y],
            size: [w, h],
            uv: [0.0; 4],
            color,
            params: [4.0, cut, border, corners as f32],
        });
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.instances.len()
    }
}

pub struct ScenePipeline {
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    sampler: wgpu::Sampler,
    bind_group: Option<wgpu::BindGroup>,
    atlas_generation: u64,
    instances: wgpu::Buffer,
    capacity: usize,
    count: u32,
}

impl ScenePipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/scene.wgsl").into()),
        });
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let capacity = 4096;
        let instances = Self::instance_buffer(device, capacity);
        Self { pipeline, globals, sampler, bind_group: None, atlas_generation: u64::MAX, instances, capacity, count: 0 }
    }

    fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene instances"),
            size: (capacity * std::mem::size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Copy the scene to the GPU. `atlas_generation` changes whenever the
    /// atlas texture was recreated.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        screen: [f32; 2],
        atlas: &wgpu::TextureView,
        atlas_generation: u64,
    ) {
        queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&[screen[0], screen[1], 0.0, 0.0]));

        if self.bind_group.is_none() || self.atlas_generation != atlas_generation {
            self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.globals.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(atlas) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            }));
            self.atlas_generation = atlas_generation;
        }

        if scene.instances.len() > self.capacity {
            self.capacity = scene.instances.len().next_power_of_two();
            self.instances = Self::instance_buffer(device, self.capacity);
        }
        if !scene.instances.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&scene.instances));
        }
        self.count = scene.instances.len() as u32;
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(bind_group) = &self.bind_group else { return };
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..4, 0..self.count);
    }
}
