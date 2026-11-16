use super::Message;
use ai_phone_api::wire::KnowledgeDocument;
use iced::{
    Color, Font, Point, Rectangle, Renderer, Theme, Vector, keyboard, mouse, touch,
    widget::canvas::{self, Action, Event, Frame, Geometry, Path, Stroke},
    window,
};
use std::{
    cell::Cell,
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Graph {
    nodes: Vec<Node>,
    edges: Vec<(usize, usize)>,
    radius: f32,
    version: u64,
}
struct Node {
    id: String,
    label: String,
    point: Point,
}
impl Graph {
    pub fn rebuild(&mut self, docs: &[KnowledgeDocument]) {
        let mut ordered: Vec<_> = docs.iter().collect();
        ordered.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        let mut nodes = ordered
            .iter()
            .enumerate()
            .map(|(index, doc)| {
                let angle = index as f32 * 2.3999631;
                let radius = (index as f32).sqrt() * 86.;
                let mut label: String = doc.title.chars().take(20).collect();
                if doc.title.chars().count() > 20 {
                    label.push('…');
                }
                Node {
                    id: doc.id.clone(),
                    label,
                    point: Point::new(radius * angle.cos(), radius * angle.sin()),
                }
            })
            .collect::<Vec<_>>();
        let indices: HashMap<_, _> = ordered
            .iter()
            .enumerate()
            .map(|(i, d)| (d.id.as_str(), i))
            .collect();
        let mut edges = ordered
            .iter()
            .enumerate()
            .flat_map(|(i, doc)| {
                let indices = &indices;
                doc.related_ids
                    .iter()
                    .filter_map(move |id| indices.get(id.as_str()).map(|&j| (i.min(j), i.max(j))))
            })
            .collect::<Vec<_>>();
        edges.sort_unstable();
        edges.dedup();
        let mut forces = vec![Vector::ZERO; nodes.len()];
        for _ in 0..80 {
            forces.fill(Vector::ZERO);
            for i in 0..nodes.len() {
                for j in i + 1..nodes.len() {
                    let difference = nodes[i].point - nodes[j].point;
                    let distance = difference.x.hypot(difference.y).max(1.);
                    if distance < 200. {
                        let force = difference * ((200. - distance) * 0.08 / distance);
                        forces[i] += force;
                        forces[j] -= force;
                    }
                }
            }
            for &(a, b) in &edges {
                let difference = nodes[b].point - nodes[a].point;
                let distance = difference.x.hypot(difference.y).max(1.);
                let force = difference * ((distance - 220.) * 0.009 / distance);
                forces[a] += force;
                forces[b] -= force;
            }
            for (node, force) in nodes.iter_mut().zip(&forces) {
                node.point += Vector::new(force.x.clamp(-8., 8.), force.y.clamp(-8., 8.));
            }
        }
        self.radius = nodes
            .iter()
            .map(|n| n.point.x.abs().max(n.point.y.abs()))
            .fold(150., f32::max);
        self.nodes = nodes;
        self.edges = edges;
        self.version += 1;
    }
}

pub struct Network<'a> {
    pub graph: &'a Graph,
    pub selected: Option<&'a str>,
}
pub struct Camera {
    offset: Vector,
    zoom: f32,
    moves: HashMap<String, Vector>,
    hover: Option<(String, Instant)>,
    press: Option<Press>,
    keyboard: Option<String>,
    base: canvas::Cache,
    painted: Cell<Option<(u64, Option<usize>, Color)>>,
}
struct Press {
    current: Point,
    origin: Point,
    node: Option<String>,
    finger: Option<touch::Finger>,
    dragging: bool,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            offset: Vector::ZERO,
            zoom: 1.,
            moves: HashMap::new(),
            hover: None,
            keyboard: None,
            press: None,
            base: canvas::Cache::new(),
            painted: Cell::new(None),
        }
    }
}
impl Network<'_> {
    fn scale(&self, state: &Camera, bounds: Rectangle) -> f32 {
        (bounds.width / (self.graph.radius * 2. + 150.))
            .min(bounds.height / (self.graph.radius * 2. + 110.))
            .clamp(0.2, 1.1)
            * state.zoom
    }
    fn screen(&self, state: &Camera, bounds: Rectangle, node: &Node) -> Point {
        let displacement = state.moves.get(&node.id).copied().unwrap_or(Vector::ZERO);
        Point::new(bounds.width * 0.5, bounds.height * 0.5)
            + state.offset
            + Vector::new(node.point.x + displacement.x, node.point.y + displacement.y)
                * self.scale(state, bounds)
    }
    fn hit(&self, state: &Camera, bounds: Rectangle, local: Point) -> Option<&Node> {
        self.graph
            .nodes
            .iter()
            .filter_map(|node| {
                let distance = self.screen(state, bounds, node).distance(local);
                (distance <= 22.).then_some((node, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(node, _)| node)
    }
    fn select(&self, state: &Camera, bounds: Rectangle, id: &str) -> Option<Action<Message>> {
        let node = self.graph.nodes.iter().find(|n| n.id == id)?;
        let anchor = self.screen(state, bounds, node) + Vector::new(bounds.x, bounds.y);
        Some(Action::publish(Message::Select(node.id.clone(), Some(anchor))).and_capture())
    }
}
impl canvas::Program<Message> for Network<'_> {
    type State = Camera;
    fn update(
        &self,
        state: &mut Camera,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let pointer = match event {
            Event::Touch(
                touch::Event::FingerPressed { position, .. }
                | touch::Event::FingerMoved { position, .. }
                | touch::Event::FingerLifted { position, .. },
            ) => Some(*position - Vector::new(bounds.x, bounds.y)),
            _ => cursor
                .position()
                .map(|p| p - Vector::new(bounds.x, bounds.y)),
        };
        let under_pointer = pointer
            .filter(|p| p.x >= 0. && p.y >= 0. && p.x <= bounds.width && p.y <= bounds.height);
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                let point = under_pointer?;
                let node = self.hit(state, bounds, point).map(|n| n.id.clone());
                state.keyboard = node
                    .clone()
                    .or_else(|| self.graph.nodes.first().map(|n| n.id.clone()));
                state.hover = None;
                let finger = match event {
                    Event::Touch(touch::Event::FingerPressed { id, .. }) => Some(*id),
                    _ => None,
                };
                state.press = Some(Press {
                    current: point,
                    origin: point,
                    node,
                    finger,
                    dragging: false,
                });
                Some(Action::request_redraw().and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                let scale = self.scale(state, bounds);
                if let Some(press) = &mut state.press {
                    if let Event::Touch(touch::Event::FingerMoved { id, .. }) = event
                        && press.finger != Some(*id)
                    {
                        return None;
                    }
                    let point = pointer?;
                    if press.dragging || press.origin.distance(point) > 4. {
                        let delta = point - press.current;
                        if let Some(node) = &press.node {
                            *state.moves.entry(node.clone()).or_insert(Vector::ZERO) +=
                                delta * (1. / scale);
                        } else {
                            state.offset += delta;
                        }
                        press.current = point;
                        press.dragging = true;
                        state.base.clear();
                    }
                    return Some(Action::request_redraw().and_capture());
                }
                let hovered = under_pointer.and_then(|p| self.hit(state, bounds, p));
                if hovered.map(|n| n.id.as_str()) == state.hover.as_ref().map(|(id, _)| id.as_str())
                {
                    return None;
                }
                if let Some(node) = hovered {
                    let now = Instant::now();
                    state.hover = Some((node.id.clone(), now));
                    return Some(Action::request_redraw_at(now + Duration::from_millis(240)));
                }
                state.hover = None;
                Some(Action::request_redraw())
            }
            Event::Touch(touch::Event::FingerLost { id, .. }) => {
                if state.press.as_ref().and_then(|press| press.finger) == Some(*id) {
                    state.press = None;
                }
                None
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. }) => {
                let press = state.press.take()?;
                if !press.dragging
                    && let Some(id) = press.node
                {
                    return self.select(state, bounds, &id);
                }
                Some(Action::capture())
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if under_pointer.is_some() => {
                let amount = match delta {
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                    mouse::ScrollDelta::Lines { y, .. } => *y * 80.,
                };
                state.zoom = (state.zoom * (amount * 0.001).exp()).clamp(0.45, 3.);
                state.base.clear();
                Some(Action::request_redraw().and_capture())
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                state.hover = None;
                state.press = None;
                Some(Action::request_redraw())
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if under_pointer.is_some() && state.keyboard.is_some() =>
            {
                use keyboard::key::Named;
                match key.as_ref() {
                    keyboard::Key::Named(Named::Home) => {
                        state.offset = Vector::ZERO;
                        state.zoom = 1.;
                    }
                    keyboard::Key::Named(Named::ArrowLeft) => state.offset.x += 30.,
                    keyboard::Key::Named(Named::ArrowRight) => state.offset.x -= 30.,
                    keyboard::Key::Named(Named::ArrowUp) => state.offset.y += 30.,
                    keyboard::Key::Named(Named::ArrowDown) => state.offset.y -= 30.,
                    keyboard::Key::Named(Named::Tab) => {
                        let index = self
                            .graph
                            .nodes
                            .iter()
                            .position(|node| Some(&node.id) == state.keyboard.as_ref())
                            .unwrap_or(0);
                        let count = self.graph.nodes.len();
                        if count != 0 {
                            let step = if modifiers.shift() { count - 1 } else { 1 };
                            state.keyboard =
                                Some(self.graph.nodes[(index + step) % count].id.clone());
                        }
                    }
                    keyboard::Key::Named(Named::Enter) => {
                        return self.select(state, bounds, state.keyboard.as_deref()?);
                    }
                    _ => return None,
                }
                state.base.clear();
                Some(Action::request_redraw().and_capture())
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                let (id, since) = state.hover.as_ref()?;
                let same_node = under_pointer
                    .and_then(|p| self.hit(state, bounds, p))
                    .is_some_and(|n| &n.id == id);
                if same_node
                    && self.selected != Some(id.as_str())
                    && *now >= *since + Duration::from_millis(240)
                {
                    return self.select(state, bounds, id);
                }
                if same_node && state.press.is_none() && *now < *since + Duration::from_millis(950)
                {
                    return Some(Action::request_redraw());
                }
                None
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Camera,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let local = cursor.position_in(bounds);
        let hovered = local.and_then(|p| self.hit(state, bounds, p));
        let active = hovered
            .map(|n| n.id.as_str())
            .or(self.selected)
            .or(state.keyboard.as_deref());
        let focus = self
            .graph
            .nodes
            .iter()
            .position(|n| Some(n.id.as_str()) == active);
        let ink = theme.palette().text;
        let stamp = (self.graph.version, focus, ink);
        if state.painted.replace(Some(stamp)) != Some(stamp) {
            state.base.clear();
        }
        let mut pulse = None;
        let elapsed = state
            .hover
            .as_ref()
            .map(|(_, since)| since.elapsed().as_secs_f32() / 0.95)
            .filter(|&elapsed| elapsed < 1.);
        if let (Some(focus), Some(elapsed)) = (focus, elapsed) {
            let mut layer = Frame::new(renderer, bounds.size());
            for &(a, b) in self
                .graph
                .edges
                .iter()
                .filter(|&&(a, b)| focus == a || focus == b)
            {
                let start = self.screen(state, bounds, &self.graph.nodes[a]);
                let end = self.screen(state, bounds, &self.graph.nodes[b]);
                let (first, second) = controls(start, end);
                let t = if focus == a { elapsed } else { 1. - elapsed };
                let u = 1. - t;
                let x = |select: fn(Point) -> f32| {
                    [start, first, second, end]
                        .into_iter()
                        .zip([u * u * u, 3. * u * u * t, 3. * u * t * t, t * t * t])
                        .map(|(point, weight)| select(point) * weight)
                        .sum()
                };
                layer.fill(
                    &Path::circle(Point::new(x(|p| p.x), x(|p| p.y)), 2.),
                    Color {
                        a: (elapsed * std::f32::consts::PI).sin() * 0.65,
                        ..ink
                    },
                );
            }
            pulse = Some(layer.into_geometry());
        }
        let base = state.base.draw(renderer, bounds.size(), |frame| {
            let points: Vec<_> = self
                .graph
                .nodes
                .iter()
                .map(|n| self.screen(state, bounds, n))
                .collect();
            let mut linked = vec![false; points.len()];
            for &(a, b) in &self.graph.edges {
                let start = points[a];
                let end = points[b];
                let (first, second) = controls(start, end);
                let emphasized = focus == Some(a) || focus == Some(b);
                if emphasized {
                    linked[a] = true;
                    linked[b] = true;
                }
                let path = Path::new(|builder| {
                    builder.move_to(start);
                    builder.bezier_curve_to(first, second, end);
                });
                let alpha = if emphasized {
                    0.45
                } else if focus.is_some() {
                    0.08
                } else {
                    0.23
                };
                frame.stroke(
                    &path,
                    Stroke::default()
                        .with_width(if emphasized { 1.25 } else { 0.8 })
                        .with_color(Color { a: alpha, ..ink }),
                );
            }
            let mut labels = Vec::<Rectangle>::new();
            for (i, (node, point)) in self.graph.nodes.iter().zip(&points).enumerate() {
                if point.x < -22.
                    || point.y < -22.
                    || point.x > bounds.width + 22.
                    || point.y > bounds.height + 22.
                {
                    continue;
                }
                let selected = focus == Some(i);
                let dimmed = focus.is_some() && !selected && !linked[i];
                let radius = if selected { 7. } else { 5. };
                frame.fill(
                    &Path::circle(*point, radius + 4.),
                    Color::from_rgba(0.35, 0.39, 0.43, 0.08),
                );
                frame.fill(
                    &Path::circle(*point, radius),
                    if selected {
                        ink
                    } else {
                        Color::from_rgba(0.57, 0.6, 0.65, if dimmed { 0.44 } else { 1. })
                    },
                );
                let width = (node
                    .label
                    .chars()
                    .map(|c| if c.is_ascii() { 7. } else { 13. })
                    .sum::<f32>()
                    + 6.)
                    .min(bounds.width.max(1.));
                let offsets = [
                    (0., 16.),
                    (0., -35.),
                    (width * 0.5 + 12., -9.),
                    (-width * 0.5 - 12., -9.),
                    (0., 36.),
                    (0., -55.),
                ];
                let candidates = offsets.map(|(x, y)| Rectangle {
                    x: (point.x + x - width * 0.5).clamp(0., (bounds.width - width).max(0.)),
                    y: (point.y + y).clamp(0., (bounds.height - 18.).max(0.)),
                    width,
                    height: 18.,
                });
                let label = candidates
                    .into_iter()
                    .min_by_key(|r| {
                        labels.iter().filter(|past| r.intersects(past)).count()
                            + points.iter().filter(|point| r.contains(**point)).count()
                    })
                    .unwrap_or(candidates[0]);
                labels.push(label);
                frame.fill_text(canvas::Text {
                    content: node.label.clone(),
                    position: Point::new(label.center_x(), label.y),
                    align_x: iced::alignment::Horizontal::Center.into(),
                    color: Color {
                        a: if dimmed { 0.48 } else { 0.75 },
                        ..ink
                    },
                    size: 13.into(),
                    shaping: iced::advanced::text::Shaping::Advanced,
                    font: Font::with_name(if cfg!(target_os = "macos") {
                        "PingFang SC"
                    } else {
                        "Microsoft YaHei"
                    }),
                    ..Default::default()
                });
            }
        });
        std::iter::once(base).chain(pulse).collect()
    }

    fn mouse_interaction(
        &self,
        state: &Camera,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.press.as_ref().is_some_and(|press| press.dragging) {
            mouse::Interaction::Grabbing
        } else if cursor
            .position_in(bounds)
            .is_some_and(|p| self.hit(state, bounds, p).is_some())
        {
            mouse::Interaction::Pointer
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

fn controls(start: Point, end: Point) -> (Point, Point) {
    let delta = end - start;
    let bend = Vector::new(0., (delta.x.hypot(delta.y) * 0.1).min(28.));
    (start + delta * 0.35 - bend, start + delta * 0.65 - bend)
}
