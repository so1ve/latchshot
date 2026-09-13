use std::fmt;

use serde::Serialize;

use crate::{Rect, Size};

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
