use crate::capture::OutputFrame;

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

pub(super) const fn multiply_channel(channel: u8, factor: u8) -> u8 {
    ((channel as u16 * factor as u16 + 127) / 255) as u8
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
}
