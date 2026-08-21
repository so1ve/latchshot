//! Latchshot is a lightweight yet intelligent window-aware screenshot tool.
//!
//! Latchshot keeps scene discovery, frame capture, interactive selection, and
//! image output as separate steps. Applications can use the complete built-in
//! flow or replace individual steps through [`SceneReader`], [`FrameCapture`],
//! and [`Selector`].
//!
//! # Interactive screenshot
//!
//! Capture the desktop before opening the overlay so the selection is made
//! against a frozen frame. The frame returned by [`overlay::select`] is the
//! same frame to which the selected logical geometry refers.
//!
//! ```no_run
//! use anyhow::Result;
//! use latchshot::output::{Target, write_to_targets};
//! use latchshot::overlay::select;
//! use latchshot::{Compositor, FrameCapture, Selection, SelectionResult, WaylandCapture};
//!
//! fn main() -> Result<()> {
//!     let compositor = Compositor::detect().unwrap_or(Compositor::Generic);
//!     let mut compositor = compositor.connect()?;
//!     let mut scene = compositor.scene()?;
//!
//!     let mut capture = WaylandCapture::connect()?;
//!     let frame = capture.capture(&scene)?;
//!     compositor.refine_scene(&mut scene, &frame)?;
//!
//!     let (result, frame) = select(scene, frame, true)?;
//!     let selection = match result {
//!         SelectionResult::Selected(selection) => selection,
//!         SelectionResult::Cancelled => return Ok(()),
//!     };
//!     let region = match selection {
//!         Selection::Window(region) | Selection::Region(region) => region,
//!     };
//!
//!     write_to_targets(
//!         &frame.crop(region),
//!         &[Target::File("screenshot.png".into())],
//!     )?;
//!     Ok(())
//! }
//! ```
//!
//! For a custom frontend, drive [`Selector`] with pointer events and use
//! [`DesktopFrame::crop`] once it returns a [`SelectionResult`].

pub mod animation;
pub mod capture;
pub mod compositor;
pub mod geometry;
pub mod output;
pub mod overlay;
pub mod scene;
pub mod selection;

pub use animation::AnimatedRect;
pub use capture::{DesktopFrame, FrameCapture, OutputFrame, WaylandCapture};
pub use compositor::{Compositor, SceneReader};
pub use geometry::{Point, Rect, Size};
pub use scene::{Output, OutputId, OutputTransform, Scene, Window};
pub use selection::{Selection, SelectionResult, Selector};

#[cfg(test)]
pub(crate) mod test_support;
