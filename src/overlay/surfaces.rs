use smithay_client_toolkit::compositor::{CompositorState, FrameCallbackData, Region};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface,
};
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::subcompositor::SubcompositorState;
use wayland_client::QueueHandle;
use wayland_client::protocol::{wl_output, wl_shm, wl_subsurface, wl_surface};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;

use super::{FramePlan, PendingReveal, State};
use crate::capture::OutputFrame;
use crate::{OutputId, Point, Rect};

const BORDER_WIDTH: i32 = 2;
const BORDER_PIXEL: [u8; 4] = [255, 239, 215, 255];
const DIM_ALPHA: u8 = 115;
const DIM_PIXEL: [u8; 4] = [0, 0, 0, DIM_ALPHA];

pub(super) struct SurfaceContext<'a> {
    pub(super) compositor: &'a CompositorState,
    pub(super) subcompositor: &'a SubcompositorState,
    pub(super) viewporter: &'a WpViewporter,
    pub(super) qh: &'a QueueHandle<State>,
}

pub(super) struct OutputOverlay {
    id: OutputId,
    logical_geometry: Rect,
    wl_output: wl_output::WlOutput,
    layer: LayerSurface,
    viewport: WpViewport,
    dim: [SolidSurface; 4],
    veil: SolidSurface,
    borders: [SolidSurface; 4],
    pool: SlotPool,
    background: Buffer,
    configured_size: Option<(u32, u32)>,
    pending_frame: Option<PendingFrame>,
    reveal_acknowledged: Option<u64>,
    dirty: bool,
}

struct PendingFrame {
    continue_animation: bool,
    reveal: Option<PendingReveal>,
}

const fn multiply_channel(channel: u8, factor: u8) -> u8 {
    ((channel as u16 * factor as u16 + 127) / 255) as u8
}

fn projected_selection(
    output: Rect,
    selection: Rect,
    configured_size: (u32, u32),
) -> Option<(i32, i32, i32, i32)> {
    let local = selection.intersection(output)?;
    let frame_left = output.left();
    let frame_top = output.top();
    let project_x = |x: f64| {
        (x / output.width() * f64::from(configured_size.0))
            .round()
            .clamp(0.0, f64::from(configured_size.0)) as i32
    };
    let project_y = |y: f64| {
        (y / output.height() * f64::from(configured_size.1))
            .round()
            .clamp(0.0, f64::from(configured_size.1)) as i32
    };
    let left = project_x(local.left() - frame_left);
    let top = project_y(local.top() - frame_top);
    let right = project_x(local.right() - frame_left);
    let bottom = project_y(local.bottom() - frame_top);

    (left < right && top < bottom).then_some((left, top, right, bottom))
}

/// A solid-color subsurface that can redraw while the compositor holds its
/// other buffer.
struct SolidSurface {
    subsurface: wl_subsurface::WlSubsurface,
    surface: wl_surface::WlSurface,
    viewport: WpViewport,
    buffers: [Buffer; 2],
    visible: bool,
}

impl SolidSurface {
    fn new(
        parent: &wl_surface::WlSurface,
        pool: &mut SlotPool,
        context: &SurfaceContext<'_>,
    ) -> Self {
        let (subsurface, surface) = context
            .subcompositor
            .create_subsurface(parent.clone(), context.qh);
        let empty_region = Region::new(context.compositor).unwrap();
        surface.set_input_region(Some(empty_region.wl_region()));
        surface.commit();

        let viewport = context.viewporter.get_viewport(&surface, context.qh, ());
        viewport.set_source(0.0, 0.0, 1.0, 1.0);
        let buffers = std::array::from_fn(|_| {
            pool.create_buffer(1, 1, 4, wl_shm::Format::Argb8888)
                .unwrap()
                .0
        });

        Self {
            subsurface,
            surface,
            viewport,
            buffers,
            visible: false,
        }
    }

    fn show(
        &mut self,
        pool: &mut SlotPool,
        position: (i32, i32),
        size: (i32, i32),
        pixel: [u8; 4],
    ) {
        self.viewport.set_destination(size.0, size.1);
        self.subsurface.set_position(position.0, position.1);
        let buffer = self
            .buffers
            .iter()
            .find(|buffer| buffer.canvas(pool).is_some())
            .unwrap();
        buffer.canvas(pool).unwrap().copy_from_slice(&pixel);
        buffer.attach_to(&self.surface).unwrap();
        self.surface.damage(0, 0, size.0, size.1);
        self.surface.commit();
        self.visible = true;
    }

    fn ready(&self, pool: &mut SlotPool) -> bool {
        self.buffers
            .iter()
            .any(|buffer| buffer.canvas(pool).is_some())
    }

    fn hide(&mut self) {
        if self.visible {
            self.surface.attach(None, 0, 0);
            self.surface.commit();
            self.visible = false;
        }
    }
}

impl Drop for SolidSurface {
    fn drop(&mut self) {
        self.viewport.destroy();
    }
}

impl OutputOverlay {
    pub(super) fn new(
        frame: &OutputFrame,
        wl_output: wl_output::WlOutput,
        layer_shell: &LayerShell,
        shm: &Shm,
        context: &SurfaceContext<'_>,
    ) -> Self {
        let width = frame.image.width();
        let height = frame.image.height();
        let stride = width as i32 * 4;
        let buffer_size = frame.image.as_raw().len();
        let mut pool = SlotPool::new(buffer_size + 4096, shm).unwrap();
        let (background, canvas) = pool
            .create_buffer(
                width as i32,
                height as i32,
                stride,
                wl_shm::Format::Argb8888,
            )
            .unwrap();
        for (target, source) in canvas
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(frame.image.pixels())
        {
            let [red, green, blue, alpha] = source.0;
            target.copy_from_slice(&[
                multiply_channel(blue, alpha),
                multiply_channel(green, alpha),
                multiply_channel(red, alpha),
                alpha,
            ]);
        }

        let surface = context.compositor.create_surface(context.qh);
        let layer = layer_shell.create_layer_surface(
            context.qh,
            surface,
            Layer::Overlay,
            Some("latchshot"),
            Some(&wl_output),
        );
        layer.set_anchor(Anchor::all());
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.set_size(0, 0);

        let viewport = context
            .viewporter
            .get_viewport(layer.wl_surface(), context.qh, ());
        viewport.set_source(0.0, 0.0, f64::from(width), f64::from(height));

        let dim =
            std::array::from_fn(|_| SolidSurface::new(layer.wl_surface(), &mut pool, context));
        let veil = SolidSurface::new(layer.wl_surface(), &mut pool, context);
        let borders =
            std::array::from_fn(|_| SolidSurface::new(layer.wl_surface(), &mut pool, context));
        layer.commit();

        Self {
            id: frame.output.clone(),
            logical_geometry: frame.logical_geometry,
            wl_output,
            layer,
            viewport,
            dim,
            veil,
            borders,
            pool,
            background,
            configured_size: None,
            pending_frame: None,
            reveal_acknowledged: None,
            dirty: true,
        }
    }

    pub(super) const fn id(&self) -> &OutputId {
        &self.id
    }

    pub(super) fn matches_surface(&self, surface: &wl_surface::WlSurface) -> bool {
        self.layer.wl_surface() == surface
    }

    pub(super) fn matches_output(&self, output: &wl_output::WlOutput) -> bool {
        self.wl_output == *output
    }

    pub(super) fn point_at(&self, position: (f64, f64)) -> Point {
        Point::new(
            self.logical_geometry.left() + position.0,
            self.logical_geometry.top() + position.1,
        )
    }

    pub(super) fn configure(&mut self, size: (u32, u32)) {
        let width = if size.0 == 0 {
            self.logical_geometry.width().round() as u32
        } else {
            size.0
        };
        let height = if size.1 == 0 {
            self.logical_geometry.height().round() as u32
        } else {
            size.1
        };
        self.viewport.set_destination(width as i32, height as i32);
        let surface = self.layer.wl_surface();
        if self.configured_size.is_none() {
            self.background.attach_to(surface).unwrap();
        }
        surface.damage(0, 0, width as i32, height as i32);
        self.configured_size = Some((width, height));
        self.mark_dirty();
    }

    /// Whether this output owes the reveal a frame, or `None` before configure.
    pub(super) fn needs_reveal_frame(&self, reveal: PendingReveal) -> Option<bool> {
        let configured_size = self.configured_size?;

        Some(
            projected_selection(self.logical_geometry, reveal.target, configured_size).is_some()
                && self.reveal_acknowledged != Some(reveal.generation),
        )
    }

    pub(super) const fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub(super) const fn frame_done(&mut self) {
        let PendingFrame {
            continue_animation,
            reveal,
        } = self.pending_frame.take().unwrap();

        if let Some(reveal) = reveal {
            self.reveal_acknowledged = Some(reveal.generation);
        }

        if continue_animation {
            self.mark_dirty();
        }
    }

    pub(super) fn present(&mut self, plan: &FramePlan, qh: &QueueHandle<State>) {
        let FramePlan {
            selection,
            reveal,
            animating: continue_animation,
            pending_reveal,
        } = *plan;
        let Some(configured_size) = self.configured_size else {
            return;
        };
        if !self.dirty || self.pending_frame.is_some() {
            return;
        }

        let projected = selection.and_then(|selection| {
            projected_selection(self.logical_geometry, selection, configured_size)
        });
        let (width, height) = (configured_size.0 as i32, configured_size.1 as i32);
        let (left, top, right, bottom) = projected.unwrap_or_default();
        // Keep the original frame fixed: cropping it into a moving viewport can
        // resample half-pixel edges on fractionally scaled outputs. Only these
        // disjoint, solid-color masks move around the selection.
        let dim_layout = [
            ((0, 0), (width, top)),
            ((0, top), (left, bottom - top)),
            ((right, top), (width - right, bottom - top)),
            ((0, bottom), (width, height - bottom)),
        ];
        if !self
            .dim
            .iter()
            .zip(dim_layout)
            .all(|(dim, (_, size))| size.0 == 0 || size.1 == 0 || dim.ready(&mut self.pool))
        {
            return;
        }

        let visible = if let Some(global_selection) = selection
            && let Some((left, top, right, bottom)) = projected
        {
            let target_width = right - left;
            let target_height = bottom - top;

            let has_top = global_selection.top() >= self.logical_geometry.top();
            let has_bottom = global_selection.bottom() <= self.logical_geometry.bottom();
            let has_left = global_selection.left() >= self.logical_geometry.left();
            let has_right = global_selection.right() <= self.logical_geometry.right();
            let horizontal_border_width = BORDER_WIDTH.min(target_height);
            let vertical_border_width = BORDER_WIDTH.min(target_width);
            let opacity = (reveal * 255.0).round() as u8;
            let border_pixel = BORDER_PIXEL.map(|channel| multiply_channel(channel, opacity));
            let veil_pixel = [
                0,
                0,
                0,
                (f32::from(DIM_ALPHA) * (1.0 - reveal)).round() as u8,
            ];
            let border_layout = [
                (
                    (left, top),
                    (target_width, horizontal_border_width),
                    has_top,
                ),
                (
                    (left, bottom - horizontal_border_width),
                    (target_width, horizontal_border_width),
                    has_bottom,
                ),
                (
                    (left, top),
                    (vertical_border_width, target_height),
                    has_left,
                ),
                (
                    (right - vertical_border_width, top),
                    (vertical_border_width, target_height),
                    has_right,
                ),
            ];

            let veil_ready = self.veil.ready(&mut self.pool);
            let borders_ready = self
                .borders
                .iter()
                .zip(border_layout)
                .all(|(border, (_, _, visible))| !visible || border.ready(&mut self.pool));
            if !(veil_ready && borders_ready) {
                return;
            }

            self.veil.show(
                &mut self.pool,
                (left, top),
                (target_width, target_height),
                veil_pixel,
            );
            for (border, (position, size, visible)) in self.borders.iter_mut().zip(border_layout) {
                if visible {
                    border.show(&mut self.pool, position, size, border_pixel);
                } else {
                    border.hide();
                }
            }

            true
        } else {
            self.veil.hide();
            self.borders.iter_mut().for_each(|s| s.hide());

            false
        };

        for (dim, (position, size)) in self.dim.iter_mut().zip(dim_layout) {
            if size.0 > 0 && size.1 > 0 {
                dim.show(&mut self.pool, position, size, DIM_PIXEL);
            } else {
                dim.hide();
            }
        }

        let surface = self.layer.wl_surface();
        surface.frame(qh, FrameCallbackData(surface.clone()));
        let pending_reveal =
            pending_reveal.filter(|reveal| visible && self.needs_reveal_frame(*reveal).unwrap());
        self.pending_frame = Some(PendingFrame {
            continue_animation,
            reveal: pending_reveal,
        });
        self.layer.commit();
        self.dirty = false;
    }
}

impl Drop for OutputOverlay {
    fn drop(&mut self) {
        self.viewport.destroy();
    }
}
