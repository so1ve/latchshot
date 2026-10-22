//! Desktop frame capture and logical-coordinate cropping.
//!
//! Use [`WaylandCapture`] for the built-in Wayland protocol implementation, or
//! implement [`FrameCapture`] and [`WindowCapture`] for other sources.

use anyhow::{Context, Result};
use image::{RgbaImage, imageops};
use libwayshot::WayshotConnection;
use log::{info, warn};

use crate::{OutputId, Rect, Scene, Selection, Window};

/// A source of frozen desktop frames for a compositor [`Scene`].
pub trait FrameCapture {
    /// Captures each output in `scene` and preserves its logical geometry.
    fn capture(&mut self, scene: &Scene) -> Result<DesktopFrame>;
}

/// Captures a selected window through a native compositor protocol.
pub trait WindowCapture {
    /// Returns `None` when this capture path cannot handle the window.
    fn capture_window(&mut self, window: &Window) -> Result<Option<RgbaImage>>;
}

/// Returns the selected image, trying native capture for window selections.
///
/// Window capture sources are tried in order until one returns an image.
/// Errors are logged and the next source is tried. If no source succeeds, the
/// window is cropped from the frozen frame. Pass an empty slice to force this
/// path.
///
/// Region and output selections use the frozen frame without calling any
/// window capture source. Output images are moved without copying their pixels.
///
/// # Panics
///
/// Panics under the same conditions as [`DesktopFrame::into_image`] when using
/// the frozen frame.
pub fn capture_selection(
    frame: DesktopFrame,
    selection: Selection,
    window_captures: &mut [&mut dyn WindowCapture],
) -> RgbaImage {
    if let Selection::Window(window) = &selection {
        for capture in window_captures {
            match capture.capture_window(window) {
                Ok(Some(image)) => return image,
                Ok(None) => {}
                Err(error) => warn!("native window capture failed: {error:#}"),
            }
        }
        info!("cropping selected window from the frozen desktop frame");
    }

    frame.into_image(selection)
}

/// Wayland frame capture.
///
/// [`ext-image-copy-capture-v1`](https://wayland.app/protocols/ext-image-copy-capture-v1)
/// is preferred when available, with
/// [`wlr-screencopy`](https://wayland.app/protocols/wlr-screencopy-unstable-v1)
/// used as a fallback.
pub struct WaylandCapture {
    connection: WayshotConnection,
}

impl WaylandCapture {
    /// Connects to the current Wayland session.
    pub fn connect() -> Result<Self> {
        Ok(Self {
            connection: WayshotConnection::new()?,
        })
    }
}

impl WindowCapture for WaylandCapture {
    /// Captures a compositor toplevel.
    ///
    /// Returns `None` when the required protocol is unavailable or the window
    /// has no matching foreign-toplevel identifier. Callers can then fall back
    /// to cropping the frozen desktop frame.
    fn capture_window(&mut self, window: &Window) -> Result<Option<RgbaImage>> {
        if !self.connection.toplevel_capture_support() {
            return Ok(None);
        }
        let Some(identifier) = window.identifier.as_deref() else {
            return Ok(None);
        };
        let Some(toplevel) = self
            .connection
            .get_all_toplevels()
            .iter()
            .find(|toplevel| toplevel.active && toplevel.identifier == identifier)
        else {
            return Ok(None);
        };

        let mut image = self
            .connection
            .screenshot_toplevel(toplevel, false)
            .with_context(|| format!("failed to capture Wayland toplevel {identifier}"))?
            .into_rgba8();
        unpremultiply_alpha(&mut image);
        info!("captured window {identifier} through ext-image-copy-capture-v1");

        Ok(Some(image))
    }
}

/// Wayland SHM alpha formats contain premultiplied color channels, while PNG
/// stores straight alpha.
fn unpremultiply_alpha(image: &mut RgbaImage) {
    for pixel in image.pixels_mut() {
        let alpha = u16::from(pixel[3]);
        if alpha == 0 {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
        } else if alpha < 255 {
            for channel in &mut pixel.0[..3] {
                *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
}

impl FrameCapture for WaylandCapture {
    fn capture(&mut self, scene: &Scene) -> Result<DesktopFrame> {
        let wayland_outputs = self.connection.get_all_outputs();
        let outputs = scene
            .outputs
            .iter()
            .map(|output| {
                let wayland_output = wayland_outputs
                    .iter()
                    .find(|candidate| candidate.name == output.id.as_str())
                    .with_context(|| format!("Wayland did not advertise output {}", output.id))?;
                let image = self
                    .connection
                    .screenshot_single_output(wayland_output, false)
                    .with_context(|| format!("failed to capture output {}", output.id))?
                    .into_rgba8();

                Ok(OutputFrame {
                    output: output.id.clone(),
                    logical_geometry: output.logical_geometry,
                    image,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(DesktopFrame { outputs })
    }
}

/// Captured pixels for one output and their position in the logical desktop.
#[derive(Debug, Clone)]
pub struct OutputFrame {
    pub output: OutputId,
    pub logical_geometry: Rect,
    pub image: RgbaImage,
}

impl OutputFrame {
    pub fn scale(&self) -> f64 {
        self.scale_x().max(self.scale_y())
    }

    pub fn scale_x(&self) -> f64 {
        f64::from(self.image.width()) / self.logical_geometry.width()
    }

    pub fn scale_y(&self) -> f64 {
        f64::from(self.image.height()) / self.logical_geometry.height()
    }
}

/// A frozen desktop assembled from one frame per output.
#[derive(Debug, Clone)]
pub struct DesktopFrame {
    pub outputs: Vec<OutputFrame>,
}

impl DesktopFrame {
    /// Consumes the desktop frame and returns the selected image.
    ///
    /// Output selections move the captured image without copying its pixels.
    /// Window and region selections are cropped from the frozen desktop.
    /// Use [`capture_selection`] when native window capture is desired.
    ///
    /// # Panics
    ///
    /// Panics if a selected output was not captured, or if a selected window or
    /// region does not intersect any captured output.
    pub fn into_image(self, selection: Selection) -> RgbaImage {
        match selection {
            Selection::Output(output) => {
                self.outputs
                    .into_iter()
                    .find(|frame| frame.output == output.id)
                    .expect("selected output is missing from the captured desktop")
                    .image
            }
            selection => self.crop(selection.geometry()),
        }
    }

    /// Crops a region expressed in global logical coordinates.
    ///
    /// Cross-output regions are rendered at the highest intersecting output's
    /// pixel density. Output gaps and portions outside an output remain
    /// transparent.
    ///
    /// # Panics
    ///
    /// Panics if `region` does not intersect any captured output.
    pub fn crop(&self, region: Rect) -> RgbaImage {
        let frames = self
            .outputs
            .iter()
            .filter_map(|frame| {
                frame
                    .logical_geometry
                    .intersection(region)
                    .map(|intersection| (frame, intersection))
            })
            .collect::<Vec<_>>();
        // Regions contained in one output need no resampling or compositing.
        if let [(frame, intersection)] = frames.as_slice()
            // Public API callers may pass regions extending beyond an output;
            // the slow path keeps their full size with transparent padding.
            && *intersection == region
        {
            let source = PixelRect::from_logical(
                *intersection,
                frame.logical_geometry,
                frame.scale_x(),
                frame.scale_y(),
            );

            return imageops::crop_imm(
                &frame.image,
                source.x,
                source.y,
                source.width,
                source.height,
            )
            .to_image();
        }

        let scale = frames
            .iter()
            .map(|(frame, _)| frame.scale())
            .reduce(f64::max)
            .unwrap();
        let mut result = RgbaImage::new(
            snap_pixel_boundary(region.width() * scale).ceil() as u32,
            snap_pixel_boundary(region.height() * scale).ceil() as u32,
        );

        for (frame, intersection) in frames {
            let source = PixelRect::from_logical(
                intersection,
                frame.logical_geometry,
                frame.scale_x(),
                frame.scale_y(),
            );
            let source = imageops::crop_imm(
                &frame.image,
                source.x,
                source.y,
                source.width,
                source.height,
            )
            .to_image();
            let target = PixelRect::from_logical(intersection, region, scale, scale);
            let source = if source.dimensions() == (target.width, target.height) {
                source
            } else {
                imageops::resize(
                    &source,
                    target.width,
                    target.height,
                    imageops::FilterType::Lanczos3,
                )
            };
            imageops::overlay(
                &mut result,
                &source,
                i64::from(target.x),
                i64::from(target.y),
            );
        }

        result
    }
}

struct PixelRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Removes arithmetic noise at integral physical-pixel boundaries while
/// preserving genuinely fractional edges for outward rounding.
fn snap_pixel_boundary(value: f64) -> f64 {
    let nearest = value.round();
    let tolerance = 8.0 * f64::EPSILON * value.abs().max(1.0);

    if (value - nearest).abs() <= tolerance {
        nearest
    } else {
        value
    }
}

impl PixelRect {
    /// Projects a logical rectangle into pixel coordinates, rounding outward.
    fn from_logical(inner: Rect, outer: Rect, scale_x: f64, scale_y: f64) -> Self {
        let left = snap_pixel_boundary((inner.left() - outer.left()) * scale_x).floor() as u32;
        let top = snap_pixel_boundary((inner.top() - outer.top()) * scale_y).floor() as u32;
        let right = snap_pixel_boundary((inner.right() - outer.left()) * scale_x).ceil() as u32;
        let bottom = snap_pixel_boundary((inner.bottom() - outer.top()) * scale_y).ceil() as u32;

        Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        }
    }
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;
    use crate::{Output, OutputTransform, Size};

    struct WindowCaptureFn<F>(F);

    impl<F: FnMut(&Window) -> Result<Option<RgbaImage>>> WindowCapture for WindowCaptureFn<F> {
        fn capture_window(&mut self, window: &Window) -> Result<Option<RgbaImage>> {
            (self.0)(window)
        }
    }

    fn solid_frame(output: &str, geometry: Rect, scale: f64, color: Rgba<u8>) -> OutputFrame {
        let image = RgbaImage::from_pixel(
            (geometry.width() * scale).ceil() as u32,
            (geometry.height() * scale).ceil() as u32,
            color,
        );

        OutputFrame {
            output: OutputId::new(output),
            logical_geometry: geometry,
            image,
        }
    }

    #[test]
    fn native_capture_uses_the_first_available_source() {
        let expected = RgbaImage::from_pixel(2, 1, Rgba([12, 34, 56, 90]));
        let mut first = WindowCaptureFn(|window: &Window| {
            assert_eq!(window.identifier.as_deref(), Some("window-1"));

            Ok(Some(expected.clone()))
        });
        let mut later = WindowCaptureFn(|_: &Window| {
            panic!("capture sources after a successful source must not be called")
        });

        let image = capture_selection(
            DesktopFrame {
                outputs: Vec::new(),
            },
            Selection::Window(Window {
                geometry: Rect::new(0.0, 0.0, 2.0, 1.0),
                identifier: Some("window-1".into()),
            }),
            &mut [&mut first, &mut later],
        );

        assert_eq!(image, expected);
    }

    #[test]
    fn native_capture_continues_after_unavailable_or_failed_sources() {
        let expected = RgbaImage::from_pixel(2, 1, Rgba([12, 34, 56, 90]));
        for fail in [false, true] {
            let mut first = WindowCaptureFn(|_: &Window| {
                if fail {
                    anyhow::bail!("native capture failed");
                }

                Ok(None)
            });
            let mut later = WindowCaptureFn(|_: &Window| Ok(Some(expected.clone())));

            let image = capture_selection(
                DesktopFrame {
                    outputs: Vec::new(),
                },
                Selection::Window(Window {
                    geometry: Rect::new(0.0, 0.0, 2.0, 1.0),
                    identifier: None,
                }),
                &mut [&mut first, &mut later],
            );

            assert_eq!(image, expected);
        }
    }

    #[test]
    fn unsuccessful_native_sources_fall_back_to_the_frozen_frame() {
        let desktop = DesktopFrame {
            outputs: vec![OutputFrame {
                output: OutputId::new("DP-1"),
                logical_geometry: Rect::new(0.0, 0.0, 4.0, 4.0),
                image: RgbaImage::from_fn(4, 4, |x, y| Rgba([x as u8, y as u8, 0, 255])),
            }],
        };
        let mut unavailable = WindowCaptureFn(|_: &Window| Ok(None));
        let mut failed = WindowCaptureFn(|_: &Window| anyhow::bail!("native capture failed"));

        let image = capture_selection(
            desktop,
            Selection::Window(Window {
                geometry: Rect::new(1.0, 1.0, 2.0, 2.0),
                identifier: None,
            }),
            &mut [&mut unavailable, &mut failed],
        );

        assert_eq!(
            image,
            RgbaImage::from_fn(2, 2, |x, y| Rgba([(x + 1) as u8, (y + 1) as u8, 0, 255]))
        );
    }

    #[test]
    fn output_selection_moves_the_captured_image() {
        let geometry = Rect::new(-4.0, 0.0, 4.0, 3.0);
        let selected = solid_frame("selected", geometry, 2.0, Rgba([0, 0, 255, 255]));
        let pixels = selected.image.as_ptr();
        let desktop = DesktopFrame {
            outputs: vec![
                solid_frame("other", geometry, 1.0, Rgba([255, 0, 0, 255])),
                selected,
            ],
        };
        let selection = Selection::Output(Output {
            id: OutputId::new("selected"),
            logical_geometry: geometry,
            pixel_size: Size::new(8.0, 6.0),
            scale: 2.0,
            transform: OutputTransform::Normal,
        });

        let mut native = WindowCaptureFn(|_: &Window| {
            panic!("output selections must not request native window capture")
        });

        let image = capture_selection(desktop, selection, &mut [&mut native]);

        assert_eq!(image.as_ptr(), pixels);
        assert_eq!(image.dimensions(), (8, 6));
        assert!(image.pixels().all(|pixel| *pixel == Rgba([0, 0, 255, 255])));
    }

    #[test]
    fn mixed_dpi_crop_uses_the_highest_intersecting_scale() {
        let desktop = DesktopFrame {
            outputs: vec![
                solid_frame(
                    "left",
                    Rect::new(0.0, 0.0, 100.0, 100.0),
                    1.0,
                    Rgba([255, 0, 0, 255]),
                ),
                solid_frame(
                    "right",
                    Rect::new(100.0, 0.0, 100.0, 100.0),
                    2.0,
                    Rgba([0, 0, 255, 255]),
                ),
            ],
        };

        let image = desktop.crop(Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(image, desktop.outputs[0].image);

        let image = desktop.crop(Rect::new(50.0, 0.0, 100.0, 100.0));
        assert_eq!(image.dimensions(), (200, 200));
        assert_eq!(*image.get_pixel(25, 100), Rgba([255, 0, 0, 255]));
        assert_eq!(*image.get_pixel(175, 100), Rgba([0, 0, 255, 255]));
    }

    #[test]
    fn output_gaps_stay_transparent() {
        let desktop = DesktopFrame {
            outputs: vec![
                solid_frame(
                    "left",
                    Rect::new(0.0, 0.0, 50.0, 50.0),
                    1.0,
                    Rgba([255, 0, 0, 255]),
                ),
                solid_frame(
                    "right",
                    Rect::new(100.0, 0.0, 50.0, 50.0),
                    1.0,
                    Rgba([0, 0, 255, 255]),
                ),
            ],
        };

        let image = desktop.crop(Rect::new(0.0, 0.0, 150.0, 50.0));
        assert_eq!(*image.get_pixel(75, 25), Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn source_coordinates_use_each_axis_pixel_density() {
        let desktop = DesktopFrame {
            outputs: vec![OutputFrame {
                output: OutputId::new("fractional"),
                logical_geometry: Rect::new(0.0, 0.0, 4.0, 3.0),
                image: RgbaImage::from_fn(8, 9, |_, y| {
                    if y < 3 {
                        Rgba([255, 0, 0, 255])
                    } else {
                        Rgba([0, 0, 255, 255])
                    }
                }),
            }],
        };

        let image = capture_selection(
            desktop,
            Selection::Window(Window {
                geometry: Rect::new(0.0, 1.0, 4.0, 1.0),
                identifier: None,
            }),
            &mut [],
        );

        assert!(image.pixels().all(|pixel| *pixel == Rgba([0, 0, 255, 255])));
    }

    #[test]
    fn crops_on_the_native_pixel_grid() {
        let mut native = WindowCaptureFn(|_: &Window| {
            panic!("region selections must not request native window capture")
        });
        for (pixels, region, dimensions, first, last) in [
            (
                8,
                Rect::new(1.0, 2.0, 3.0, 4.0),
                (3, 4),
                [1, 2, 0, 255],
                [3, 5, 0, 255],
            ),
            (
                16,
                Rect::new(1.0, 2.0, 3.0, 4.0),
                (6, 8),
                [2, 4, 0, 255],
                [7, 11, 0, 255],
            ),
            (
                10,
                Rect::new(0.4, 0.4, 4.0, 4.0),
                (6, 6),
                [0, 0, 0, 255],
                [5, 5, 0, 255],
            ),
        ] {
            let desktop = DesktopFrame {
                outputs: vec![OutputFrame {
                    output: OutputId::new("test"),
                    logical_geometry: Rect::new(0.0, 0.0, 8.0, 8.0),
                    image: RgbaImage::from_fn(pixels, pixels, |x, y| {
                        Rgba([x as u8, y as u8, 0, 255])
                    }),
                }],
            };
            let image = capture_selection(desktop, Selection::Region(region), &mut [&mut native]);

            assert_eq!(image.dimensions(), dimensions);
            assert_eq!(*image.get_pixel(0, 0), Rgba(first));
            assert_eq!(
                *image.get_pixel(dimensions.0 - 1, dimensions.1 - 1),
                Rgba(last)
            );
        }
    }

    #[test]
    fn pixel_aligned_crop_ignores_floating_point_noise() {
        let desktop = DesktopFrame {
            outputs: vec![OutputFrame {
                output: OutputId::new("fractional"),
                logical_geometry: Rect::new(0.0, 0.0, 1.0, 1028.5714285714287),
                image: RgbaImage::from_fn(1, 1800, |_, y| Rgba([y as u8, 0, 0, 255])),
            }],
        };

        let image = desktop.crop(Rect::new(0.0, 46.285714285714285, 1.0, 974.8571428571428));

        assert_eq!(image.dimensions(), (1, 1706));
        assert_eq!(*image.get_pixel(0, 0), Rgba([81, 0, 0, 255]));
    }
}
