// FIXME: Keep the local wire types aligned with upstream Niri IPC and the
// custom fork.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use std::{env, fs, process, thread};

use anyhow::{Context, Result};
use image::RgbaImage;
use log::{info, warn};

use self::ipc::{Action, Request, Response, Socket, Transform};
use super::SceneReader;
use crate::{
    DesktopFrame, Output, OutputId, OutputTransform, Rect, Scene, Size, Window, WindowCapture,
};

mod fallback;
mod ipc;

static SCREENSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const SCREENSHOT_TIMEOUT: Duration = Duration::from_secs(3);
const SCREENSHOT_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// [niri](https://github.com/niri-wm/niri) window discovery
pub(super) struct Niri {
    socket: Socket,
    fallback: Option<fallback::Snapshot>,
}

impl Niri {
    pub(super) fn connect() -> Result<Self> {
        let socket = Socket::connect().context("failed to connect to the Niri IPC socket")?;

        Ok(Self {
            socket,
            fallback: None,
        })
    }

    fn request_raw(&mut self, request: &Request) -> Result<Result<Response, String>> {
        self.socket
            .send(request)
            .context("failed to communicate with Niri")
    }

    fn request(&mut self, request: &Request) -> Result<Response> {
        self.request_raw(request)?.map_err(anyhow::Error::msg)
    }
}

impl SceneReader for Niri {
    fn scene(&mut self) -> Result<Scene> {
        let geometries = if env::var_os("LATCHSHOT_NIRI_FORCE_FALLBACK").is_some() {
            warn!("forcing window geometry reconstruction from standard Niri IPC");

            None
        } else {
            match self.request_raw(&Request::WindowGeometries)? {
                Ok(Response::WindowGeometries(geometries)) => Some(geometries),
                Ok(_) => panic!("Niri returned an unexpected response to WindowGeometries"),
                Err(error) => {
                    warn!(
                        "Niri rejected WindowGeometries ({error}); reconstructing window geometry from standard IPC"
                    );

                    // FIXME: Remove the fallback module and state once upstream Niri exposes
                    // exact on-screen window geometry.
                    None
                }
            }
        };
        let Response::Outputs(outputs) = self.request(&Request::Outputs)? else {
            panic!("Niri returned an unexpected response to Outputs");
        };
        let Response::Windows(ipc_windows) = self.request(&Request::Windows)? else {
            panic!("Niri returned an unexpected response to Windows");
        };

        let mut outputs: Vec<Output> = outputs
            .into_values()
            .filter_map(|output| output.logical.map(|logical| (output, logical)))
            .map(|(output, logical)| {
                // Current-mode metadata is required by the Niri IPC.
                let mode = &output.modes[output.current_mode.unwrap()];
                let (transform, swaps_axes) = match logical.transform {
                    Transform::Normal => (OutputTransform::Normal, false),
                    Transform::_90 => (OutputTransform::Rotate90, true),
                    Transform::_180 => (OutputTransform::Rotate180, false),
                    Transform::_270 => (OutputTransform::Rotate270, true),
                    Transform::Flipped => (OutputTransform::Flipped, false),
                    Transform::Flipped90 => (OutputTransform::Flipped90, true),
                    Transform::Flipped180 => (OutputTransform::Flipped180, false),
                    Transform::Flipped270 => (OutputTransform::Flipped270, true),
                };
                let pixel_size = if swaps_axes {
                    Size::new(mode.height as f64, mode.width as f64)
                } else {
                    Size::new(mode.width as f64, mode.height as f64)
                };

                Output {
                    id: OutputId::new(output.name),
                    logical_geometry: Rect::new(
                        f64::from(logical.x),
                        f64::from(logical.y),
                        pixel_size.width / logical.scale,
                        pixel_size.height / logical.scale,
                    ),
                    pixel_size,
                    scale: logical.scale,
                    transform,
                }
            })
            .collect();
        // Stabilize scene output
        outputs.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

        let windows = if let Some(geometries) = geometries {
            self.fallback = None;
            let output_by_name = outputs
                .iter()
                .map(|output| (output.id.as_str(), output))
                .collect::<HashMap<_, _>>();
            let geometries = geometries
                .into_iter()
                .map(|geometry| (geometry.id, geometry))
                .collect::<HashMap<_, _>>();

            // Preserve stacking order; skip entries lost between IPC snapshots.
            ipc_windows
                .into_iter()
                .filter_map(|window| {
                    let geometry = geometries.get(&window.id)?;
                    let output = output_by_name.get(geometry.output.as_str())?;

                    let geometry = Rect::new(
                        output.logical_geometry.left() + geometry.x,
                        output.logical_geometry.top() + geometry.y,
                        geometry.width,
                        geometry.height,
                    );
                    let geometry = geometry.intersection(output.logical_geometry)?;

                    Some(Window {
                        geometry,
                        identifier: Some(window.id.to_string()),
                    })
                })
                .collect()
        } else {
            self.fallback = Some(fallback::Snapshot::read(&mut self.socket, ipc_windows)?);

            Vec::new()
        };

        Ok(Scene { outputs, windows })
    }

    fn refine_scene(&mut self, scene: &mut Scene, frame: &DesktopFrame) -> Result<()> {
        if let Some(fallback) = &self.fallback {
            scene.windows = fallback::reconstruct(fallback, scene, frame);
        }

        Ok(())
    }
}

pub(super) struct NiriWindowCapture;

impl WindowCapture for NiriWindowCapture {
    fn capture_window(&mut self, window: &Window) -> Result<Option<RgbaImage>> {
        let Some(id) = window
            .identifier
            .as_deref()
            .and_then(|identifier| identifier.parse::<u64>().ok())
        else {
            return Ok(None);
        };
        let runtime_dir = env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
        let sequence = SCREENSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(runtime_dir)
            .join(format!("latchshot-window-{}-{sequence}.png", process::id()));
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("failed to clear stale Niri screenshot"),
        }
        let cleanup = TemporaryScreenshot(path);
        let path = cleanup
            .0
            .to_str()
            .context("Niri screenshot path is not valid UTF-8")?
            .to_owned();
        let mut socket = Socket::connect().context("failed to connect to the Niri IPC socket")?;
        let response = socket
            .send(&Request::Action(Action::ScreenshotWindow {
                id: Some(id),
                write_to_disk: true,
                show_pointer: false,
                path: Some(path),
            }))
            .context("failed to request a Niri window screenshot")?
            .map_err(anyhow::Error::msg)?;
        let Response::Handled = response else {
            panic!("Niri returned an unexpected response to ScreenshotWindow");
        };

        let deadline = Instant::now() + SCREENSHOT_TIMEOUT;
        loop {
            match image::open(&cleanup.0) {
                Ok(image) => {
                    info!("captured window {id} through Niri IPC");

                    return Ok(Some(image.into_rgba8()));
                }
                Err(error) if Instant::now() >= deadline => {
                    return Err(error).context("failed to read Niri window screenshot");
                }
                Err(_) => thread::sleep(SCREENSHOT_POLL_INTERVAL),
            }
        }
    }
}

struct TemporaryScreenshot(PathBuf);

impl Drop for TemporaryScreenshot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
