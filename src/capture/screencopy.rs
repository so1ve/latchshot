use anyhow::{Context, Result};
use libwayshot::WayshotConnection;

use super::{DesktopFrame, FrameCapture, OutputFrame};
use crate::Scene;

/// Output capture via [wlr-screencopy](https://wayland.app/protocols/wlr-screencopy-unstable-v1)
/// (wlroots compositors).
pub(super) struct Screencopy {
    connection: WayshotConnection,
}

impl Screencopy {
    pub(super) fn connect() -> Result<Self> {
        Ok(Self {
            connection: WayshotConnection::new()?,
        })
    }
}

impl FrameCapture for Screencopy {
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
