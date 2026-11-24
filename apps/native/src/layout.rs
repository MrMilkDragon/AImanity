use iced::{Point, Rectangle, Size, Vector};

pub const WORKBENCH: Size = Size::new(1440., 900.);
pub const STAGE_TOP: f32 = 64.;
pub const SIDE_INSET: f32 = 24.;
pub const CHAT_PADDING: [f32; 2] = [14., 18.];

pub fn fit_canvas(window: Size, canvas: Size) -> (f32, Vector) {
    let scale = (window.width / canvas.width).min(window.height / canvas.height);
    let origin = Vector::new(
        ((window.width / scale - canvas.width) * 0.5).max(0.),
        ((window.height / scale - canvas.height) * 0.5).max(0.),
    );
    (scale, origin)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub zoom: f32,
    pub pan: Vector,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            zoom: 1.,
            pan: Vector::ZERO,
        }
    }
}
impl Camera {
    pub fn zoom_at(&mut self, cursor: Point, delta: f32, origin: Point) {
        let next = (self.zoom * (delta * 0.0017).exp()).clamp(0.55, 3.);
        let ratio = next / self.zoom;
        let anchor = cursor - origin;
        self.pan = anchor + (self.pan - anchor) * ratio;
        self.zoom = next;
    }
    pub fn limit(&mut self, board: Rectangle, sheet: Size) {
        let x = (board.width + sheet.width * self.zoom) * 0.5 - 64.;
        let y = (board.height + sheet.height * self.zoom) * 0.5 - 64.;
        self.pan.x = self.pan.x.clamp(-x, x);
        self.pan.y = self.pan.y.clamp(-y, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chat_stays_put_and_all_devices_fit_the_board() {
        let mut camera = Camera::default();
        let empty = Layout::home(WORKBENCH, []);
        for aspects in [
            vec![0.5],
            vec![1.8],
            vec![0.45, 1.6],
            vec![0.5, 0.6, 1.6, 1.9, 0.8],
        ] {
            let views = aspects.iter().enumerate().map(|(i, a)| (i.to_string(), *a));
            let layout = Layout::home(WORKBENCH, views).project(&mut camera);
            assert_eq!(layout.tiles.len(), aspects.len());
            assert_eq!(layout.chat, empty.chat);
            assert!(layout.board.contains(layout.chat.center()));
            assert_eq!(layout.chat.width, layout.chat.x - 2. * SIDE_INSET);
            assert_eq!(
                WORKBENCH.width - layout.chat.x - layout.chat.width,
                SIDE_INSET
            );
            for (i, tile) in layout.tiles.iter().enumerate() {
                let inner = tile.screen.shrink(tile.radius / 3.);
                assert!((inner.width / inner.height - aspects[i]).abs() < 1e-4);
                assert!(layout.board.contains(tile.screen.position()));
                assert!(
                    tile.screen.x + tile.screen.width
                        <= layout.board.x + layout.board.width + 0.001
                );
                assert!(
                    tile.screen.y + tile.screen.height
                        <= layout.board.y + layout.board.height + 0.001
                );
                let (hit, p) = layout.device_at(inner.center()).unwrap();
                assert_eq!(hit, i);
                assert!(p.distance(Point::new(0.5, 0.5)) < 1e-5);
            }
            assert!(layout.device_at(layout.chat.center()).is_none());
        }
    }

    #[test]
    fn zoom_keeps_the_pixel_under_the_pointer_and_respects_both_limits() {
        let mut camera = Camera::default();
        let sheet = |c: &mut Camera| {
            Layout::home(WORKBENCH, [("a".into(), 0.51), ("b".into(), 1.7)]).project(c)
        };
        let base = sheet(&mut camera);
        let at = base.tiles[1].screen.center() + Vector::new(25., 9.);
        let (_, before) = base.device_at(at).unwrap();
        camera.zoom_at(at, 125., base.grid_origin);
        assert!(sheet(&mut camera).device_at(at).unwrap().1.distance(before) < 1e-5);
        camera.zoom_at(at, 10000., base.grid_origin);
        assert_eq!(camera.zoom, 3.);
        camera.zoom_at(at, -10000., base.grid_origin);
        assert_eq!(camera.zoom, 0.55);
        assert_eq!(
            sheet(&mut Camera::default()).tiles[1].screen,
            base.tiles[1].screen
        );
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub id: String,
    pub screen: Rectangle,
    pub header: Rectangle,
    pub radius: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub board: Rectangle,
    pub chat: Rectangle,
    pub sheet: Size,
    pub tiles: Vec<Tile>,
    pub grid_origin: Point,
    pub grid_spacing: f32,
}
impl Tile {
    pub fn shift(&mut self, delta: Vector) {
        self.screen = self.screen + delta;
        self.header = self.header + delta;
    }
    pub fn hidden(id: String) -> Self {
        Self {
            id,
            screen: Rectangle::default(),
            header: Rectangle::default(),
            radius: 0.,
        }
    }
    pub fn visible(&self) -> bool {
        self.screen.width > 0.
    }
}
impl Layout {
    pub fn home(size: Size, devices: impl IntoIterator<Item = (String, f32)>) -> Self {
        let board = Rectangle::new(
            Point::new(0., STAGE_TOP),
            Size::new(size.width, size.height - STAGE_TOP),
        );
        let chat_width = (size.width - 3. * SIDE_INSET) * 0.5;
        let chat = Rectangle::new(
            Point::new(size.width - SIDE_INSET - chat_width, STAGE_TOP + 6.),
            Size::new(chat_width, board.height - SIDE_INSET - 6.),
        );
        let devices: Vec<_> = devices
            .into_iter()
            .map(|(id, aspect)| {
                let h = 498_f32.min(585. / aspect);
                (id, Size::new(h * aspect, h))
            })
            .collect();
        let space = Size::new(chat.x - 2. * SIDE_INSET, board.height - 76.);
        let anchor = Point::new(SIDE_INSET + space.width * 0.5, board.center_y() - 5.);
        let mut best = 0.;
        let mut slots = Vec::new();
        let mut sheet = Size::ZERO;
        const GAP: f32 = 32.;
        const HEAD: f32 = 40.;
        let mut order = (0..devices.len()).collect::<Vec<_>>();
        order.sort_by_key(|&i| devices[i].1.width >= devices[i].1.height);
        for count in 1..=devices
            .len()
            .min(2 * (devices.len() as f32).sqrt().ceil() as usize)
        {
            let mut columns = vec![Size::ZERO; count];
            let mut positions = vec![(0, 0.); devices.len()];
            for (rank, &i) in order.iter().enumerate() {
                let s = devices[i].1;
                let column = rank * count / devices.len();
                positions[i] = (column, columns[column].height);
                columns[column].width = columns[column].width.max(s.width);
                columns[column].height += s.height + HEAD + GAP;
            }
            let width = columns.iter().map(|s| s.width).sum::<f32>() + GAP * (count - 1) as f32;
            let height = columns.iter().map(|s| s.height).fold(0., f32::max) - GAP;
            let scale = (space.width / width).min(space.height / height).min(1.65);
            if scale <= best {
                continue;
            }
            best = scale;
            sheet = Size::new(width * scale, height * scale);
            let mut x = 0.;
            let origins: Vec<_> = columns
                .iter()
                .map(|s| {
                    let pos = Point::new(x, (height - s.height + GAP) * 0.48);
                    x += s.width + GAP;
                    pos
                })
                .collect();
            slots = positions
                .into_iter()
                .zip(&devices)
                .map(|((c, y), (_, s))| {
                    origins[c] + Vector::new((columns[c].width - s.width) * 0.5, y + HEAD)
                })
                .collect();
        }
        let top = anchor - Vector::new(sheet.width * 0.5, sheet.height * 0.5);
        let tiles = devices
            .into_iter()
            .zip(slots)
            .map(|((id, size), p)| {
                let display = Rectangle::new(
                    top + (p - Point::ORIGIN) * best,
                    Size::new(size.width * best, size.height * best),
                );
                let radius = (display.width.min(display.height) * 0.045).min(13. * best);
                let screen = display.expand(radius / 3.);
                let header_height = (HEAD * best - 4.).clamp(24., 36.);
                let header = Rectangle::new(
                    Point::new(screen.x, screen.y - header_height - 4.),
                    Size::new(screen.width, header_height),
                );
                Tile {
                    id,
                    screen,
                    header,
                    radius,
                }
            })
            .collect();
        Self {
            board,
            chat,
            sheet,
            tiles,
            grid_origin: anchor,
            grid_spacing: 27.,
        }
    }
    pub fn project(&self, camera: &mut Camera) -> Self {
        camera.limit(
            Rectangle {
                width: self.chat.x - self.board.x,
                ..self.board
            },
            self.sheet,
        );
        let mut view = self.clone();
        if *camera == Camera::default() {
            return view;
        }
        let origin = self.grid_origin + camera.pan;
        let transform = |r: Rectangle| {
            Rectangle::new(
                origin + (r.position() - self.grid_origin) * camera.zoom,
                Size::new(r.width * camera.zoom, r.height * camera.zoom),
            )
        };
        for t in view.tiles.iter_mut().filter(|t| t.visible()) {
            t.screen = transform(t.screen);
            t.header = transform(t.header);
            t.radius *= camera.zoom;
        }
        view.grid_spacing *= camera.zoom;
        view.grid_origin = origin;
        view
    }

    pub fn blend(&self, target: &Self, t: f32) -> Self {
        let mut next = target.clone();
        let rect = |a: Rectangle, b: Rectangle| {
            Rectangle::new(
                a.position() + (b.position() - a.position()) * t,
                Size::new(
                    a.width + (b.width - a.width) * t,
                    a.height + (b.height - a.height) * t,
                ),
            )
        };
        for (i, tile) in next
            .tiles
            .iter_mut()
            .enumerate()
            .filter(|(_, t)| t.visible())
        {
            if let Some(old) = self
                .tiles
                .get(i)
                .filter(|old| old.id == tile.id)
                .or_else(|| self.tiles.iter().find(|old| old.id == tile.id))
                .filter(|old| old.visible())
            {
                tile.screen = rect(old.screen, tile.screen);
                tile.header = rect(old.header, tile.header);
                tile.radius = old.radius + (tile.radius - old.radius) * t;
            } else {
                let lift = Vector::new(0., 9. * (1. - t));
                tile.screen = tile.screen + lift;
                tile.header = tile.header + lift;
            }
        }
        next.grid_origin = self.grid_origin + (target.grid_origin - self.grid_origin) * t;
        next.grid_spacing = self.grid_spacing + (target.grid_spacing - self.grid_spacing) * t;
        next
    }

    pub fn header_at(&self, p: Point) -> Option<usize> {
        if !self.board.contains(p) || self.chat.contains(p) {
            return None;
        }
        self.tiles
            .iter()
            .rposition(|tile| tile.visible() && tile.header.contains(p))
    }
    pub fn device_at(&self, p: Point) -> Option<(usize, Point)> {
        if !self.board.contains(p) || self.chat.contains(p) {
            return None;
        }
        self.tiles
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, _)| self.local(i, p).map(|at| (i, at)))
    }

    pub fn local(&self, i: usize, p: Point) -> Option<Point> {
        let tile = self.tiles.get(i)?;
        let r = tile.screen;
        let bezel = tile.radius / 3.;
        let dx = ((p.x - r.center_x()).abs() - (r.width * 0.5 - tile.radius)).max(0.);
        let dy = ((p.y - r.center_y()).abs() - (r.height * 0.5 - tile.radius)).max(0.);
        if !tile.visible()
            || !self.board.contains(p)
            || self.chat.contains(p)
            || !r.contains(p)
            || dx.hypot(dy) > tile.radius - bezel
        {
            return None;
        }
        Some(Point::new(
            (p.x - r.x - bezel) / (r.width - 2. * bezel),
            (p.y - r.y - bezel) / (r.height - 2. * bezel),
        ))
    }
}
