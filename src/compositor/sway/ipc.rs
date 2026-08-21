use std::env;
use std::io::{BufReader, Read, Write};
use std::os::unix::net::UnixStream;

use anyhow::{Result, bail};
use serde::Deserialize;

const GET_TREE: u32 = 4;

pub(super) struct Socket {
    stream: BufReader<UnixStream>,
}

impl Socket {
    pub(super) fn connect() -> Result<Self> {
        let Some(path) = env::var_os("SWAYSOCK") else {
            bail!("SWAYSOCK is not set");
        };

        Ok(Self {
            stream: BufReader::new(UnixStream::connect(path)?),
        })
    }

    pub(super) fn tree(&mut self) -> Result<Node> {
        let mut request = Vec::with_capacity(14);
        request.extend_from_slice(b"i3-ipc");
        request.extend_from_slice(&0_u32.to_ne_bytes());
        request.extend_from_slice(&GET_TREE.to_ne_bytes());
        self.stream.get_mut().write_all(&request)?;

        let mut header = [0; 14];
        self.stream.read_exact(&mut header)?;
        assert_eq!(&header[..6], b"i3-ipc");
        assert_eq!(
            u32::from_ne_bytes(header[10..14].try_into().unwrap()),
            GET_TREE
        );
        let length = u32::from_ne_bytes(header[6..10].try_into().unwrap());
        let mut payload = vec![0; length as usize];
        self.stream.read_exact(&mut payload)?;

        Ok(serde_json::from_slice(&payload)?)
    }
}

#[derive(Deserialize)]
pub(super) struct Node {
    pub(super) foreign_toplevel_identifier: Option<String>,
    pub(super) rect: Geometry,
    pub(super) visible: Option<bool>,
    pub(super) nodes: Vec<Self>,
    pub(super) floating_nodes: Vec<Self>,
}

#[derive(Deserialize)]
pub(super) struct Geometry {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: i32,
    pub(super) height: i32,
}
