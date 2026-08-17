use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use anyhow::Result;
use serde::de::DeserializeOwned;

pub(super) fn request_json<T: DeserializeOwned>(path: &Path, command: &[u8]) -> Result<T> {
    let mut socket = UnixStream::connect(path)?;
    socket.write_all(command)?;

    let mut reply = Vec::new();
    socket.read_to_end(&mut reply)?;

    Ok(serde_json::from_slice(&reply)?)
}
