use anyhow::Result;

use self::ipc::Socket;
use super::SceneReader;
use super::generic::Generic;
use crate::{Rect, Scene, Window};

mod ipc;

/// [Hyprland](https://hypr.land/) window discovery.
pub(super) struct Hyprland {
    outputs: Generic,
    socket: Socket,
}

impl Hyprland {
    pub(super) fn connect() -> Result<Self> {
        Ok(Self {
            outputs: Generic::connect()?,
            socket: Socket::connect()?,
        })
    }
}

impl SceneReader for Hyprland {
    fn scene(&mut self) -> Result<Scene> {
        let monitors = self.socket.monitors()?;
        let workspaces = monitors
            .iter()
            .map(|monitor| monitor.active_workspace.id)
            .filter(|id| *id != 0)
            .collect::<Vec<_>>();
        let special_workspaces = monitors
            .into_iter()
            .map(|monitor| monitor.special_workspace.id)
            .filter(|id| *id != 0)
            .collect::<Vec<_>>();
        let mut clients = self.socket.clients()?;

        // Hyprland stores and reports windows from bottom to top.
        clients.reverse();
        clients.sort_by_key(|client| {
            (
                !client.pinned,
                !special_workspaces.contains(&client.workspace.id),
                !client.floating,
            )
        });

        let mut scene = self.outputs.scene()?;
        scene.windows = clients
            .into_iter()
            .filter(|client| {
                !client.hidden
                    && client.visible != Some(false)
                    && (client.pinned
                        || workspaces.contains(&client.workspace.id)
                        || special_workspaces.contains(&client.workspace.id))
            })
            .map(|client| Window {
                identifier: client.stable_id,
                geometry: Rect::new(
                    f64::from(client.at[0]),
                    f64::from(client.at[1]),
                    f64::from(client.size[0]),
                    f64::from(client.size[1]),
                ),
            })
            .collect();

        Ok(scene)
    }
}
