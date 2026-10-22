//! Compositor discovery and scene snapshots.
//!
//! Use [`Compositor`] for a built-in reader, or implement [`SceneReader`] to
//! supply output and window geometry from another compositor integration.

use std::{env, fmt};

use anyhow::Result;
use clap::ValueEnum;
use log::debug;

use crate::{DesktopFrame, Scene, WindowCapture};

mod generic;
#[cfg(feature = "hyprland")]
mod hyprland;
#[cfg(any(feature = "hyprland", feature = "mango"))]
mod ipc;
#[cfg(feature = "mango")]
mod mango;
#[cfg(feature = "niri")]
mod niri;
#[cfg(feature = "sway")]
mod sway;

/// Reads the output and window geometry needed by the capture and selection
/// stages.
pub trait SceneReader {
    /// Returns a snapshot in global logical coordinates.
    fn scene(&mut self) -> Result<Scene>;

    /// Refines scene geometry against the frozen desktop frame.
    ///
    /// Most compositors already expose exact geometry and keep the default
    /// no-op. Backends whose IPC omits on-screen positions can recover them
    /// here without taking a second, potentially different screenshot.
    fn refine_scene(&mut self, _scene: &mut Scene, _frame: &DesktopFrame) -> Result<()> {
        Ok(())
    }
}

/// Compositor backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Compositor {
    #[cfg(feature = "niri")]
    Niri,
    #[cfg(feature = "sway")]
    Sway,
    #[cfg(feature = "hyprland")]
    Hyprland,
    #[cfg(feature = "mango")]
    Mango,
    Generic,
}

impl Compositor {
    /// The environment variable that signals this compositor.
    const fn env_var(self) -> Option<&'static str> {
        match self {
            #[cfg(feature = "niri")]
            Self::Niri => Some("NIRI_SOCKET"),
            #[cfg(feature = "sway")]
            Self::Sway => Some("SWAYSOCK"),
            #[cfg(feature = "hyprland")]
            Self::Hyprland => Some("HYPRLAND_INSTANCE_SIGNATURE"),
            #[cfg(feature = "mango")]
            Self::Mango => Some("MANGO_INSTANCE_SIGNATURE"),
            Self::Generic => None,
        }
    }

    /// Detects the running compositor from environment markers.
    #[must_use]
    pub fn detect() -> Option<Self> {
        for backend in Self::value_variants() {
            let Some(env_var) = backend.env_var() else {
                continue;
            };
            if env::var_os(env_var).is_some() {
                debug!("env {env_var} matched {backend}");

                return Some(*backend);
            }
        }

        env::var("XDG_CURRENT_DESKTOP").ok().and_then(|desktop| {
            desktop
                .split(':')
                .find_map(|name| Self::from_str(name, true).ok())
                .filter(|backend| *backend != Self::Generic)
        })
    }

    /// Connects to the selected scene backend.
    pub fn connect(self) -> Result<Box<dyn SceneReader>> {
        match self {
            #[cfg(feature = "niri")]
            Self::Niri => niri::connect(),
            #[cfg(feature = "sway")]
            Self::Sway => Ok(Box::new(sway::Sway::connect()?)),
            #[cfg(feature = "hyprland")]
            Self::Hyprland => Ok(Box::new(hyprland::Hyprland::connect()?)),
            #[cfg(feature = "mango")]
            Self::Mango => Ok(Box::new(mango::Mango::connect()?)),
            Self::Generic => {
                debug!("using generic Wayland scene discovery");
                Ok(Box::new(generic::Generic::connect()?))
            }
        }
    }

    /// Creates the compositor-specific window capture path, when one exists.
    #[must_use]
    pub fn window_capturer(self) -> Option<Box<dyn WindowCapture>> {
        #[cfg(feature = "niri-capture")]
        if self == Self::Niri {
            return Some(Box::new(niri::NiriWindowCapture));
        }

        None
    }
}

impl fmt::Display for Compositor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.to_possible_value().unwrap().get_name())
    }
}
