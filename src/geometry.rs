use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    #[must_use]
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
    #[must_use]
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rect {
    origin: Point,
    size: Size,
}

impl Rect {
    #[must_use]
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            origin: Point::new(x, y),
            size: Size::new(width, height),
        }
    }

    #[must_use]
    pub fn from_points(a: Point, b: Point) -> Self {
        let left = a.x.min(b.x);
        let top = a.y.min(b.y);

        Self::new(left, top, (a.x - b.x).abs(), (a.y - b.y).abs())
    }

    #[must_use]
    pub const fn left(self) -> f64 {
        self.origin.x
    }

    #[must_use]
    pub const fn top(self) -> f64 {
        self.origin.y
    }

    #[must_use]
    pub const fn right(self) -> f64 {
        self.left() + self.width()
    }

    #[must_use]
    pub const fn bottom(self) -> f64 {
        self.top() + self.height()
    }

    #[must_use]
    pub const fn width(self) -> f64 {
        self.size.width
    }

    #[must_use]
    pub const fn height(self) -> f64 {
        self.size.height
    }

    #[must_use]
    pub fn contains(self, point: Point) -> bool {
        (self.left() <= point.x && point.x < self.right())
            && (self.top() <= point.y && point.y < self.bottom())
    }

    #[must_use]
    pub fn intersection(self, other: Self) -> Option<Self> {
        let left = self.left().max(other.left());
        let top = self.top().max(other.top());
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());

        (left < right && top < bottom).then(|| Self::new(left, top, right - left, bottom - top))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_from_points_is_normalized() {
        assert_eq!(
            Rect::from_points(Point::new(80.0, 70.0), Point::new(20.0, 10.0)),
            Rect::new(20.0, 10.0, 60.0, 60.0)
        );
    }

    #[test]
    fn rectangle_intersection_excludes_touching_edges() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);

        assert_eq!(
            rect.intersection(Rect::new(50.0, 20.0, 80.0, 40.0)),
            Some(Rect::new(50.0, 20.0, 50.0, 40.0))
        );
        assert_eq!(rect.intersection(Rect::new(100.0, 0.0, 20.0, 20.0)), None);
    }
}
