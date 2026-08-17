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
        let Some(path) = env::var_os("MANGO_INSTANCE_SIGNATURE") else {
            bail!("MANGO_INSTANCE_SIGNATURE is not set");
        };

        Ok(Self { path: path.into() })
    }

    pub(super) fn clients(&self) -> Result<Vec<Client>> {
        Ok(request_json::<Reply>(&self.path, b"get all-clients\n")?.clients)
    }
}

#[derive(Deserialize)]
struct Reply {
    clients: Vec<Client>,
}

#[derive(Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub(super) struct Client {
    pub(super) is_visible: bool,
    pub(super) is_focused: bool,
    pub(super) is_floating: bool,
    pub(super) is_overlay: bool,
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: i32,
    pub(super) height: i32,
}
