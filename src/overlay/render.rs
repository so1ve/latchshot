use crate::capture::OutputFrame;

pub(super) fn copy_frame(frame: &OutputFrame, canvas: &mut [u8]) {
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
            multiply_channel(blue, alpha),
            multiply_channel(green, alpha),
            multiply_channel(red, alpha),
            alpha,
        ]);
    }
}

pub(super) const fn multiply_channel(channel: u8, factor: u8) -> u8 {
    ((channel as u16 * factor as u16 + 127) / 255) as u8
}
