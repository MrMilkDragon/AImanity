use iced::wgpu;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg8Unorm;

pub(super) fn mask(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("Copilot mask"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

pub(super) struct Draw {
    particles: wgpu::RenderPipeline,
    waves: wgpu::RenderPipeline,
    scene: wgpu::BindGroup,
}

impl Draw {
    pub(super) fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        uniform: &wgpu::Buffer,
    ) -> Self {
        let globals = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Copilot scene"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let scene = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Copilot scene"),
            layout: &globals,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Copilot mask"),
            bind_group_layouts: &[&globals],
            push_constant_ranges: &[],
        });
        let pipeline = |entry, topology, write_mask| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("copilot_stroke"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                multisample: Default::default(),
                depth_stencil: None,
                multiview: None,
                cache: None,
            })
        };
        Self {
            particles: pipeline(
                "copilot_particle",
                wgpu::PrimitiveTopology::TriangleList,
                wgpu::ColorWrites::RED,
            ),
            waves: pipeline(
                "copilot_wave",
                wgpu::PrimitiveTopology::TriangleStrip,
                wgpu::ColorWrites::GREEN,
            ),
            scene,
        }
    }

    pub(super) fn render(&self, mask: &wgpu::TextureView, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Copilot mask"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: mask,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.particles);
        pass.set_bind_group(0, &self.scene, &[]);
        pass.draw(0..6, 0..(48 * 16));
        pass.set_pipeline(&self.waves);
        pass.draw(0..(6 * 24 + 1) * 2, 0..4);
    }
}
