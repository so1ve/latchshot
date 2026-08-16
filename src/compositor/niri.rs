// FIXME: Keep the local wire types aligned with the custom Niri IPC fork.
use std::cell::Cell;
use std::collections::HashMap;

use anyhow::{Context, Result};
use log::warn;

use self::ipc::{Request, Response, Socket, Transform};
use super::SceneReader;
use super::generic::Generic;
use crate::{Output, OutputId, OutputTransform, Rect, Scene, Size, Window};

mod ipc;

/// [niri](https://github.com/niri-wm/niri) window discovery via its
/// [IPC socket](https://github.com/niri-wm/niri/wiki/IPC).
pub(super) struct Niri {
    socket: Cell<Option<Socket>>,
}

impl Niri {
    pub(super) fn connect() -> Result<Self> {
        let socket = Socket::connect().context("failed to connect to the Niri IPC socket")?;

        Ok(Self {
            socket: Cell::new(Some(socket)),
        })
    }

    fn request_raw(&self, request: Request) -> Result<Result<Response, String>> {
        let mut socket = self.socket.take().unwrap();
        let result = socket
            .send(request)
            .context("failed to communicate with Niri");
        self.socket.set(Some(socket));

        result
    }

    fn request(&self, request: Request) -> Result<Response> {
        self.request_raw(request)?.map_err(anyhow::Error::msg)
    }
}

impl SceneReader for Niri {
    fn scene(&self) -> Result<Scene> {
        let geometries = match self.request_raw(Request::WindowGeometries)? {
            Ok(Response::WindowGeometries(geometries)) => geometries,
            Ok(_) => panic!("Niri returned an unexpected response to WindowGeometries"),
            Err(error) if is_unsupported_window_geometries(&error) => {
                warn!("Niri does not support WindowGeometries; using generic Wayland discovery");

                // FIXME: Remove this compatibility fallback as soon as upstream Niri supports
                // the WindowGeometries IPC request.
                return Generic::connect()?.scene();
            }
            Err(error) => return Err(anyhow::Error::msg(error)),
        };
        let Response::Outputs(outputs) = self.request(Request::Outputs)? else {
            panic!("Niri returned an unexpected response to Outputs");
        };
        let Response::Windows(windows) = self.request(Request::Windows)? else {
            panic!("Niri returned an unexpected response to Windows");
        };

        let mut outputs: Vec<Output> = outputs
            .into_values()
            .filter_map(|output| output.logical.map(|logical| (output, logical)))
            .map(|(output, logical)| {
                // Current-mode metadata is required by the Niri IPC.
                let mode_index = output.current_mode.unwrap();
                let mode = output.modes.get(mode_index).unwrap();
                let pixel_size = {
                    let width = mode.width;
                    let height = mode.height;
                    let transform = logical.transform;
                    match transform {
                        Transform::Normal
                        | Transform::_180
                        | Transform::Flipped
                        | Transform::Flipped180 => Size::new(width as f64, height as f64),
                        Transform::_90
                        | Transform::_270
                        | Transform::Flipped90
                        | Transform::Flipped270 => Size::new(height as f64, width as f64),
                    }
                };
                let transform = match logical.transform {
                    Transform::Normal => OutputTransform::Normal,
                    Transform::_90 => OutputTransform::Rotate90,
                    Transform::_180 => OutputTransform::Rotate180,
                    Transform::_270 => OutputTransform::Rotate270,
                    Transform::Flipped => OutputTransform::Flipped,
                    Transform::Flipped90 => OutputTransform::Flipped90,
                    Transform::Flipped180 => OutputTransform::Flipped180,
                    Transform::Flipped270 => OutputTransform::Flipped270,
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

        let output_by_name = outputs
            .iter()
            .map(|output| (output.id.as_str(), output))
            .collect::<HashMap<_, _>>();
        let geometries = geometries
            .into_iter()
            .map(|geometry| (geometry.id, geometry))
            .collect::<HashMap<_, _>>();

        // Preserve stacking order; skip entries lost between IPC snapshots.
        let windows = windows
            .into_iter()
            .map(|window| window.id)
            .filter_map(|window_id| {
                let geometry = geometries.get(&window_id)?;
                let output = output_by_name.get(geometry.output.as_str())?;

                let geometry = Rect::new(
                    output.logical_geometry.left() + geometry.x,
                    output.logical_geometry.top() + geometry.y,
                    geometry.width,
                    geometry.height,
                );
                let geometry = geometry.intersection(output.logical_geometry)?;

                Some(Window { geometry })
            })
            .collect();

        Ok(Scene { outputs, windows })
    }
}

fn is_unsupported_window_geometries(error: &str) -> bool {
    error.contains("error parsing request") && error.contains("unknown variant `WindowGeometries`")
}

#[cfg(test)]
mod tests {
    use super::is_unsupported_window_geometries;

    #[test]
    fn recognizes_only_the_missing_window_geometries_request() {
        assert!(is_unsupported_window_geometries(
            "error parsing request: unknown variant `WindowGeometries`, expected `Outputs`"
        ));
        assert!(!is_unsupported_window_geometries(
            "error parsing request: unknown variant `Outputs`, expected `Windows`"
        ));
        assert!(!is_unsupported_window_geometries(
            "failed to retrieve WindowGeometries"
        ));
    }
}
