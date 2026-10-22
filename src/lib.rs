//! Latchshot is a lightweight yet intelligent window-aware screenshot tool.
//!
//! Latchshot keeps scene discovery, frame capture, interactive selection, and
//! image output as separate steps. Applications can use the complete built-in
//! flow or replace individual steps through [`SceneReader`], [`FrameCapture`],
//! [`WindowCapture`], and [`Selector`].
//!
//! # Cargo features
//!
//! All compositor integrations are enabled by default. Use
//! `--no-default-features` and `--features` to select which ones to include:
//!
//! | Feature | Capability |
//! | --- | --- |
//! | `niri` | Niri IPC scene discovery, including the custom `WindowGeometries` extension |
//! | `niri-fallback` | Heuristic window geometry reconstruction from standard Niri IPC and frozen output pixels; enables `niri` |
//! | `niri-capture` | Native window screenshots through Niri's `ScreenshotWindow` IPC; enables `niri` |
//! | `sway` | Sway IPC window discovery |
//! | `hyprland` | Hyprland IPC window discovery |
//! | `mango` | Mango IPC window discovery |
//!
//! For example, include Niri's exact geometry path and native capture without
//! heuristic reconstruction or other compositor integrations:
//!
//! ```toml
//! latchshot = { version = "*", default-features = false, features = ["niri", "niri-capture"] }
//! ```
//!
//! The generic Wayland backend, standard Wayland capture, and free-form region
//! selection are always available.
//!
//! # Interactive screenshot
//!
//! Capture the desktop before opening the overlay so the selection is made
//! against a frozen frame. The frame returned by [`overlay::select`] is the
//! same frame to which the selected logical geometry refers.
//!
//! A selection identifies a window, a drawn region, or an entire output.
//! [`capture_selection`] tries the supplied native window capture sources in
//! order, then falls back to cropping the frozen desktop. Region selections
//! are cropped, and output selections take the captured image without copying
//! its pixels.
//!
//! ```no_run
//! use anyhow::Result;
//! use latchshot::output::{Target, write_to_targets};
//! use latchshot::overlay::select;
//! use latchshot::{
//!     Compositor, FrameCapture, SelectionResult, WaylandCapture, WindowCapture, capture_selection,
//! };
//!
//! fn main() -> Result<()> {
//!     let compositor = Compositor::detect().unwrap_or(Compositor::Generic);
//!     let mut compositor_capture = compositor.window_capturer();
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
//!     let mut sources: Vec<&mut dyn WindowCapture> = vec![&mut capture];
//!     if let Some(source) = compositor_capture.as_deref_mut() {
//!         sources.push(source);
//!     }
//!     let image = capture_selection(frame, selection, &mut sources);
//!
//!     write_to_targets(&image, &[Target::File("screenshot.png".into())])?;
//!     Ok(())
//! }
//! ```
//!
//! For a custom frontend, drive [`Selector`] with pointer events and use
//! [`capture_selection`] once it returns a [`SelectionResult`]. Pass an empty
//! capture source slice, or call [`DesktopFrame::into_image`] directly, to use
//! only the frozen desktop pixels.

pub mod animation;
pub mod capture;
pub mod compositor;
pub mod geometry;
pub mod output;
pub mod overlay;
pub mod scene;
pub mod selection;

pub use animation::AnimatedRect;
pub use capture::{
    DesktopFrame, FrameCapture, OutputFrame, WaylandCapture, WindowCapture, capture_selection,
};
pub use compositor::{Compositor, SceneReader};
pub use geometry::{Point, Rect, Size};
pub use scene::{Output, OutputId, OutputTransform, Scene, Window};
pub use selection::{Selection, SelectionResult, Selector};
