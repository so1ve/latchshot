use std::env;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{ArgGroup, Parser};
use latchshot::output::{Target, notify};
use latchshot::overlay::select;
use latchshot::{Compositor, FrameCapture, Selection, SelectionResult, WaylandCapture};
use log::info;

#[derive(Parser)]
#[command(version, about)]
#[command(group(ArgGroup::new("destination").multiple(false)))]
#[allow(clippy::struct_excessive_bools)]
struct Args {
    /// Write the screenshot to PATH
    #[arg(short, long, value_name = "PATH", group = "destination")]
    output: Option<PathBuf>,

    /// Write PNG data to stdout
    #[arg(long, group = "destination")]
    stdout: bool,

    /// Copy the PNG to the Wayland clipboard (the default)
    #[arg(short, long, group = "destination")]
    clipboard: bool,

    /// Print the scene as JSON
    #[arg(long, conflicts_with_all = ["output", "stdout", "clipboard"])]
    windows: bool,

    /// Disable snap animation
    #[arg(long)]
    no_animation: bool,

    /// Compositor backend (auto-detected, or generic when unknown)
    #[arg(long)]
    compositor: Option<Compositor>,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    let Some(capture_lock) = try_acquire_capture_lock()? else {
        info!("another latchshot capture is already in progress; exiting");

        return Ok(());
    };

    let compositor = args
        .compositor
        .or_else(Compositor::detect)
        .unwrap_or(Compositor::Generic);
    info!("compositor backend: {compositor}");
    let mut compositor = compositor.connect()?;
    let mut scene = compositor.scene()?;
    if scene.outputs.is_empty() {
        bail!("the compositor reported no active outputs");
    }

    let mut capture = WaylandCapture::connect()?;
    let frame = capture.capture(&scene)?;
    compositor.refine_scene(&mut scene, &frame)?;
    info!(
        "scene: {} outputs, {} windows",
        scene.outputs.len(),
        scene.windows.len()
    );

    if args.windows {
        println!("{}", serde_json::to_string_pretty(&scene).unwrap());

        return Ok(());
    }

    let (result, frame) = select(scene, frame, !args.no_animation)?;
    drop(capture_lock);

    let selection = match result {
        SelectionResult::Selected(selection) => selection,
        SelectionResult::Cancelled => return Ok(()),
    };
    let geometry = match selection {
        Selection::Window(geometry) | Selection::Region(geometry) => geometry,
    };

    let image = frame.crop(geometry);
    let target = if args.stdout {
        Target::Stdout
    } else if let Some(path) = args.output {
        Target::File(path)
    } else {
        Target::Clipboard
    };
    target.write(&image)?;

    match target {
        Target::File(path) => notify(&format!("Screenshot saved to {}", path.display())),
        Target::Clipboard => notify("Screenshot copied to clipboard"),
        Target::Stdout => {}
    }

    Ok(())
}

fn try_acquire_capture_lock() -> Result<Option<File>> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    let path = PathBuf::from(runtime_dir).join("latchshot.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("failed to open capture lock {}", path.display()))?;

    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => {
            Err(error).with_context(|| format!("failed to acquire capture lock {}", path.display()))
        }
    }
}
