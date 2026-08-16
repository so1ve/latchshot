//! Compositor discovery and scene snapshots.
//!
//! Use [`Compositor`] for a built-in reader, or implement [`SceneReader`] to
//! supply output and window geometry from another compositor integration.

use std::{env, fmt};

use anyhow::Result;
use clap::ValueEnum;
use log::debug;

use crate::Scene;

mod niri;

/// Reads the output and window geometry needed by the capture and selection
/// stages.
pub trait SceneReader {
    /// Returns a snapshot in global logical coordinates.
    fn scene(&self) -> Result<Scene>;
}

/// Compositor integrations known to latchshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Compositor {
    Niri,
    Sway,
    Hyprland,
    Mango,
}

impl Compositor {
    /// The environment variable that signals this compositor.
    const fn env_var(self) -> &'static str {
        match self {
            Self::Niri => "NIRI_SOCKET",
            Self::Sway => "SWAYSOCK",
            Self::Hyprland => "HYPRLAND_INSTANCE_SIGNATURE",
            Self::Mango => "MANGO_INSTANCE_SIGNATURE",
        }
    }

    /// Detects the running compositor from environment markers.
    #[must_use]
    pub fn detect() -> Option<Self> {
        for backend in Self::value_variants() {
            if env::var_os(backend.env_var()).is_some() {
                debug!("env {} matched {backend}", backend.env_var());

                return Some(*backend);
            }
        }

        env::var("XDG_CURRENT_DESKTOP").ok().and_then(|desktop| {
            desktop
                .split(':')
                .find_map(|name| Self::from_str(name, true).ok())
        })
    }

    /// Connects to this compositor.
    pub fn connect(self) -> Result<Box<dyn SceneReader>> {
        match self {
            Self::Niri => Ok(Box::new(niri::Niri::connect()?)),
            other => unimplemented!("the `{other}` compositor is not implemented yet"),
        }
    }
}

impl fmt::Display for Compositor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.to_possible_value().unwrap().get_name())
    }
}
