use std::cell::Cell;
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
    client: Cell<Option<Client>>,
}

impl ImageCopyCapture {
    pub(super) fn connect() -> Result<Self> {
        let mut client = Client::connect().context("failed to connect for image-copy capture")?;
        client
            .refresh()
            .context("failed to enumerate capture sources")?;

        Ok(Self {
            client: Cell::new(Some(client)),
        })
    }
}

impl FrameCapture for ImageCopyCapture {
    fn capture(&self, scene: &Scene) -> Result<DesktopFrame> {
        let mut client = self.client.take().unwrap();

        let result = (|| {
            let outputs = client.outputs().to_vec();
            let frames = scene
                .outputs
                .iter()
                .map(|output| {
                    let wayland_output = outputs
                        .iter()
                        .find(|candidate| candidate.name == output.id.as_str())
                        .with_context(|| {
                            format!("Wayland did not advertise output {}", output.id)
                        })?;
                    let frame = client
                        .capture_output_once(wayland_output, CAPTURE_BUDGET)
                        .with_context(|| format!("failed to capture output {}", output.id))?;
                    let image = match frame {
                        Frame::Shm(captured) => {
                            RgbaImage::from_raw(captured.width, captured.height, captured.rgba)
                                .context("invalid buffer size")?
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

            Ok(DesktopFrame { outputs: frames })
        })();

        self.client.set(Some(client));

        result
    }
}
