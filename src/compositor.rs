//! Compositor discovery and scene snapshots.
//!
//! Use [`Compositor`] for a built-in reader, or implement [`SceneReader`] to
//! supply output and window geometry from another compositor integration.

use std::{env, fmt};

use anyhow::Result;
use clap::ValueEnum;
use log::debug;

use crate::Scene;

mod generic;
mod niri;

/// Reads the output and window geometry needed by the capture and selection
/// stages.
pub trait SceneReader {
    /// Returns a snapshot in global logical coordinates.
    fn scene(&mut self) -> Result<Scene>;
}

/// Compositor backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Compositor {
    Niri,
    Sway,
    Hyprland,
    Mango,
    Generic,
}

impl Compositor {
    /// The environment variable that signals this compositor.
    const fn env_var(self) -> Option<&'static str> {
        match self {
            Self::Niri => Some("NIRI_SOCKET"),
            Self::Sway => Some("SWAYSOCK"),
            Self::Hyprland => Some("HYPRLAND_INSTANCE_SIGNATURE"),
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
    ///
    /// Compositor variants without a dedicated integration currently use the
    /// generic Wayland backend.
    pub fn connect(self) -> Result<Box<dyn SceneReader>> {
        match self {
            Self::Niri => Ok(Box::new(niri::Niri::connect()?)),
            backend @ (Self::Sway | Self::Hyprland | Self::Mango | Self::Generic) => {
                debug!("using generic Wayland scene discovery for {backend}");

                Ok(Box::new(generic::Generic::connect()?))
            }
        }
    }
}

impl fmt::Display for Compositor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.to_possible_value().unwrap().get_name())
    }
}
