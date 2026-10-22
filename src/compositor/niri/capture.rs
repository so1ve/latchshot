use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use std::{env, fs, process, thread};

use anyhow::{Context, Result};
use image::RgbaImage;
use log::info;
use serde::{Deserialize, Serialize};

use super::ipc::Socket;
use crate::{Window, WindowCapture};

static SCREENSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const SCREENSHOT_TIMEOUT: Duration = Duration::from_secs(3);
const SCREENSHOT_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Serialize)]
enum Request {
    Action(Action),
}

#[derive(Serialize)]
enum Action {
    ScreenshotWindow {
        id: Option<u64>,
        write_to_disk: bool,
        show_pointer: bool,
        path: Option<String>,
    },
}

#[derive(Deserialize)]
enum Response {
    Handled,
}

pub struct NiriWindowCapture;

impl WindowCapture for NiriWindowCapture {
    fn capture_window(&mut self, window: &Window) -> Result<Option<RgbaImage>> {
        let Some(id) = window
            .identifier
            .as_deref()
            .and_then(|identifier| identifier.parse::<u64>().ok())
        else {
            return Ok(None);
        };
        let runtime_dir = env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
        let sequence = SCREENSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(runtime_dir)
            .join(format!("latchshot-window-{}-{sequence}.png", process::id()));
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("failed to clear stale Niri screenshot"),
        }
        let cleanup = TemporaryScreenshot(path);
        let path = cleanup
            .0
            .to_str()
            .context("Niri screenshot path is not valid UTF-8")?
            .to_owned();
        let mut socket = Socket::connect().context("failed to connect to the Niri IPC socket")?;
        let response = socket
            .send(&Request::Action(Action::ScreenshotWindow {
                id: Some(id),
                write_to_disk: true,
                show_pointer: false,
                path: Some(path),
            }))
            .context("failed to request a Niri window screenshot")?
            .map_err(anyhow::Error::msg)?;
        let Response::Handled = response;

        let deadline = Instant::now() + SCREENSHOT_TIMEOUT;
        loop {
            match image::open(&cleanup.0) {
                Ok(image) => {
                    info!("captured window {id} through Niri IPC");

                    return Ok(Some(image.into_rgba8()));
                }
                Err(error) if Instant::now() >= deadline => {
                    return Err(error).context("failed to read Niri window screenshot");
                }
                Err(_) => thread::sleep(SCREENSHOT_POLL_INTERVAL),
            }
        }
    }
}

struct TemporaryScreenshot(PathBuf);

impl Drop for TemporaryScreenshot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
