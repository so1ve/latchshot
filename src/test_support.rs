use crate::{Output, OutputId, OutputTransform, Rect, Size, Window};

pub fn output(id: &str, scale: f64) -> Output {
    Output {
        id: OutputId::new(id),
        logical_geometry: Rect::new(0.0, 0.0, 1920.0, 1080.0),
        pixel_size: Size::new(1920.0 * scale, 1080.0 * scale),
        scale,
        transform: OutputTransform::Normal,
    }
}

pub const fn window(geometry: Rect) -> Window {
    Window {
        geometry,
        identifier: None,
    }
}
