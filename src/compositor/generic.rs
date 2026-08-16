use anyhow::{Context, Result};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::{delegate_registry, registry_handlers};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_output::{self, Transform};
use wayland_client::{Connection, QueueHandle};

use super::SceneReader;
use crate::{Output, OutputId, OutputTransform, Rect, Scene, Size};

/// Output-only scene backend for compositors without a dedicated integration.
pub(super) struct Generic {
    outputs: Vec<Output>,
}

impl Generic {
    pub(super) fn connect() -> Result<Self> {
        let connection = Connection::connect_to_env()?;
        let (globals, mut event_queue) = registry_queue_init(&connection)?;
        let qh = event_queue.handle();
        let mut state = State {
            registry_state: RegistryState::new(&globals),
            output_state: OutputState::new(&globals, &qh),
        };

        event_queue
            .roundtrip(&mut state)
            .context("failed to read Wayland output information")?;

        let mut outputs = state
            .output_state
            .outputs()
            .map(|output| {
                let info = state.output_state.info(&output).unwrap();
                let (mode_width, mode_height) = info
                    .modes
                    .iter()
                    .find(|mode| mode.current)
                    .unwrap()
                    .dimensions;
                let (transform, swaps_axes) = match info.transform {
                    Transform::Normal => (OutputTransform::Normal, false),
                    Transform::_90 => (OutputTransform::Rotate90, true),
                    Transform::_180 => (OutputTransform::Rotate180, false),
                    Transform::_270 => (OutputTransform::Rotate270, true),
                    Transform::Flipped => (OutputTransform::Flipped, false),
                    Transform::Flipped90 => (OutputTransform::Flipped90, true),
                    Transform::Flipped180 => (OutputTransform::Flipped180, false),
                    Transform::Flipped270 => (OutputTransform::Flipped270, true),
                    _ => unreachable!(),
                };
                let (pixel_width, pixel_height) = if swaps_axes {
                    (mode_height, mode_width)
                } else {
                    (mode_width, mode_height)
                };
                let pixel_size = Size::new(f64::from(pixel_width), f64::from(pixel_height));
                let (x, y, logical_width, logical_height) =
                    match (info.logical_position, info.logical_size) {
                        (Some((x, y)), Some((width, height))) => {
                            (x, y, f64::from(width), f64::from(height))
                        }
                        _ => (
                            info.location.0,
                            info.location.1,
                            pixel_size.width / f64::from(info.scale_factor),
                            pixel_size.height / f64::from(info.scale_factor),
                        ),
                    };
                let scale =
                    (pixel_size.width / logical_width).max(pixel_size.height / logical_height);

                Output {
                    id: OutputId::new(info.name.unwrap()),
                    logical_geometry: Rect::new(
                        f64::from(x),
                        f64::from(y),
                        logical_width,
                        logical_height,
                    ),
                    pixel_size,
                    scale,
                    transform,
                }
            })
            .collect::<Vec<_>>();
        outputs.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

        Ok(Self { outputs })
    }
}

impl SceneReader for Generic {
    fn scene(&self) -> Result<Scene> {
        Ok(Scene {
            outputs: self.outputs.clone(),
            windows: Vec::new(),
        })
    }
}

struct State {
    registry_state: RegistryState,
    output_state: OutputState,
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

delegate_registry!(State);

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(State);
