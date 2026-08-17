use anyhow::Result;

use self::ipc::{Node, Socket};
use super::SceneReader;
use super::generic::Generic;
use crate::{Rect, Scene, Window};

mod ipc;

/// [Sway](https://github.com/swaywm/sway) window discovery.
pub(super) struct Sway {
    outputs: Generic,
    socket: Socket,
}

impl Sway {
    pub(super) fn connect() -> Result<Self> {
        Ok(Self {
            outputs: Generic::connect()?,
            socket: Socket::connect()?,
        })
    }
}

impl SceneReader for Sway {
    fn scene(&mut self) -> Result<Scene> {
        let tree = self.socket.tree()?;
        let mut scene = self.outputs.scene()?;
        tree.append_windows(&mut scene.windows);

        Ok(scene)
    }
}

impl Node {
    fn append_windows(self, windows: &mut Vec<Window>) {
        for node in self.floating_nodes.into_iter().rev() {
            node.append_windows(windows);
        }
        if self.visible == Some(true) {
            windows.push(Window {
                geometry: Rect::new(
                    f64::from(self.rect.x),
                    f64::from(self.rect.y),
                    f64::from(self.rect.width),
                    f64::from(self.rect.height),
                ),
            });
        }
        for node in self.nodes.into_iter().rev() {
            node.append_windows(windows);
        }
    }
}
