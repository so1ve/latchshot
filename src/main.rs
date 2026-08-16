use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{ArgGroup, Parser};
use latchshot::output::{Target, notify};
use latchshot::overlay::select;
use latchshot::{CaptureBackend, Compositor, Selection, SelectionResult};
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

    /// Capture backend (auto-detected when omitted)
    #[arg(long)]
    capture: Option<CaptureBackend>,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    let compositor = args
        .compositor
        .or_else(Compositor::detect)
        .unwrap_or(Compositor::Generic);
    info!("compositor backend: {compositor}");
    let mut compositor = compositor.connect()?;
    let scene = compositor.scene()?;
    info!(
        "scene: {} outputs, {} windows",
        scene.outputs.len(),
        scene.windows.len()
    );

    if args.windows {
        println!("{}", serde_json::to_string_pretty(&scene).unwrap());

        return Ok(());
    }
    if scene.outputs.is_empty() {
        bail!("the compositor reported no active outputs");
    }

    let capture = args
        .capture
        .or_else(CaptureBackend::detect)
        .context("failed to detect a supported capture backend; pass --capture to override")?;
    info!("capture backend: {capture}");
    let mut capture = capture.connect()?;
    let frame = capture.capture(&scene)?;
    let (result, frame) = select(scene, frame, !args.no_animation)?;
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
