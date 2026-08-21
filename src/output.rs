//! PNG encoding and output destinations for captured images.
//!
//! [`Target`] can save an image, stream it to standard output, or hand it to
//! `wl-copy`. [`write`] sends the same encoded PNG to any combination of
//! targets. Applications may also use the returned `image::RgbaImage` directly
//! instead.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use log::warn;
use notify_rust::Notification;

/// Destination for an encoded PNG screenshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    File(PathBuf),
    Stdout,
    Clipboard,
}

impl Target {
    fn write_png(&self, png: &[u8]) -> Result<()> {
        match self {
            Self::File(path) => fs::write(path, png)
                .with_context(|| format!("failed to write screenshot to {}", path.display())),
            Self::Stdout => {
                let stdout = io::stdout();
                let mut stdout = stdout.lock();

                stdout.write_all(png)?;
                stdout.flush()?;

                Ok(())
            }
            Self::Clipboard => {
                let mut child = Command::new("wl-copy")
                    .args(["--type", "image/png"])
                    .stdin(Stdio::piped())
                    .spawn()
                    .context("failed to start wl-copy")?;

                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(png)
                    .context("failed to send screenshot to wl-copy")?;

                let status = child.wait()?;
                if !status.success() {
                    bail!("wl-copy exited with {status}");
                }

                Ok(())
            }
        }
    }
}

/// Encodes an image once and writes the PNG to every target in order.
pub fn write(image: &RgbaImage, targets: &[Target]) -> Result<()> {
    if targets.is_empty() {
        return Ok(());
    }

    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::default())
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .unwrap();

    for target in targets {
        target.write_png(&png)?;
    }

    Ok(())
}

/// Sends a desktop notification without propagating failures.
pub fn notify(body: &str) {
    if let Err(error) = Notification::new()
        .appname("latchshot")
        .summary("latchshot")
        .body(body)
        .show()
    {
        warn!("failed to send a notification: {error}");
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, process};

    use image::Rgba;

    use super::*;

    fn sample_image() -> RgbaImage {
        RgbaImage::from_fn(2, 1, |x, _| match x {
            0 => Rgba([1, 2, 3, 255]),
            1 => Rgba([40, 50, 60, 128]),
            _ => unreachable!(),
        })
    }

    #[test]
    fn writes_to_the_exact_requested_path() {
        let path =
            std::env::temp_dir().join(format!("latchshot-output-test-{}.png", process::id()));
        write(&sample_image(), &[Target::File(path.clone())]).unwrap();

        let decoded = image::open(&path).unwrap().into_rgba8();
        fs::remove_file(path).unwrap();

        assert_eq!(decoded, sample_image());
    }
}
