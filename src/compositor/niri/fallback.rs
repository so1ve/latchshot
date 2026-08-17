//! Temporary compatibility path for Niri releases without `WindowGeometries`.
//!
//! Keep all capture-assisted reconstruction here so the module can be removed
//! once upstream Niri exposes exact on-screen window geometry.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use image::RgbaImage;
use log::{debug, warn};

use super::ipc::{Request, Response, Socket, Window as IpcWindow, Workspace};
use crate::{DesktopFrame, Output, OutputFrame, Rect, Scene, Window};

const EDGE_SAMPLES: u32 = 96;
const MIN_EDGE_STRENGTH: f64 = 12.0;

pub(super) struct Snapshot {
    windows: Vec<IpcWindow>,
    workspaces: Vec<Workspace>,
    overview_open: bool,
}

impl Snapshot {
    pub(super) fn read(socket: &mut Socket, windows: Vec<IpcWindow>) -> Result<Self> {
        let Response::Workspaces(workspaces) = socket
            .send(Request::Workspaces)
            .context("failed to communicate with Niri")?
            .map_err(anyhow::Error::msg)?
        else {
            panic!("Niri returned an unexpected response to Workspaces");
        };
        let overview_open = match socket
            .send(Request::OverviewState)
            .context("failed to communicate with Niri")?
        {
            Ok(Response::OverviewState(overview)) => overview.is_open,
            Ok(_) => panic!("Niri returned an unexpected response to OverviewState"),
            Err(error) => {
                warn!("could not read Niri overview state: {error}");
                false
            }
        };

        Ok(Self {
            windows,
            workspaces,
            overview_open,
        })
    }
}

pub(super) fn reconstruct(
    snapshot: &Snapshot,
    scene: &Scene,
    desktop: &DesktopFrame,
) -> Vec<Window> {
    if snapshot.overview_open {
        debug!("Niri overview is open; window reconstruction is unavailable");

        return Vec::new();
    }

    let mut windows = Vec::new();

    for output in &scene.outputs {
        let Some(workspace) = snapshot.workspaces.iter().find(|workspace| {
            workspace.is_active && workspace.output.as_deref() == Some(output.id.as_str())
        }) else {
            continue;
        };
        let frame = desktop
            .outputs
            .iter()
            .find(|frame| frame.output == output.id)
            .unwrap();
        let workspace_windows = snapshot
            .windows
            .iter()
            .filter(|window| window.workspace_id == Some(workspace.id))
            .collect::<Vec<_>>();

        let tiled = reconstruct_tiled(output, frame, workspace, &workspace_windows);
        if floats_are_visible(output, workspace, &workspace_windows) {
            let mut floating = workspace_windows
                .iter()
                .copied()
                .filter(|window| window.is_floating)
                .collect::<Vec<_>>();
            floating.sort_by_key(|window| Reverse(window.focus_timestamp));

            windows.extend(floating.into_iter().filter_map(|window| {
                let (x, y) = window.layout.tile_pos_in_workspace_view?;

                window_geometry(output, window, x, y)
            }));
        }
        if let Some(tiled) = tiled {
            windows.extend(tiled);
        }
    }

    windows
}

struct Column<'a> {
    tiles: Vec<&'a IpcWindow>,
    width: f64,
    tabbed: bool,
}

impl<'a> Column<'a> {
    fn visible_tiles(&self, active_window: Option<u64>) -> Vec<&'a IpcWindow> {
        if !self.tabbed {
            return self.tiles.clone();
        }

        active_window
            .and_then(|id| self.tiles.iter().find(|window| window.id == id).copied())
            .or_else(|| {
                self.tiles
                    .iter()
                    .copied()
                    .max_by_key(|window| window.focus_timestamp)
            })
            .into_iter()
            .collect()
    }
}

fn reconstruct_tiled(
    output: &Output,
    frame: &OutputFrame,
    workspace: &Workspace,
    windows: &[&IpcWindow],
) -> Option<Vec<Window>> {
    let mut grouped = BTreeMap::<usize, Vec<&IpcWindow>>::new();
    for window in windows.iter().copied().filter(|window| !window.is_floating) {
        let Some((column, _)) = window.layout.pos_in_scrolling_layout else {
            continue;
        };
        grouped.entry(column).or_default().push(window);
    }
    let columns = grouped
        .into_values()
        .map(|mut tiles| {
            tiles.sort_by_key(|window| window.layout.pos_in_scrolling_layout.unwrap().1);
            let width = tiles
                .iter()
                .map(|window| window.layout.tile_size.0)
                .reduce(f64::max)
                .unwrap();
            let stacked_height = tiles
                .iter()
                .map(|window| window.layout.tile_size.1)
                .sum::<f64>();

            Column {
                tiles,
                width,
                tabbed: stacked_height > output.logical_geometry.height() + 0.5,
            }
        })
        .collect::<Vec<_>>();
    if columns.is_empty() {
        return Some(Vec::new());
    }

    let widths = columns
        .iter()
        .map(|column| column.width)
        .collect::<Vec<_>>();
    let focused_column = workspace.active_window_id.and_then(|id| {
        columns
            .iter()
            .position(|column| column.tiles.iter().any(|window| window.id == id))
    });
    let horizontal_profile =
        EdgeProfile::vertical(&frame.image, 0, frame.image.height(), frame.scale_x());
    let horizontal = fit_axis(
        &horizontal_profile,
        &widths,
        output.logical_geometry.width(),
        frame.scale_x(),
        focused_column,
        true,
        None,
    )
    .or_else(|| {
        (widths.len() == 1 && (widths[0] - output.logical_geometry.width()).abs() <= 1.0).then_some(
            Axis {
                origin: 0.0,
                gap: None,
            },
        )
    });
    let Some(horizontal) = horizontal else {
        debug!(
            "could not resolve Niri's horizontal layout on {}",
            output.id
        );

        return None;
    };

    let horizontal_gap = horizontal.gap.unwrap_or(0.0);
    let mut x = horizontal.origin;
    let column_x = columns
        .iter()
        .map(|column| {
            let current = x;
            x += column.width + horizontal_gap;

            current
        })
        .collect::<Vec<_>>();
    let sample_column = columns
        .iter()
        .zip(&column_x)
        .enumerate()
        .filter_map(|(index, (column, x))| {
            let left = x.max(0.0);
            let right = (x + column.width).min(output.logical_geometry.width());

            (right - left >= 2.0).then_some((index, left, right))
        })
        .max_by(|a, b| (a.2 - a.1).total_cmp(&(b.2 - b.1)))?;
    let vertical_tiles = columns[sample_column.0].visible_tiles(workspace.active_window_id);
    let heights = vertical_tiles
        .iter()
        .map(|window| window.layout.tile_size.1)
        .collect::<Vec<_>>();
    let x0 = (sample_column.1 * frame.scale_x()).floor() as u32;
    let x1 = (sample_column.2 * frame.scale_x()).ceil() as u32;
    let vertical_profile = EdgeProfile::horizontal(&frame.image, x0, x1, frame.scale_y());
    let vertical = fit_axis(
        &vertical_profile,
        &heights,
        output.logical_geometry.height(),
        frame.scale_y(),
        None,
        false,
        horizontal.gap,
    )
    .or_else(|| {
        let gap = horizontal.gap.unwrap_or(0.0);
        let height = heights.iter().sum::<f64>() + gap * (heights.len() as f64 - 1.0);

        ((height - output.logical_geometry.height()).abs() <= 1.0).then_some(Axis {
            origin: 0.0,
            gap: (heights.len() > 1).then_some(gap),
        })
    });
    let Some(vertical) = vertical else {
        debug!("could not resolve Niri's vertical layout on {}", output.id);

        return None;
    };
    let gap = horizontal.gap.or(vertical.gap).unwrap_or(0.0);
    let mut result = Vec::new();

    for (column, x) in columns.iter().zip(column_x) {
        let mut y = vertical.origin;
        for window in column.visible_tiles(workspace.active_window_id) {
            if let Some(window) = window_geometry(output, window, x, y) {
                result.push(window);
            }
            y += window.layout.tile_size.1 + gap;
        }
    }

    Some(result)
}

fn floats_are_visible(output: &Output, workspace: &Workspace, windows: &[&IpcWindow]) -> bool {
    let Some(active) = workspace
        .active_window_id
        .and_then(|id| windows.iter().find(|window| window.id == id))
    else {
        return true;
    };
    if active.is_floating {
        return true;
    }

    let (width, height) = active.layout.tile_size;

    (width - output.logical_geometry.width()).abs() > 0.5
        || (height - output.logical_geometry.height()).abs() > 0.5
}

fn window_geometry(
    output: &Output,
    window: &IpcWindow,
    tile_x: f64,
    tile_y: f64,
) -> Option<Window> {
    let (offset_x, offset_y) = window.layout.window_offset_in_tile;
    let (width, height) = window.layout.window_size;
    let width = (width * output.scale).round() / output.scale;
    let height = (height * output.scale).round() / output.scale;
    let geometry = Rect::new(
        output.logical_geometry.left() + tile_x + offset_x,
        output.logical_geometry.top() + tile_y + offset_y,
        width,
        height,
    )
    .intersection(output.logical_geometry)?;

    Some(Window { geometry })
}

#[derive(Clone, Copy)]
struct Axis {
    origin: f64,
    gap: Option<f64>,
}

#[derive(Clone, Copy)]
struct Candidate {
    axis: Axis,
    score: f64,
}

#[allow(clippy::too_many_arguments)]
fn fit_axis(
    profile: &EdgeProfile,
    segments: &[f64],
    length: f64,
    scale: f64,
    focused_segment: Option<usize>,
    scrollable: bool,
    fixed_gap: Option<f64>,
) -> Option<Axis> {
    let gap_pixels = if segments.len() == 1 {
        0..=0
    } else if let Some(gap) = fixed_gap {
        let gap = (gap * scale).round() as u32;
        gap..=gap
    } else {
        0..=((length / 4.0).min(256.0) * scale).round() as u32
    };
    let edge_margin = f64::from(profile.tolerance + 1) / scale;
    let mut candidates = Vec::new();

    for gap_pixels in gap_pixels {
        let gap = f64::from(gap_pixels) / scale;
        let mut edges = Vec::with_capacity(segments.len() * 2);
        let mut cursor = 0.0;
        for segment in segments {
            edges.extend([cursor, cursor + segment]);
            cursor += segment + gap;
        }
        let total = cursor - gap;
        if !scrollable && total > length + 0.5 {
            continue;
        }

        let (mut lower, mut upper) = if scrollable && total > length {
            (-total, length)
        } else {
            (0.0, (length - total).max(0.0))
        };
        if let Some(index) = focused_segment {
            let offset = segments[..index].iter().sum::<f64>() + gap * index as f64;
            let minimum_visible = 0.5 / scale;

            // Only reject placements where the focused column is
            // entirely outside the output.
            lower = lower.max(-offset - segments[index] + minimum_visible);
            upper = upper.min(length - offset - minimum_visible);
        }
        if lower > upper {
            continue;
        }

        edges.sort_by(f64::total_cmp);
        edges.dedup_by(|a, b| (*a - *b).abs() * scale < 0.5);
        let mut origins = BTreeSet::new();
        for peak in &profile.peaks {
            let peak = *peak as f64 / scale;
            for edge in &edges {
                let origin = peak - edge;
                if lower - edge_margin <= origin && origin <= upper + edge_margin {
                    origins.insert((origin.clamp(lower, upper) * scale).round() as i64);
                }
            }
        }

        for origin in origins {
            let origin = (origin as f64 / scale).clamp(lower, upper);
            let mut hits = 0_usize;
            let mut seen = 0_usize;
            let mut strength = 0.0;
            let mut distance = 0.0;
            let mut matched_peaks = BTreeSet::new();

            for edge in &edges {
                let position = origin + edge;
                if position <= edge_margin || position >= length - edge_margin {
                    continue;
                }
                let value = profile.at(position * scale);
                if value >= profile.threshold
                    && let Some((peak, delta)) = profile.nearest_peak(position * scale)
                {
                    hits += 1;
                    matched_peaks.insert(peak);
                    distance += delta;
                }
                seen += 1;
                strength += value.ln_1p();
            }
            let explained = matched_peaks.len();
            let minimum_explained = if scrollable { 1 } else { 2 };
            if explained < minimum_explained || hits * 2 < seen {
                continue;
            }
            let mut cursor = origin;
            let visible_coverage: f64 = segments
                .iter()
                .map(|segment| {
                    let visible = ((cursor + segment).min(length) - cursor.max(0.0)).max(0.0);
                    cursor += segment + gap;

                    visible
                })
                .sum();

            candidates.push(Candidate {
                axis: Axis {
                    origin,
                    gap: (segments.len() > 1).then_some(gap),
                },
                score: 10.0 * explained as f64 / seen as f64
                    + explained as f64
                    + (strength - distance) / seen as f64
                    + visible_coverage / length,
            });
        }
    }

    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    let best = *candidates.first()?;
    let tolerance = f64::from(profile.tolerance * 2 + 1);
    let rival = candidates.iter().find(|candidate| {
        let origin_delta = (candidate.axis.origin - best.axis.origin).abs() * scale;
        let gap_delta = match (candidate.axis.gap, best.axis.gap) {
            (Some(a), Some(b)) => (a - b).abs() * scale,
            _ => 0.0,
        };

        origin_delta > tolerance || gap_delta > tolerance
    });
    if !scrollable && rival.is_some_and(|rival| best.score < rival.score * 1.04) {
        return None;
    }

    Some(best.axis)
}

struct EdgeProfile {
    values: Vec<f64>,
    peaks: Vec<usize>,
    threshold: f64,
    tolerance: u32,
}

impl EdgeProfile {
    fn vertical(image: &RgbaImage, y0: u32, y1: u32, scale: f64) -> Self {
        Self::new(image.width(), scale, |x| {
            edge_strength(image, x, y0, y1, true)
        })
    }

    fn horizontal(image: &RgbaImage, x0: u32, x1: u32, scale: f64) -> Self {
        Self::new(image.height(), scale, |y| {
            edge_strength(image, y, x0, x1, false)
        })
    }

    fn new(length: u32, scale: f64, mut measure: impl FnMut(u32) -> f64) -> Self {
        let raw = (0..length).map(&mut measure).collect::<Vec<_>>();
        let mut sorted = raw
            .iter()
            .copied()
            .filter(|value| *value > 0.0)
            .collect::<Vec<_>>();
        sorted.sort_by(f64::total_cmp);
        let threshold = sorted
            .get(sorted.len().saturating_sub(1) * 99 / 100)
            .map(|value| (value * 0.3).max(MIN_EDGE_STRENGTH))
            .unwrap_or(f64::INFINITY);
        let tolerance = scale.ceil().max(1.0) as u32 + 1;
        let values = (0..raw.len())
            .map(|index| {
                let start = index.saturating_sub(tolerance as usize);
                let end = (index + tolerance as usize + 1).min(raw.len());

                raw[start..end].iter().copied().reduce(f64::max).unwrap()
            })
            .collect();
        let mut peaks = Vec::new();
        let mut index = 0;
        while index < raw.len() {
            if raw[index] < threshold {
                index += 1;
                continue;
            }

            let start = index;
            while index + 1 < raw.len() && raw[index + 1] >= threshold {
                index += 1;
            }
            peaks.push((start + index) / 2);
            index += 1;
        }

        Self {
            values,
            peaks,
            threshold,
            tolerance,
        }
    }

    fn at(&self, position: f64) -> f64 {
        self.values
            .get(position.round() as usize)
            .copied()
            .unwrap_or(0.0)
    }

    fn nearest_peak(&self, position: f64) -> Option<(usize, f64)> {
        self.peaks
            .iter()
            .enumerate()
            .map(|(index, peak)| (index, (*peak as f64 - position).abs()))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, distance)| *distance <= f64::from(self.tolerance))
    }
}

fn edge_strength(
    image: &RgbaImage,
    axis: u32,
    orthogonal_start: u32,
    orthogonal_end: u32,
    vertical: bool,
) -> f64 {
    if axis == 0 {
        return 0.0;
    }

    let limit = if vertical {
        image.width()
    } else {
        image.height()
    };
    if axis >= limit || orthogonal_start >= orthogonal_end {
        return 0.0;
    }

    let orthogonal_limit = if vertical {
        image.height()
    } else {
        image.width()
    };
    let start = orthogonal_start.min(orthogonal_limit);
    let end = orthogonal_end.min(orthogonal_limit);
    if start >= end {
        return 0.0;
    }
    let step = ((end - start) / EDGE_SAMPLES).max(1);
    let mut differences = Vec::with_capacity(EDGE_SAMPLES as usize + 1);
    let mut orthogonal = start;

    while orthogonal < end {
        let (before, after) = if vertical {
            (
                image.get_pixel(axis - 1, orthogonal),
                image.get_pixel(axis, orthogonal),
            )
        } else {
            (
                image.get_pixel(orthogonal, axis - 1),
                image.get_pixel(orthogonal, axis),
            )
        };
        differences.push(
            before
                .0
                .iter()
                .zip(after.0.iter())
                .take(3)
                .map(|(a, b)| a.abs_diff(*b) as u32)
                .sum::<u32>(),
        );
        orthogonal += step;
    }
    if differences.is_empty() {
        return 0.0;
    }

    differences.sort_unstable();

    f64::from(differences[differences.len() / 4])
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;
    use crate::{OutputId, OutputTransform, Size};

    fn output(width: f64, height: f64) -> Output {
        Output {
            id: OutputId::new("DP-1"),
            logical_geometry: Rect::new(100.0, 200.0, width, height),
            pixel_size: Size::new(width, height),
            scale: 1.0,
            transform: OutputTransform::Normal,
        }
    }

    fn ipc_window(
        id: u64,
        column: usize,
        tile_size: (f64, f64),
        window_size: (f64, f64),
    ) -> IpcWindow {
        IpcWindow {
            id,
            workspace_id: Some(1),
            is_floating: false,
            layout: super::super::ipc::WindowLayout {
                pos_in_scrolling_layout: Some((column, 1)),
                tile_size,
                window_size,
                tile_pos_in_workspace_view: None,
                window_offset_in_tile: (2.0, 2.0),
            },
            focus_timestamp: None,
        }
    }

    fn snapshot(windows: Vec<IpcWindow>, active_window_id: Option<u64>) -> Snapshot {
        Snapshot {
            windows,
            workspaces: vec![Workspace {
                id: 1,
                output: Some("DP-1".into()),
                is_active: true,
                active_window_id,
            }],
            overview_open: false,
        }
    }

    fn frame(image: RgbaImage, geometry: Rect) -> DesktopFrame {
        DesktopFrame {
            outputs: vec![OutputFrame {
                output: OutputId::new("DP-1"),
                logical_geometry: geometry,
                image,
            }],
        }
    }

    fn paint(image: &mut RgbaImage, rect: Rect, color: Rgba<u8>) {
        let left = rect.left().max(0.0) as u32;
        let top = rect.top().max(0.0) as u32;
        let right = rect.right().min(f64::from(image.width())) as u32;
        let bottom = rect.bottom().min(f64::from(image.height())) as u32;

        for y in top..bottom {
            for x in left..right {
                image.put_pixel(x, y, color);
            }
        }
    }

    #[test]
    fn recovers_scroll_gap_and_work_area_from_the_frozen_frame() {
        let output = output(800.0, 500.0);
        let scene = Scene {
            outputs: vec![output.clone()],
            windows: Vec::new(),
        };
        let mut image = RgbaImage::from_pixel(800, 500, Rgba([10, 10, 10, 255]));
        paint(
            &mut image,
            Rect::new(-180.0, 50.0, 500.0, 400.0),
            Rgba([180, 80, 80, 255]),
        );
        paint(
            &mut image,
            Rect::new(332.0, 50.0, 450.0, 400.0),
            Rgba([80, 180, 80, 255]),
        );
        let desktop = frame(image, output.logical_geometry);
        let snapshot = snapshot(
            vec![
                ipc_window(1, 1, (500.0, 400.0), (496.0, 396.0)),
                ipc_window(2, 2, (450.0, 400.0), (446.0, 396.0)),
            ],
            Some(2),
        );
        assert_eq!(
            reconstruct(&snapshot, &scene, &desktop),
            vec![
                Window {
                    geometry: Rect::new(100.0, 252.0, 318.0, 396.0),
                },
                Window {
                    geometry: Rect::new(434.0, 252.0, 446.0, 396.0),
                },
            ]
        );
    }

    #[test]
    fn recovers_a_partially_visible_column_from_one_edge() {
        let output = output(900.0, 500.0);
        let scene = Scene {
            outputs: vec![output.clone()],
            windows: Vec::new(),
        };
        let mut image = RgbaImage::from_pixel(900, 500, Rgba([10, 10, 10, 255]));
        paint(
            &mut image,
            Rect::new(0.0, 50.0, 450.0, 400.0),
            Rgba([180, 80, 80, 255]),
        );
        paint(
            &mut image,
            Rect::new(450.0, 50.0, 600.0, 400.0),
            Rgba([80, 180, 80, 255]),
        );
        let desktop = frame(image, output.logical_geometry);
        let snapshot = snapshot(
            vec![
                ipc_window(1, 1, (450.0, 400.0), (446.0, 396.0)),
                ipc_window(2, 2, (600.0, 400.0), (596.0, 396.0)),
            ],
            Some(2),
        );

        assert_eq!(
            reconstruct(&snapshot, &scene, &desktop),
            vec![
                Window {
                    geometry: Rect::new(102.0, 252.0, 446.0, 396.0),
                },
                Window {
                    geometry: Rect::new(552.0, 252.0, 448.0, 396.0),
                },
            ]
        );
    }

    #[test]
    fn ambiguous_pixels_do_not_create_guessed_windows() {
        let output = output(800.0, 500.0);
        let scene = Scene {
            outputs: vec![output.clone()],
            windows: Vec::new(),
        };
        let desktop = frame(
            RgbaImage::from_pixel(800, 500, Rgba([20, 20, 20, 255])),
            output.logical_geometry,
        );
        let snapshot = snapshot(
            vec![ipc_window(1, 1, (400.0, 400.0), (396.0, 396.0))],
            Some(1),
        );

        assert!(reconstruct(&snapshot, &scene, &desktop).is_empty());
    }

    #[test]
    fn window_size_is_aligned_to_physical_pixels() {
        let mut output = output(1645.7142857142858, 1028.5714285714287);
        output.scale = 1.75;
        output.pixel_size = Size::new(2880.0, 1800.0);
        let window = ipc_window(1, 1, (1645.0, 1000.0), (1630.0, 975.0));

        assert_eq!(
            window_geometry(&output, &window, 6.0, 44.285714285714285),
            Some(Window {
                geometry: Rect::new(108.0, 246.28571428571428, 2853.0 / 1.75, 1706.0 / 1.75,),
            })
        );
    }

    #[test]
    fn floating_windows_are_kept_above_tiles() {
        let output = output(800.0, 500.0);
        let scene = Scene {
            outputs: vec![output.clone()],
            windows: Vec::new(),
        };
        let image = RgbaImage::from_pixel(800, 500, Rgba([120, 120, 120, 255]));
        let desktop = frame(image, output.logical_geometry);
        let mut floating = ipc_window(2, 1, (200.0, 100.0), (196.0, 96.0));
        floating.is_floating = true;
        floating.layout.pos_in_scrolling_layout = None;
        floating.layout.tile_pos_in_workspace_view = Some((300.0, 120.0));
        floating.focus_timestamp = Some(super::super::ipc::Timestamp { secs: 1, nanos: 0 });
        let snapshot = snapshot(
            vec![ipc_window(1, 1, (800.0, 500.0), (796.0, 496.0)), floating],
            Some(2),
        );

        assert_eq!(
            reconstruct(&snapshot, &scene, &desktop)[0],
            Window {
                geometry: Rect::new(402.0, 322.0, 196.0, 96.0),
            }
        );
    }
}
