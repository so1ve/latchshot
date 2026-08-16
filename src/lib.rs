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
//! use anyhow::{Context, Result};
//! use latchshot::output::Target;
//! use latchshot::overlay::select;
//! use latchshot::{CaptureBackend, Compositor, Selection, SelectionResult};
//!
//! fn main() -> Result<()> {
//!     let compositor = Compositor::detect().context("no supported compositor was detected")?;
//!     let scene = compositor.connect()?.scene()?;
//!
//!     let capture =
//!         CaptureBackend::detect().context("no supported capture protocol was detected")?;
//!     let frame = capture.connect()?.capture(&scene)?;
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
//!     Target::File("screenshot.png".into()).write(&frame.crop(region))?;
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
pub use capture::{CaptureBackend, DesktopFrame, FrameCapture, OutputFrame};
pub use compositor::{Compositor, SceneReader};
pub use geometry::{Point, Rect, Size};
pub use scene::{Output, OutputId, OutputTransform, Scene, Window};
pub use selection::{Selection, SelectionResult, Selector};

#[cfg(test)]
pub(crate) mod test_support;
