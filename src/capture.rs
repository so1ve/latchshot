//! Desktop frame capture and logical-coordinate cropping.
//!
//! Use [`CaptureBackend`] for the built-in Wayland protocol implementations,
//! or implement [`FrameCapture`] to provide frames from another source.

use std::fmt;

use anyhow::Result;
use clap::ValueEnum;
use image::{RgbaImage, imageops};
use log::{debug, warn};
use wlr_capture::wl::advertised_globals;

use crate::{OutputId, Rect, Scene};

mod image_copy_capture;
mod screencopy;

/// A source of frozen desktop frames for a compositor [`Scene`].
pub trait FrameCapture {
    /// Captures each output in `scene` and preserves its logical geometry.
    fn capture(&mut self, scene: &Scene) -> Result<DesktopFrame>;
}

/// Capture backends, preferred in detection order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CaptureBackend {
    ImageCopyCapture,
    Screencopy,
}

impl fmt::Display for CaptureBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.to_possible_value().unwrap().get_name())
    }
}

impl CaptureBackend {
    /// Detects the best available capture backend from the advertised Wayland
    /// globals, preferring the new image-copy protocol over screencopy.
    #[must_use]
    pub fn detect() -> Option<Self> {
        let Ok(globals) = advertised_globals() else {
            warn!("failed to enumerate Wayland globals");

            return None;
        };
        let has = |interface: &str| globals.iter().any(|(name, _)| name == interface);

        if has("ext_image_copy_capture_manager_v1") {
            debug!("image-copy capture protocol advertised");

            return Some(Self::ImageCopyCapture);
        }
        if has("zwlr_screencopy_manager_v1") {
            debug!("wlr-screencopy protocol advertised");

            return Some(Self::Screencopy);
        }

        None
    }

    /// Connects to this capture backend.
    pub fn connect(self) -> Result<Box<dyn FrameCapture>> {
        match self {
            Self::ImageCopyCapture => {
                Ok(Box::new(image_copy_capture::ImageCopyCapture::connect()?))
            }
            Self::Screencopy => Ok(Box::new(screencopy::Screencopy::connect()?)),
        }
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
            (region.width() * scale).ceil() as u32,
            (region.height() * scale).ceil() as u32,
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

impl PixelRect {
    /// Projects a logical rectangle into pixel coordinates, rounding outward.
    fn from_logical(inner: Rect, outer: Rect, scale_x: f64, scale_y: f64) -> Self {
        let left = ((inner.left() - outer.left()) * scale_x).floor() as u32;
        let top = ((inner.top() - outer.top()) * scale_y).floor() as u32;
        let right = ((inner.right() - outer.left()) * scale_x).ceil() as u32;
        let bottom = ((inner.bottom() - outer.top()) * scale_y).ceil() as u32;

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
}
