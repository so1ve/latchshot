use crate::capture::OutputFrame;

pub(super) const BORDER_WIDTH: f64 = 2.0;
pub(super) const CORNER_RADIUS: f64 = 6.0;
pub(super) const DIM_FACTOR: u8 = 140;

const BORDER_BLUE: u8 = 255;
const BORDER_GREEN: u8 = 239;
const BORDER_RED: u8 = 215;
const CORNER_SAMPLES: u32 = 4;

pub(super) fn copy_frame(frame: &OutputFrame, canvas: &mut [u8], map_channel: impl Fn(u8) -> u8) {
    // The shm slot is padded to 64 bytes; only the image extent is copied and
    // shared with the compositor.
    assert!(canvas.len() >= frame.image.as_raw().len());

    for (target, source) in canvas
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(frame.image.pixels())
    {
        let [red, green, blue, alpha] = source.0;
        target.copy_from_slice(&[
            map_channel(multiply_channel(blue, alpha)),
            map_channel(multiply_channel(green, alpha)),
            map_channel(multiply_channel(red, alpha)),
            alpha,
        ]);
    }
}

#[derive(Clone, Copy)]
pub(super) enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

pub(super) struct CornerMask {
    size: u32,
    coverage: Vec<u8>,
}

impl CornerMask {
    #[must_use]
    pub(super) fn new(scale: f64, corner: Corner) -> Self {
        let radius = CORNER_RADIUS * scale;
        let border_width = BORDER_WIDTH * scale;
        let size = radius.ceil() as u32;
        let center = match corner {
            Corner::TopLeft => (radius, radius),
            Corner::TopRight => (0.0, radius),
            Corner::BottomLeft => (radius, 0.0),
            Corner::BottomRight => (0.0, 0.0),
        };
        let mut coverage = Vec::with_capacity(size.pow(2) as usize);

        for y in 0..size {
            for x in 0..size {
                let mut covered = 0;
                for sample_y in 0..CORNER_SAMPLES {
                    for sample_x in 0..CORNER_SAMPLES {
                        let x =
                            f64::from(x) + (f64::from(sample_x) + 0.5) / f64::from(CORNER_SAMPLES);
                        let y =
                            f64::from(y) + (f64::from(sample_y) + 0.5) / f64::from(CORNER_SAMPLES);
                        let distance = (x - center.0).hypot(y - center.1);

                        if distance <= radius && distance >= radius - border_width {
                            covered += 1;
                        }
                    }
                }
                coverage.push(
                    ((covered * 255 + CORNER_SAMPLES.pow(2) / 2) / CORNER_SAMPLES.pow(2)) as u8,
                );
            }
        }

        Self { size, coverage }
    }

    #[must_use]
    pub(super) const fn size(&self) -> u32 {
        self.size
    }

    pub(super) fn render(&self, opacity: u8, canvas: &mut [u8]) {
        debug_assert_eq!(canvas.len(), self.coverage.len() * 4);

        for (pixel, coverage) in canvas.as_chunks_mut::<4>().0.iter_mut().zip(&self.coverage) {
            let alpha = ((*coverage as u16 * opacity as u16 + 127) / 255) as u8;
            pixel.copy_from_slice(&border_pixel_with_alpha(alpha));
        }
    }
}

pub(super) const fn multiply_channel(channel: u8, factor: u8) -> u8 {
    ((channel as u16 * factor as u16 + 127) / 255) as u8
}

pub(super) const fn border_pixel_with_alpha(alpha: u8) -> [u8; 4] {
    [
        multiply_channel(BORDER_BLUE, alpha),
        multiply_channel(BORDER_GREEN, alpha),
        multiply_channel(BORDER_RED, alpha),
        alpha,
    ]
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::*;
    use crate::{OutputId, Rect};

    fn frame(color: Rgba<u8>) -> OutputFrame {
        OutputFrame {
            output: OutputId::new("test"),
            logical_geometry: Rect::new(0.0, 0.0, 2.0, 2.0),
            image: RgbaImage::from_pixel(2, 2, color),
        }
    }

    #[test]
    fn backgrounds_use_premultiplied_bgra() {
        let frame = frame(Rgba([100, 50, 200, 128]));
        let mut original = vec![0; 16];
        let mut dimmed = vec![0; 16];

        copy_frame(&frame, &mut original, std::convert::identity);
        copy_frame(&frame, &mut dimmed, |channel| channel / 2);

        assert_eq!(&original[..4], &[100, 25, 50, 128]);
        assert_eq!(&dimmed[..4], &[50, 12, 25, 128]);
    }

    #[test]
    fn border_pixels_are_premultiplied() {
        assert_eq!(border_pixel_with_alpha(0), [0, 0, 0, 0]);
        assert_eq!(border_pixel_with_alpha(255), [255, 239, 215, 255]);
    }

    #[test]
    fn corner_mask_is_transparent_away_from_the_arc() {
        let mask = CornerMask::new(1.0, Corner::TopLeft);
        let mut canvas = vec![0; (mask.size().pow(2) * 4) as usize];

        mask.render(255, &mut canvas);

        assert_eq!(&canvas[..4], &[0, 0, 0, 0]);
        assert!(canvas.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0));
    }
}
