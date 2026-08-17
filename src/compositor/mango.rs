use anyhow::Result;

use self::ipc::Socket;
use super::SceneReader;
use super::generic::Generic;
use crate::{Rect, Scene, Window};

mod ipc;

/// [Mango](https://github.com/mangowm/mango) window discovery.
pub(super) struct Mango {
    outputs: Generic,
    socket: Socket,
}

impl Mango {
    pub(super) fn connect() -> Result<Self> {
        Ok(Self {
            outputs: Generic::connect()?,
            socket: Socket::connect()?,
        })
    }
}

impl SceneReader for Mango {
    fn scene(&mut self) -> Result<Scene> {
        let mut clients = self
            .socket
            .clients()?
            .into_iter()
            .filter(|client| client.is_visible)
            .collect::<Vec<_>>();
        clients.sort_by_key(|client| (!client.is_overlay, !client.is_focused, !client.is_floating));

        let mut scene = self.outputs.scene()?;
        scene.windows = clients
            .into_iter()
            .map(|client| Window {
                geometry: Rect::new(
                    f64::from(client.x),
                    f64::from(client.y),
                    f64::from(client.width),
                    f64::from(client.height),
                ),
            })
            .collect();

        Ok(scene)
    }
}
