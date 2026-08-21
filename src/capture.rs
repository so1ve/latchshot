//! Desktop frame capture and logical-coordinate cropping.
//!
//! Use [`WaylandCapture`] for the built-in Wayland protocol implementation, or
//! implement [`FrameCapture`] and [`WindowCapture`] for other sources.

use anyhow::{Context, Result};
use image::{RgbaImage, imageops};
use libwayshot::WayshotConnection;
use log::info;

use crate::{OutputId, Rect, Scene, Window};

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
    #[must_use]
    pub fn scale(&self) -> f64 {
        self.scale_x().max(self.scale_y())
    }

    #[must_use]
    pub fn scale_x(&self) -> f64 {
        f64::from(self.image.width()) / self.logical_geometry.width()
    }

    #[must_use]
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
        // Fast path: If there's only one frame and it exactly matches the region, we
        // can return it directly.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    fn crops_a_single_output_at_native_scale() {
        let desktop = DesktopFrame {
            outputs: vec![solid_frame(
                "eDP-1",
                Rect::new(0.0, 0.0, 100.0, 80.0),
                2.0,
                Rgba([10, 20, 30, 255]),
            )],
        };

        let image = desktop.crop(Rect::new(10.0, 20.0, 30.0, 40.0));
        assert_eq!(image.dimensions(), (60, 80));
        assert_eq!(*image.get_pixel(0, 0), Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn mixed_dpi_crop_uses_the_highest_scale() {
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

        let image = desktop.crop(Rect::new(0.0, 1.0, 4.0, 1.0));

        assert!(image.pixels().all(|pixel| *pixel == Rgba([0, 0, 255, 255])));
    }

    #[test]
    fn fractional_origin_crop_stays_on_the_native_pixel_grid() {
        let desktop = DesktopFrame {
            outputs: vec![OutputFrame {
                output: OutputId::new("fractional"),
                logical_geometry: Rect::new(0.0, 0.0, 8.0, 8.0),
                image: RgbaImage::from_fn(10, 10, |x, _| Rgba([x as u8, 0, 0, 255])),
            }],
        };

        let image = desktop.crop(Rect::new(0.4, 0.4, 4.0, 4.0));

        assert_eq!(image.dimensions(), (6, 6));
        assert_eq!(*image.get_pixel(0, 0), Rgba([0, 0, 0, 255]));
        assert_eq!(*image.get_pixel(5, 0), Rgba([5, 0, 0, 255]));
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
