use std::env;
use std::path::PathBuf;

use anyhow::{Result, bail};
use serde::Deserialize;

use super::super::ipc::request_json;

pub(super) struct Socket {
    path: PathBuf,
}

impl Socket {
    pub(super) fn connect() -> Result<Self> {
        let Some(runtime) = env::var_os("XDG_RUNTIME_DIR") else {
            bail!("XDG_RUNTIME_DIR is not set");
        };
        let Some(signature) = env::var_os("HYPRLAND_INSTANCE_SIGNATURE") else {
            bail!("HYPRLAND_INSTANCE_SIGNATURE is not set");
        };

        Ok(Self {
            path: PathBuf::from(runtime)
                .join("hypr")
                .join(signature)
                .join(".socket.sock"),
        })
    }

    pub(super) fn monitors(&self) -> Result<Vec<Monitor>> {
        request_json(&self.path, b"j/monitors")
    }

    pub(super) fn clients(&self) -> Result<Vec<Client>> {
        request_json(&self.path, b"j/clients")
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Monitor {
    pub(super) active_workspace: Workspace,
    pub(super) special_workspace: Workspace,
}

#[derive(Deserialize)]
pub(super) struct Client {
    pub(super) hidden: bool,
    pub(super) visible: Option<bool>,
    pub(super) at: [i32; 2],
    pub(super) size: [i32; 2],
    pub(super) workspace: Workspace,
    pub(super) floating: bool,
    pub(super) pinned: bool,
}

#[derive(Deserialize)]
pub(super) struct Workspace {
    pub(super) id: i64,
}
