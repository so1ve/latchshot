use std::time::Duration;

use anyhow::{Context, Result, bail};
use image::RgbaImage;
use wlr_capture::wl::{Client, Frame};

use super::{DesktopFrame, FrameCapture, OutputFrame};
use crate::Scene;

const CAPTURE_BUDGET: Duration = Duration::from_secs(2);

/// Output capture via [ext-image-copy-capture-v1](https://wayland.app/protocols/ext-image-copy-capture-v1)
/// (newer compositors).
pub(super) struct ImageCopyCapture {
    client: Client,
}

impl ImageCopyCapture {
    pub(super) fn connect() -> Result<Self> {
        let mut client = Client::connect()?;
        client.refresh()?;

        Ok(Self { client })
    }
}

impl FrameCapture for ImageCopyCapture {
    fn capture(&mut self, scene: &Scene) -> Result<DesktopFrame> {
        let outputs = self.client.outputs().to_vec();
        let outputs = scene
            .outputs
            .iter()
            .map(|output| {
                let wayland_output = outputs
                    .iter()
                    .find(|candidate| candidate.name == output.id.as_str())
                    .with_context(|| format!("Wayland did not advertise output {}", output.id))?;
                let frame = self
                    .client
                    .capture_output_once(wayland_output, CAPTURE_BUDGET)
                    .with_context(|| format!("failed to capture output {}", output.id))?;
                let image = match frame {
                    Frame::Shm(captured) => {
                        RgbaImage::from_raw(captured.width, captured.height, captured.rgba)
                            .expect("capture buffer dimensions must match its byte length")
                    }
                    Frame::Dmabuf(_) => bail!("the compositor returned a dma-buf frame"),
                };

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
