use std::fmt;

use serde::Serialize;

use crate::{Point, Rect, Size};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct OutputId(String);

impl OutputId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OutputId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputTransform {
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

/// A compositor output in global logical coordinates.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Output {
    pub id: OutputId,
    pub logical_geometry: Rect,
    pub pixel_size: Size,
    pub scale: f64,
    pub transform: OutputTransform,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Window {
    pub geometry: Rect,
    /// Stable compositor identifier used for native toplevel capture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Scene {
    pub outputs: Vec<Output>,
    /// Visible windows ordered from front to back.
    pub windows: Vec<Window>,
}

impl Scene {
    pub(crate) fn window_index_at(&self, point: Point) -> Option<usize> {
        self.windows
            .iter()
            .position(|window| window.geometry.contains(point))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{output, window};

    fn scene() -> Scene {
        Scene {
            outputs: vec![output("DP-1", 2.0)],
            windows: vec![
                window(Rect::new(40.0, 40.0, 100.0, 100.0)),
                window(Rect::new(0.0, 0.0, 200.0, 200.0)),
            ],
        }
    }

    #[test]
    fn picks_the_frontmost_window() {
        let scene = scene();

        assert_eq!(scene.window_index_at(Point::new(50.0, 50.0)), Some(0));
    }

    #[test]
    fn rectangle_excludes_its_bottom_right_edges() {
        let rect = Rect::new(10.0, 20.0, 30.0, 40.0);

        assert!(rect.contains(Point::new(10.0, 20.0)));
        assert!(!rect.contains(Point::new(40.0, 60.0)));
    }
}
