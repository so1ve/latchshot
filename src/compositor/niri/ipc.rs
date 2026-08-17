//! Minimal blocking client for the subset of Niri's JSON IPC used here.
//!
//! Keeping the wire types local lets latchshot speak the custom
//! `WindowGeometries` extension without depending on an unpublished Git crate.

use std::collections::HashMap;
use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use serde::{Deserialize, Serialize};

const SOCKET_PATH_ENV: &str = "NIRI_SOCKET";

pub(super) struct Socket {
    stream: BufReader<UnixStream>,
}

impl Socket {
    pub(super) fn connect() -> io::Result<Self> {
        let path = env::var_os(SOCKET_PATH_ENV).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{SOCKET_PATH_ENV} is not set, are you running within Niri?"),
            )
        })?;
        let stream = UnixStream::connect(path)?;

        Ok(Self {
            stream: BufReader::new(stream),
        })
    }

    pub(super) fn send(&mut self, request: Request) -> io::Result<Result<Response, String>> {
        let mut message = serde_json::to_vec(&request)?;
        message.push(b'\n');
        self.stream.get_mut().write_all(&message)?;

        let mut reply = String::new();
        if self.stream.read_line(&mut reply)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Niri closed the IPC socket without replying",
            ));
        }

        Ok(serde_json::from_str(&reply)?)
    }
}

#[derive(Clone, Copy, Serialize)]
pub(super) enum Request {
    Outputs,
    Workspaces,
    Windows,
    OverviewState,
    WindowGeometries,
}

#[derive(Deserialize)]
pub(super) enum Response {
    Outputs(HashMap<String, Output>),
    Workspaces(Vec<Workspace>),
    Windows(Vec<Window>),
    OverviewState(Overview),
    WindowGeometries(Vec<WindowGeometry>),
}

#[derive(Deserialize)]
pub(super) struct Output {
    pub(super) name: String,
    pub(super) modes: Vec<Mode>,
    pub(super) current_mode: Option<usize>,
    pub(super) logical: Option<LogicalOutput>,
}

#[derive(Deserialize)]
pub(super) struct Mode {
    pub(super) width: u16,
    pub(super) height: u16,
}

#[derive(Clone, Copy, Deserialize)]
pub(super) struct LogicalOutput {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) scale: f64,
    pub(super) transform: Transform,
}

#[derive(Clone, Copy, Deserialize)]
pub(super) enum Transform {
    Normal,
    #[serde(rename = "90")]
    _90,
    #[serde(rename = "180")]
    _180,
    #[serde(rename = "270")]
    _270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

#[derive(Deserialize)]
pub(super) struct Window {
    pub(super) id: u64,
    pub(super) workspace_id: Option<u64>,
    #[serde(default)]
    pub(super) is_floating: bool,
    #[serde(default)]
    pub(super) layout: WindowLayout,
    pub(super) focus_timestamp: Option<Timestamp>,
}

#[derive(Default, Deserialize)]
pub(super) struct WindowLayout {
    #[serde(default)]
    pub(super) pos_in_scrolling_layout: Option<(usize, usize)>,
    #[serde(default)]
    pub(super) tile_size: (f64, f64),
    #[serde(default)]
    pub(super) window_size: (f64, f64),
    #[serde(default)]
    pub(super) tile_pos_in_workspace_view: Option<(f64, f64)>,
    #[serde(default)]
    pub(super) window_offset_in_tile: (f64, f64),
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Timestamp {
    pub(super) secs: u64,
    pub(super) nanos: u32,
}

#[derive(Deserialize)]
pub(super) struct Workspace {
    pub(super) id: u64,
    pub(super) output: Option<String>,
    #[serde(default)]
    pub(super) is_active: bool,
    pub(super) active_window_id: Option<u64>,
}

#[derive(Deserialize)]
pub(super) struct Overview {
    pub(super) is_open: bool,
}

#[derive(Deserialize)]
pub(super) struct WindowGeometry {
    pub(super) id: u64,
    pub(super) output: String,
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
}
