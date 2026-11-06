//! Niri scene discovery through its standard JSON IPC.

use std::collections::HashMap;

use anyhow::{Context, Result};

#[cfg(feature = "niri-capture")]
pub(super) use self::capture::NiriWindowCapture;
use self::ipc::{Request, Response, Socket, Transform, Window as IpcWindow, Workspace};
use super::SceneReader;
use crate::{Output, OutputId, OutputTransform, Rect, Scene, Size, Window};

#[cfg(feature = "niri-capture")]
mod capture;
mod ipc;

/// [niri](https://github.com/niri-wm/niri) window discovery.
pub(super) struct Niri {
    socket: Socket,
}

impl Niri {
    pub(super) fn connect() -> Result<Self> {
        let socket = Socket::connect().context("failed to connect to the Niri IPC socket")?;

        Ok(Self { socket })
    }

    fn request(&mut self, request: &Request) -> Result<Response> {
        self.socket
            .send(request)
            .context("failed to communicate with Niri")?
            .map_err(anyhow::Error::msg)
    }

    fn outputs(&mut self) -> Result<Vec<Output>> {
        let Response::Outputs(outputs) = self.request(&Request::Outputs)? else {
            panic!("Niri returned an unexpected response to Outputs");
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

        Ok(outputs)
    }

    fn windows(&mut self, outputs: &[Output]) -> Result<Vec<Window>> {
        let Response::Workspaces(workspaces) = self.request(&Request::Workspaces)? else {
            panic!("Niri returned an unexpected response to Workspaces");
        };
        let Response::Windows(ipc_windows) = self.request(&Request::Windows)? else {
            panic!("Niri returned an unexpected response to Windows");
        };

        Ok(visible_windows(ipc_windows, &workspaces, outputs))
    }
}

impl SceneReader for Niri {
    fn scene(&mut self) -> Result<Scene> {
        let outputs = self.outputs()?;
        let windows = self.windows(&outputs)?;

        Ok(Scene { outputs, windows })
    }
}

fn visible_windows(
    ipc_windows: Vec<IpcWindow>,
    workspaces: &[Workspace],
    outputs: &[Output],
) -> Vec<Window> {
    let output_by_name = outputs
        .iter()
        .map(|output| (output.id.as_str(), output))
        .collect::<HashMap<_, _>>();
    let output_by_workspace = workspaces
        .iter()
        .filter(|workspace| workspace.is_active)
        .filter_map(|workspace| {
            let output = *output_by_name.get(workspace.output.as_deref()?)?;

            Some((workspace.id, output))
        })
        .collect::<HashMap<_, _>>();

    // Preserve stacking order; skip entries lost between IPC snapshots or
    // currently outside their workspace view.
    ipc_windows
        .into_iter()
        .filter_map(|ipc_window| {
            let (x, y) = ipc_window.layout.tile_pos_in_workspace_view?;
            let output = *output_by_workspace.get(&ipc_window.workspace_id?)?;
            let (width, height) = ipc_window.layout.tile_size;
            let geometry = Rect::new(
                output.logical_geometry.left() + x,
                output.logical_geometry.top() + y,
                width,
                height,
            )
            .intersection(output.logical_geometry)?;

            Some(Window {
                geometry,
                identifier: Some(ipc_window.id.to_string()),
            })
        })
        .collect()
}
