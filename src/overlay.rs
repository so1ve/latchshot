//! The built-in interactive Wayland selection overlay.
//!
//! Call [`select`] with a scene and its already captured frame. It consumes
//! both while the overlay is open and returns the frame alongside the user's
//! result, keeping the selected logical geometry tied to the correct pixels.

mod highlight;
mod render;
mod session;
mod surfaces;

pub use session::select;
