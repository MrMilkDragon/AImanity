use crate::{board, style};
use iced::{Rectangle, wgpu, widget::shader};
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Uniforms {
    size: [f32; 4],
    chat: [f32; 4],
    popup: [f32; 4],
    child_popup: [f32; 4],
    radii_and_clip: [f32; 4],
    material: [f32; 4],
    background: [f32; 4],
    mode_effects: [f32; 4],
    mode_motion: [f32; 4],
}

#[derive(Debug)]
pub struct Primitive {
    uniforms: Uniforms,
    tiles: Vec<board::Tile>,
}
impl Primitive {
    pub fn new(scene: &crate::scene::Scene) -> Self {
        let rect = |r: Rectangle| [r.x, r.y, r.width, r.height];
        let mut background = style::BACKGROUND.into_linear();
        background[3] = scene.child_popup_opacity;
        let tiles = scene
            .layout
            .tiles
            .iter()
            .zip(&scene.devices)
            .filter(|(tile, _)| {
                tile.visible() && tile.screen.expand(25.).intersects(&scene.layout.board)
            })
            .map(|(tile, d)| board::Tile {
                id: tile.id.clone(),
                frame: d.frame.clone(),
                material: board::Material {
                    bounds: rect(tile.screen),
                    shape: [tile.radius, if d.frame.is_some() { 1. } else { 0. }, 0., 0.],
                    markers: d.markers,
                },
            })
            .collect();
        Self {
            tiles,
            uniforms: Uniforms {
                size: [scene.size.width, scene.size.height, 0., scene.popup_opacity],
                chat: rect(scene.layout.chat),
                popup: rect(scene.popup),
                child_popup: rect(scene.child_popup),
                radii_and_clip: [
                    scene.layout.grid_origin.x,
                    scene.layout.grid_origin.y,
                    crate::layout::STAGE_TOP,
                    scene.layout.grid_spacing,
                ],
                material: [
                    style::PANEL_RADIUS,
                    style::GLASS_OPACITY,
                    style::GLASS_BLUR,
                    style::BORDER_WIDTH,
                ],
                background,
                mode_effects: scene.mode_effects,
                mode_motion: scene.mode_motion,
            },
        }
    }
}
pub struct Pipeline {
    render: wgpu::RenderPipeline,
    paper: wgpu::RenderPipeline,
    devices: wgpu::RenderPipeline,
    small_devices: wgpu::RenderPipeline,
    board: board::Board,
    tile_layout: wgpu::BindGroupLayout,
    backdrop: wgpu::RenderPipeline,
    horizontal: wgpu::RenderPipeline,
    vertical: wgpu::RenderPipeline,
    glass_layout: wgpu::BindGroupLayout,
    glass_views: [wgpu::TextureView; 2],
    glass_groups: [wgpu::BindGroup; 2],
    glass_key: Option<[f32; 6]>,
    group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
}
fn texture(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("glass-backdrop"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}
impl Pipeline {
    fn glass_buffers(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        width: u32,
        height: u32,
    ) -> ([wgpu::TextureView; 2], [wgpu::BindGroup; 2]) {
        let views = std::array::from_fn(|_| {
            texture(device, width, height).create_view(&Default::default())
        });
        let groups = std::array::from_fn(|index| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glass-source"),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[index]),
                }],
            })
        });
        (views, groups)
    }
}
impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("AImanity glass"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene-layout"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let binding = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty,
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene-layout"),
            entries: &[
                binding(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                binding(
                    3,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
        let glass_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass-layout"),
            entries: &[binding(
                0,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            )],
        });
        let tile_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("device frame and geometry"),
            entries: &[
                binding(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                binding(
                    1,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let make_pipeline = |entry: &str, format, layouts: &[&wgpu::BindGroupLayout]| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: layouts,
                push_constant_ranges: &[],
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(if entry == "device_pixel" {
                        "device_vertex"
                    } else {
                        "vertex"
                    }),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: (entry == "fragment" || entry == "device_pixel")
                            .then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let paper = make_pipeline("backdrop", format, &[&layout]);
        let devices = make_pipeline("device_pixel", format, &[&layout, &tile_layout]);
        let small_devices = make_pipeline(
            "device_pixel",
            wgpu::TextureFormat::Rgba8UnormSrgb,
            &[&layout, &tile_layout],
        );
        let render = make_pipeline("fragment", format, &[&layout, &glass_layout]);
        let backdrop = make_pipeline("backdrop", wgpu::TextureFormat::Rgba8UnormSrgb, &[&layout]);
        let horizontal = make_pipeline(
            "horizontal",
            wgpu::TextureFormat::Rgba8UnormSrgb,
            &[&layout, &glass_layout],
        );
        let vertical = make_pipeline(
            "vertical",
            wgpu::TextureFormat::Rgba8UnormSrgb,
            &[&layout, &glass_layout],
        );
        let (glass_views, glass_groups) = Self::glass_buffers(device, &glass_layout, 1, 1);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("workspace material"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Self {
            render,
            paper,
            devices,
            small_devices,
            tile_layout,
            board: Default::default(),
            backdrop,
            horizontal,
            vertical,
            glass_layout,
            glass_views,
            glass_groups,
            glass_key: None,
            group,
            uniform,
        }
    }
}
impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;
    fn prepare(
        &self,
        pipeline: &mut Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _: &Rectangle,
        _: &shader::Viewport,
    ) {
        queue.write_buffer(&pipeline.uniform, 0, bytemuck::bytes_of(&self.uniforms));
        let frames_changed =
            pipeline
                .board
                .prepare(&self.tiles, &pipeline.tile_layout, device, queue);
        let key = [
            self.uniforms.size[0],
            self.uniforms.size[1],
            self.uniforms.radii_and_clip[0],
            self.uniforms.radii_and_clip[1],
            self.uniforms.radii_and_clip[3],
            self.uniforms.mode_effects[2],
        ];
        if !frames_changed && pipeline.glass_key == Some(key) {
            return;
        }
        pipeline.glass_key = Some(key);
        let width = (self.uniforms.size[0] / 4.).ceil() as u32;
        let height = (self.uniforms.size[1] / 4.).ceil() as u32;
        let glass = pipeline.glass_views[0].texture();
        if (glass.width(), glass.height()) != (width, height) {
            (pipeline.glass_views, pipeline.glass_groups) =
                Pipeline::glass_buffers(device, &pipeline.glass_layout, width, height);
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("workspace glass"),
        });
        for (i, shader) in [&pipeline.backdrop, &pipeline.horizontal, &pipeline.vertical]
            .into_iter()
            .enumerate()
        {
            let dest = i % 2;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass backdrop"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &pipeline.glass_views[dest],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(shader);
            pass.set_bind_group(0, &pipeline.group, &[]);
            if i > 0 {
                pass.set_bind_group(1, &pipeline.glass_groups[1 - dest], &[]);
            }
            pass.draw(0..3, 0..1);
            if i == 0 {
                pipeline.board.draw(&mut pass, &pipeline.small_devices);
            }
        }
        queue.submit([encoder.finish()]);
    }
    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pass.set_pipeline(&pipeline.paper);
        pass.set_bind_group(0, &pipeline.group, &[]);
        pass.draw(0..3, 0..1);
        pipeline.board.draw(pass, &pipeline.devices);
        pass.set_pipeline(&pipeline.render);
        pass.set_bind_group(0, &pipeline.group, &[]);
        pass.set_bind_group(1, &pipeline.glass_groups[0], &[]);
        pass.draw(0..3, 0..1);
        true
    }
}
