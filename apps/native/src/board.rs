use crate::connection::Frame;
use iced::wgpu;
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Material {
    pub bounds: [f32; 4],
    pub shape: [f32; 4],
    pub markers: [[f32; 4]; 5],
}

#[derive(Debug)]
pub struct Tile {
    pub id: String,
    pub frame: Option<Arc<Frame>>,
    pub material: Material,
}

struct Image {
    pixels: Weak<Vec<u8>>,
    texture: wgpu::Texture,
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
    material: Material,
    seen: u64,
}

#[derive(Default)]
pub struct Board {
    images: HashMap<String, Image>,
    order: Vec<String>,
    revision: u64,
}
impl Board {
    pub fn prepare(
        &mut self,
        tiles: &[Tile],
        layout: &wgpu::BindGroupLayout,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> bool {
        self.revision = self.revision.wrapping_add(1);
        let mut dirty = !self
            .order
            .iter()
            .map(String::as_str)
            .eq(tiles.iter().map(|t| t.id.as_str()));
        if dirty {
            self.order = tiles.iter().map(|t| t.id.clone()).collect();
        }
        for t in tiles {
            let f = t.frame.as_deref();
            let (width, height) = f.map_or((1, 1), |f| (f.width, f.height));
            let fresh = self
                .images
                .get(&t.id)
                .is_none_or(|i| i.texture.width() != width || i.texture.height() != height);
            if fresh {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("device frame"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("device bounds"),
                    size: std::mem::size_of::<Material>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let view = texture.create_view(&Default::default());
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("device"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: uniform.as_entire_binding(),
                        },
                    ],
                });
                self.images.insert(
                    t.id.clone(),
                    Image {
                        texture,
                        uniform,
                        group,
                        material: t.material,
                        seen: 0,
                        pixels: Weak::new(),
                    },
                );
            }
            let image = self.images.get_mut(&t.id).unwrap();
            image.seen = self.revision;
            if fresh || image.material != t.material {
                queue.write_buffer(&image.uniform, 0, bytemuck::bytes_of(&t.material));
                image.material = t.material;
                dirty = true;
            }
            if let Some(frame) = f
                && !std::ptr::eq(image.pixels.as_ptr(), Arc::as_ptr(&frame.pixels))
            {
                queue.write_texture(
                    image.texture.as_image_copy(),
                    &frame.pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width * 4),
                        rows_per_image: Some(height),
                    },
                    image.texture.size(),
                );
                image.pixels = Arc::downgrade(&frame.pixels);
                dirty = true;
            }
        }
        self.images.retain(|_, i| i.seen == self.revision);
        dirty
    }
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline) {
        pass.set_pipeline(pipeline);
        for id in &self.order {
            pass.set_bind_group(1, &self.images[id].group, &[]);
            pass.draw(0..6, 0..1);
        }
    }
}
