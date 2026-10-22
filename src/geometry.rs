use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn distance_squared(self, other: Self) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;

        dx * dx + dy * dy
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rect {
    origin: Point,
    size: Size,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            origin: Point::new(x, y),
            size: Size::new(width, height),
        }
    }

    pub fn from_points(a: Point, b: Point) -> Self {
        let left = a.x.min(b.x);
        let top = a.y.min(b.y);

        Self::new(left, top, (a.x - b.x).abs(), (a.y - b.y).abs())
    }

    pub fn left(self) -> f64 {
        self.origin.x
    }

    pub fn top(self) -> f64 {
        self.origin.y
    }

    pub fn right(self) -> f64 {
        self.left() + self.width()
    }

    pub fn bottom(self) -> f64 {
        self.top() + self.height()
    }

    pub fn width(self) -> f64 {
        self.size.width
    }

    pub fn height(self) -> f64 {
        self.size.height
    }

    pub fn contains(self, point: Point) -> bool {
        (self.left() <= point.x && point.x < self.right())
            && (self.top() <= point.y && point.y < self.bottom())
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        let left = self.left().max(other.left());
        let top = self.top().max(other.top());
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());

        (left < right && top < bottom).then(|| Self::new(left, top, right - left, bottom - top))
    }
}
