use crate::{
    Message,
    connection::{DeviceInput, Frame, Press},
    layout::Layout,
    style,
};
use ai_phone_api::wire;
use iced::{Event, Point, Rectangle, Size, keyboard, mouse, touch, wgpu, widget::shader};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Scene {
    pub layout: Layout,
    pub size: Size,
    pub frames: [Option<Arc<Frame>>; 2],
    pub focus: usize,
    pub linked: bool,
    pub popup: Rectangle,
    pub popup_opacity: f32,
    pub child_popup: Rectangle,
    pub child_popup_opacity: f32,
    pub mode_effects: [f32; 4],
    pub mode_motion: [f32; 4],
    pub keyboard_target: Option<usize>,
    pub agent_pointers: [[[f32; 4]; 5]; 2],
}

const POINTER_MS: u64 = 650;

pub struct AgentTrace {
    points: [[f32; 4]; 5],
    received: Instant,
}

impl AgentTrace {
    pub fn from_cursor(
        cursor: &wire::DeviceCursor,
        device: &wire::RegisteredDevice,
    ) -> Option<Self> {
        let agent = if device.kind == "phone" {
            cursor.actor == "agent"
        } else {
            uuid::Uuid::parse_str(&cursor.actor).is_ok()
        };
        if !agent
            || !cursor.input_confirmed
            || cursor.device_id != device.id
            || cursor.action_id.is_empty()
            || cursor.width == 0
            || cursor.height == 0
            || !matches!(cursor.status.as_str(), "running" | "completed")
        {
            return None;
        }
        let mut points = [[0.; 4]; 5];
        for (slot, point) in points
            .iter_mut()
            .zip(cursor.paths.iter().filter_map(|p| p.points.last()))
        {
            let age = cursor
                .reported_at_ms
                .saturating_sub(cursor.started_at_ms.saturating_add(u64::from(point.at_ms)));
            *slot = [
                (point.x as f32 + 0.5) / cursor.width as f32,
                (point.y as f32 + 0.5) / cursor.height as f32,
                POINTER_MS.saturating_sub(age) as f32,
                cursor.width as f32 / cursor.height as f32,
            ];
        }
        Some(Self {
            points,
            received: Instant::now(),
        })
    }

    pub fn markers(&self, frame: &Frame) -> [[f32; 4]; 5] {
        let elapsed = self.received.elapsed().as_secs_f32() * 1000.;
        let aspect = frame.width as f32 / frame.height as f32;
        self.points.map(|[x, y, remaining, captured_aspect]| {
            let opacity = if (aspect - captured_aspect).abs() < 0.05 {
                ((remaining - elapsed) / 160.).clamp(0., 1.)
            } else {
                0.
            };
            [x, y, opacity, 0.]
        })
    }
}
#[derive(Default)]
pub struct Pointer {
    pressed: Option<PointerPress>,
    touch_released: Option<(Point, Instant)>,
    pub anchor: Point,
    pub preedit: Option<iced::advanced::input_method::Preedit>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PointerSource {
    Mouse(mouse::Button),
    Touch(touch::Finger),
}

#[derive(Clone, Copy)]
struct PointerPress {
    device: usize,
    from: Point,
    source: PointerSource,
    started: Instant,
    moved: Option<Instant>,
    focused: bool,
}

impl Scene {
    fn occludes_device(&self, point: Point) -> bool {
        point.y < crate::layout::STAGE_TOP
            || self.layout.chat.contains(point)
            || self.popup.contains(point)
            || self.child_popup.contains(point)
    }

    fn begin_press(
        &self,
        state: &mut Pointer,
        position: Point,
        source: PointerSource,
    ) -> Option<shader::Action<Message>> {
        let (device, from) = self.layout.device_at(self.focus, true, position)?;
        state.anchor = position;
        state.pressed = Some(PointerPress {
            device,
            from,
            source,
            started: Instant::now(),
            moved: None,
            focused: device == self.focus || self.linked,
        });
        Some(shader::Action::publish(Message::Focus(device)).and_capture())
    }

    fn release_press(
        &self,
        state: &mut Pointer,
        position: Option<Point>,
        source: PointerSource,
    ) -> Option<shader::Action<Message>> {
        let press = state.pressed.filter(|press| press.source == source)?;
        state.pressed = None;
        let position = position?;
        let now = Instant::now();
        if matches!(source, PointerSource::Touch(_)) {
            state.touch_released = Some((position, now));
        }
        if !press.focused || self.occludes_device(position) {
            return Some(shader::Action::capture());
        }
        let to = self.layout.local(press.device, position)?;
        let frame = self.frames[press.device].as_ref()?;
        let pixel = |point: Point| {
            (
                (point.x * frame.width as f32).min((frame.width - 1) as f32) as u32,
                (point.y * frame.height as f32).min((frame.height - 1) as f32) as u32,
            )
        };
        let (x, y) = pixel(press.from);
        let input = if press.from.distance(to) > 0.015 {
            let (to_x, to_y) = pixel(to);
            let (began, hold, longest) = match source {
                PointerSource::Mouse(_) => (press.started, true, 2000),
                PointerSource::Touch(_) => {
                    let moved = press.moved.unwrap_or(press.started);
                    (
                        moved,
                        moved.duration_since(press.started) >= Duration::from_millis(400),
                        650,
                    )
                }
            };
            DeviceInput::Drag {
                x,
                y,
                to_x,
                to_y,
                duration: now.duration_since(began).as_millis().clamp(120, longest) as u32,
                hold,
            }
        } else {
            let gesture = match source {
                PointerSource::Mouse(mouse::Button::Right) => Press::Right,
                _ if press.device == 0
                    && now.duration_since(press.started) >= Duration::from_millis(500) =>
                {
                    Press::Hold
                }
                _ => Press::Left,
            };
            DeviceInput::Tap {
                x,
                y,
                press: gesture,
            }
        };
        Some(shader::Action::publish(Message::Input(press.device, input)).and_capture())
    }
}

impl shader::Program<Message> for Scene {
    type State = Pointer;
    type Primitive = Primitive;
    fn update(
        &self,
        state: &mut Pointer,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        let origin = iced::Vector::new(bounds.x, bounds.y);
        let cursor = cursor - origin;
        let touch_active = state
            .pressed
            .is_some_and(|press| matches!(press.source, PointerSource::Touch(_)));
        if let Event::Mouse(mouse) = event {
            if touch_active
                || (matches!(
                    mouse,
                    mouse::Event::ButtonPressed(_) | mouse::Event::ButtonReleased(_)
                ) && state
                    .touch_released
                    .is_some_and(|(touch_position, released)| {
                        released.elapsed() < Duration::from_millis(120)
                            && cursor
                                .position()
                                .is_some_and(|p| p.distance(touch_position) < 8.)
                    }))
            {
                return Some(shader::Action::capture());
            }
            if cursor.position().is_some_and(|p| self.occludes_device(p)) {
                state.pressed = None;
                return None;
            }
        }
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(
                button @ (mouse::Button::Left | mouse::Button::Right),
            )) => {
                return self.begin_press(state, cursor.position()?, PointerSource::Mouse(*button));
            }
            Event::Mouse(mouse::Event::ButtonReleased(button)) => {
                return self.release_press(state, cursor.position(), PointerSource::Mouse(*button));
            }
            Event::Touch(touch::Event::FingerPressed { id, position }) => {
                if touch_active {
                    return Some(shader::Action::capture());
                }
                let position = *position - origin;
                if self.occludes_device(position) {
                    return None;
                }
                return self.begin_press(state, position, PointerSource::Touch(*id));
            }
            Event::Touch(touch::Event::FingerMoved { id, position }) => {
                let press = state
                    .pressed
                    .as_mut()
                    .filter(|p| p.source == PointerSource::Touch(*id))?;
                if press.moved.is_none()
                    && self
                        .layout
                        .local(press.device, *position - origin)
                        .is_some_and(|point| press.from.distance(point) > 0.015)
                {
                    press.moved = Some(Instant::now());
                }
                return Some(shader::Action::capture());
            }
            Event::Touch(touch::Event::FingerLifted { id, position }) => {
                return self.release_press(
                    state,
                    Some(*position - origin),
                    PointerSource::Touch(*id),
                );
            }
            Event::Touch(touch::Event::FingerLost { id, position }) => {
                state
                    .pressed
                    .filter(|press| press.source == PointerSource::Touch(*id))?;
                state.pressed = None;
                state.touch_released = Some((*position - origin, Instant::now()));
                return Some(shader::Action::capture());
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let (device, p) =
                    self.layout
                        .device_at(self.focus, self.linked, cursor.position()?)?;
                let frame = self.frames[device].as_ref()?;
                let amount = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 96.,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                return Some(
                    shader::Action::publish(Message::Input(
                        device,
                        DeviceInput::Scroll {
                            x: (p.x * frame.width as f32) as u32,
                            y: (p.y * frame.height as f32) as u32,
                            amount: amount as i32,
                        },
                    ))
                    .and_capture(),
                );
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                text,
                modifiers,
                ..
            }) => {
                use keyboard::key::Named;
                let target = self.keyboard_target?;
                let control = modifiers.command() || modifiers.control();
                if target == 0 {
                    let phone_key = match key.as_ref() {
                        keyboard::Key::Character(value)
                            if control && value.eq_ignore_ascii_case("a") =>
                        {
                            Some("SELECT_ALL")
                        }
                        keyboard::Key::Named(name) if !control && !modifiers.alt() => match name {
                            Named::Enter => Some("ENTER"),
                            Named::Backspace => Some("DEL"),
                            Named::Delete => Some("FORWARD_DEL"),
                            Named::Escape => Some("BACK"),
                            Named::Tab => Some("TAB"),
                            Named::ArrowLeft => Some("DPAD_LEFT"),
                            Named::ArrowRight => Some("DPAD_RIGHT"),
                            Named::ArrowUp => Some("DPAD_UP"),
                            Named::ArrowDown => Some("DPAD_DOWN"),
                            _ => None,
                        },
                        _ => None,
                    };
                    let input = match phone_key {
                        Some(key) => DeviceInput::Key(key.into()),
                        None if !control && !modifiers.alt() => {
                            DeviceInput::Text(text.as_ref()?.to_string())
                        }
                        _ => return None,
                    };
                    return Some(
                        shader::Action::publish(Message::Input(target, input)).and_capture(),
                    );
                }
                if target == 1 {
                    let named = match key.as_ref() {
                        keyboard::Key::Named(name) => match name {
                            Named::Enter => Some("Return"),
                            Named::Backspace => Some("BackSpace"),
                            Named::Delete => Some("Delete"),
                            Named::Escape => Some("Escape"),
                            Named::Tab => Some("Tab"),
                            Named::ArrowLeft => Some("Left"),
                            Named::ArrowRight => Some("Right"),
                            Named::ArrowUp => Some("Up"),
                            Named::ArrowDown => Some("Down"),
                            Named::Home => Some("Home"),
                            Named::End => Some("End"),
                            Named::PageUp => Some("Prior"),
                            Named::PageDown => Some("Next"),
                            _ => None,
                        },
                        _ => None,
                    };
                    if named.is_some() || control || modifiers.alt() {
                        let value = match (named, key.as_ref()) {
                            (Some(name), _) => name,
                            (_, keyboard::Key::Character(value)) => value,
                            _ => return None,
                        };
                        let mut chord = String::new();
                        if control {
                            chord.push_str("ctrl+");
                        }
                        if modifiers.alt() {
                            chord.push_str("alt+");
                        }
                        if modifiers.shift() {
                            chord.push_str("shift+");
                        }
                        chord.push_str(value);
                        return Some(
                            shader::Action::publish(Message::Input(
                                target,
                                DeviceInput::Key(chord),
                            ))
                            .and_capture(),
                        );
                    }
                }
                let input = DeviceInput::Text(text.as_ref()?.to_string());
                return Some(shader::Action::publish(Message::Input(target, input)).and_capture());
            }
            _ => {}
        }
        None
    }
    fn draw(&self, _: &Pointer, _: mouse::Cursor, _: Rectangle) -> Primitive {
        let rect = |r: Rectangle| [r.x, r.y, r.width, r.height];
        Primitive {
            uniforms: Uniforms {
                size: [
                    self.size.width,
                    self.size.height,
                    self.layout.cable,
                    self.popup_opacity,
                ],
                phone: rect(self.layout.devices[0]),
                desktop: rect(self.layout.devices[1]),
                chat: rect(self.layout.chat),
                popup: rect(self.popup),
                child_popup: rect(self.child_popup),
                state: [
                    self.layout.angles[0],
                    self.layout.angles[1],
                    self.focus as f32,
                    if self.agent_pointers.iter().flatten().any(|p| p[2] > 0.) {
                        1.
                    } else {
                        0.
                    },
                ],
                radii_and_clip: [
                    self.layout.device_radius[0],
                    self.layout.device_radius[1],
                    crate::layout::STAGE_TOP,
                    self.layout.cable_end,
                ],
                material: [
                    style::PANEL_RADIUS,
                    style::GLASS_OPACITY,
                    style::GLASS_BLUR,
                    style::BORDER_WIDTH,
                ],
                background: {
                    let mut color = style::BACKGROUND.into_linear();
                    color[3] = self.child_popup_opacity;
                    color
                },
                mode_effects: self.mode_effects,
                mode_motion: self.mode_motion,
                agent_pointers: self.agent_pointers,
            },
            frames: self.frames.clone(),
        }
    }
    fn mouse_interaction(
        &self,
        _: &Pointer,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.position_from(bounds.position()).is_some_and(|p| {
            !self.occludes_device(p) && (0..2).any(|i| self.layout.local(i, p).is_some())
        }) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct Uniforms {
    size: [f32; 4],
    phone: [f32; 4],
    desktop: [f32; 4],
    chat: [f32; 4],
    popup: [f32; 4],
    child_popup: [f32; 4],
    state: [f32; 4],
    radii_and_clip: [f32; 4],
    material: [f32; 4],
    background: [f32; 4],
    mode_effects: [f32; 4],
    mode_motion: [f32; 4],
    agent_pointers: [[[f32; 4]; 5]; 2],
}

#[derive(PartialEq)]
struct BackdropKey {
    size_and_cable: [f32; 3],
    devices: [[f32; 4]; 2],
    state: [f32; 4],
    radii_and_clip: [f32; 4],
    background: [f32; 3],
    blur: f32,
    goal: f32,
    pointers: [[[f32; 3]; 5]; 2],
    sequences: [u64; 2],
}

impl BackdropKey {
    fn new(scene: &Uniforms, sequences: [u64; 2]) -> Self {
        let [width, height, cable, _] = scene.size;
        let [red, green, blue, _] = scene.background;
        Self {
            size_and_cable: [width, height, cable],
            devices: [scene.phone, scene.desktop],
            state: scene.state,
            radii_and_clip: scene.radii_and_clip,
            background: [red, green, blue],
            blur: scene.material[2],
            goal: scene.mode_effects[2],
            pointers: scene.agent_pointers.map(|device| {
                device.map(|[x, y, opacity, _]| {
                    if opacity > 0. {
                        [x, y, opacity]
                    } else {
                        [0.; 3]
                    }
                })
            }),
            sequences,
        }
    }
}

#[derive(Debug)]
pub struct Primitive {
    uniforms: Uniforms,
    frames: [Option<Arc<Frame>>; 2],
}

pub struct Pipeline {
    render: wgpu::RenderPipeline,
    backdrop: wgpu::RenderPipeline,
    horizontal: wgpu::RenderPipeline,
    vertical: wgpu::RenderPipeline,
    glass_layout: wgpu::BindGroupLayout,
    glass_views: [wgpu::TextureView; 2],
    glass_groups: [wgpu::BindGroup; 2],
    glass_key: Option<BackdropKey>,
    layout: wgpu::BindGroupLayout,
    group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    textures: [wgpu::Texture; 2],
    sequences: [u64; 2],
    sampler: wgpu::Sampler,
}
fn texture(device: &wgpu::Device, w: u32, h: u32, render: bool) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("device-frame"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | if render {
                wgpu::TextureUsages::RENDER_ATTACHMENT
            } else {
                wgpu::TextureUsages::COPY_DST
            },
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
            texture(device, width, height, true).create_view(&Default::default())
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
    fn group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniform: &wgpu::Buffer,
        textures: &[wgpu::Texture; 2],
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("native-scene"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &textures[0].create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &textures[1].create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}
impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
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
            visibility: wgpu::ShaderStages::FRAGMENT,
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
                    1,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                binding(
                    2,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::REPLACE),
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
        let textures = [texture(device, 1, 1, false), texture(device, 1, 1, false)];
        for t in &textures {
            queue.write_texture(
                t.as_image_copy(),
                &[240, 243, 246, 255],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                t.size(),
            );
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let group = Self::group(device, &layout, &uniform, &textures, &sampler);
        Self {
            render,
            backdrop,
            horizontal,
            vertical,
            glass_layout,
            glass_views,
            glass_groups,
            glass_key: None,
            layout,
            group,
            uniform,
            textures,
            sequences: [0, 0],
            sampler,
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
        let mut resized = false;
        for (index, frame) in self.frames.iter().enumerate() {
            if let Some(frame) = frame
                && pipeline.sequences[index] != frame.sequence
            {
                if pipeline.textures[index].width() != frame.width
                    || pipeline.textures[index].height() != frame.height
                {
                    pipeline.textures[index] = texture(device, frame.width, frame.height, false);
                    resized = true;
                }
                let t = &pipeline.textures[index];
                queue.write_texture(
                    t.as_image_copy(),
                    &frame.pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(frame.width * 4),
                        rows_per_image: Some(frame.height),
                    },
                    t.size(),
                );
                pipeline.sequences[index] = frame.sequence;
                if frame.sequence == 1 || frame.sequence % 120 == 0 {
                    eprintln!(
                        "native_metric frame device={index} sequence={} receive_to_upload_ms={:0.3} frame_age_ms={:0.3}",
                        frame.sequence,
                        frame.received.elapsed().as_secs_f64() * 1000.,
                        if frame.captured_ms > 0. {
                            crate::connection::epoch_ms() - frame.captured_ms
                        } else {
                            0.
                        }
                    );
                }
            }
        }
        if resized {
            pipeline.group = Pipeline::group(
                device,
                &pipeline.layout,
                &pipeline.uniform,
                &pipeline.textures,
                &pipeline.sampler,
            );
        }
        let backdrop = BackdropKey::new(&self.uniforms, pipeline.sequences);
        if pipeline.glass_key.as_ref() == Some(&backdrop) {
            return;
        }
        pipeline.glass_key = Some(backdrop);
        let width = (self.uniforms.size[0] / 4.).ceil() as u32;
        let height = (self.uniforms.size[1] / 4.).ceil() as u32;
        let glass = pipeline.glass_views[0].texture();
        if glass.width() != width || glass.height() != height {
            (pipeline.glass_views, pipeline.glass_groups) =
                Pipeline::glass_buffers(device, &pipeline.glass_layout, width, height);
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("glass-backdrop"),
        });
        for (index, render) in [&pipeline.backdrop, &pipeline.horizontal, &pipeline.vertical]
            .into_iter()
            .enumerate()
        {
            let destination = index % 2;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass-blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &pipeline.glass_views[destination],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(render);
            pass.set_bind_group(0, &pipeline.group, &[]);
            if index > 0 {
                pass.set_bind_group(1, &pipeline.glass_groups[1 - destination], &[]);
            }
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }
    fn draw(&self, pipeline: &Pipeline, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pass.set_pipeline(&pipeline.render);
        pass.set_bind_group(0, &pipeline.group, &[]);
        pass.set_bind_group(1, &pipeline.glass_groups[0], &[]);
        pass.draw(0..3, 0..1);
        true
    }
}
