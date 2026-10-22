use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub(super) enum Request {
    Windows,
    Workspaces,
    OverviewState,
}

#[derive(Deserialize)]
pub(super) enum Response {
    Windows(Vec<Window>),
    Workspaces(Vec<Workspace>),
    OverviewState(Overview),
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
    secs: u64,
    nanos: u32,
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
