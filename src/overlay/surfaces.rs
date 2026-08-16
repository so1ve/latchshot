use smithay_client_toolkit::compositor::{CompositorState, FrameCallbackData, Region};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface,
};
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::subcompositor::SubcompositorState;
use wayland_client::protocol::{wl_output, wl_shm, wl_subsurface, wl_surface};
use wayland_client::{Proxy, QueueHandle};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;

use super::highlight::PendingReveal;
use super::render::{
    BORDER_WIDTH, CORNER_RADIUS, Corner, CornerMask, DIM_FACTOR, border_pixel_with_alpha,
    copy_frame, multiply_channel,
};
use super::session::{FramePlan, State};
use crate::capture::OutputFrame;
use crate::{OutputId, Point, Rect};

const CORNERS: [Corner; 4] = [
    Corner::TopLeft,
    Corner::TopRight,
    Corner::BottomLeft,
    Corner::BottomRight,
];

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
    highlight_subsurface: wl_subsurface::WlSubsurface,
    highlight_surface: wl_surface::WlSurface,
    highlight_viewport: WpViewport,
    veil: SolidSurface,
    borders: [SolidSurface; 4],
    corners: [CornerSurface; 4],
    pool: SlotPool,
    background: Buffer,
    highlight_buffer: Buffer,
    width: u32,
    height: u32,
    configured_size: Option<(u32, u32)>,
    pending_frame: Option<PendingFrame>,
    reveal_acknowledged: Option<u64>,
    content_state: ContentState,
    dirty: bool,
}

struct PendingFrame {
    continue_animation: bool,
    reveal: Option<PendingReveal>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContentState {
    Empty,
    HighlightHidden,
    HighlightVisible,
}

fn child_surface(
    parent: &wl_surface::WlSurface,
    context: &SurfaceContext<'_>,
) -> (wl_subsurface::WlSubsurface, wl_surface::WlSurface) {
    let (subsurface, surface) = context
        .subcompositor
        .create_subsurface(parent.clone(), context.qh);
    let empty_region = Region::new(context.compositor).unwrap();
    surface.set_input_region(Some(empty_region.wl_region()));
    surface.commit();

    (subsurface, surface)
}

fn damage(surface: &wl_surface::WlSurface, width: i32, height: i32) {
    if surface.version() >= 4 {
        surface.damage_buffer(0, 0, width, height);
    } else {
        surface.damage(0, 0, i32::MAX, i32::MAX);
    }
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

/// A subsurface that can redraw while the compositor holds its other buffer.
struct ChildSurface {
    subsurface: wl_subsurface::WlSubsurface,
    surface: wl_surface::WlSurface,
    viewport: WpViewport,
    buffers: [Buffer; 2],
    width: i32,
    height: i32,
    visible: bool,
}

impl ChildSurface {
    fn new(
        parent: &wl_surface::WlSurface,
        pool: &mut SlotPool,
        size: (i32, i32),
        context: &SurfaceContext<'_>,
    ) -> Self {
        let (subsurface, surface) = child_surface(parent, context);
        let viewport = context.viewporter.get_viewport(&surface, context.qh, ());
        let stride = size.0 * 4;
        let first = pool
            .create_buffer(size.0, size.1, stride, wl_shm::Format::Argb8888)
            .unwrap()
            .0;
        let second = pool
            .create_buffer(size.0, size.1, stride, wl_shm::Format::Argb8888)
            .unwrap()
            .0;

        Self {
            subsurface,
            surface,
            viewport,
            buffers: [first, second],
            width: size.0,
            height: size.1,
            visible: false,
        }
    }

    fn layout(&mut self, position: (i32, i32), destination: (i32, i32)) {
        self.viewport.set_destination(destination.0, destination.1);
        self.subsurface.set_position(position.0, position.1);
    }

    /// Repaints after [`Self::ready`] confirms that a buffer is available.
    fn redraw(&mut self, pool: &mut SlotPool, fill: impl FnOnce(&mut [u8])) {
        let buffer = self
            .buffers
            .iter()
            .find(|buffer| buffer.canvas(pool).is_some())
            .unwrap();
        fill(buffer.canvas(pool).unwrap());
        buffer.attach_to(&self.surface).unwrap();
        damage(&self.surface, self.width, self.height);
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

impl Drop for ChildSurface {
    fn drop(&mut self) {
        self.viewport.destroy();
    }
}

struct SolidSurface {
    child: ChildSurface,
    pixel: Option<[u8; 4]>,
}

impl SolidSurface {
    fn new(
        parent: &wl_surface::WlSurface,
        pool: &mut SlotPool,
        context: &SurfaceContext<'_>,
    ) -> Self {
        let child = ChildSurface::new(parent, pool, (1, 1), context);
        child.viewport.set_source(0.0, 0.0, 1.0, 1.0);

        Self { child, pixel: None }
    }

    fn show(
        &mut self,
        pool: &mut SlotPool,
        position: (i32, i32),
        size: (i32, i32),
        pixel: [u8; 4],
    ) {
        if size.0 <= 0 || size.1 <= 0 {
            self.hide();

            return;
        }

        self.child.layout(position, size);
        if self.child.visible && self.pixel == Some(pixel) {
            self.child.surface.commit();

            return;
        }

        self.child
            .redraw(pool, |canvas| canvas.copy_from_slice(&pixel));
        self.pixel = Some(pixel);
    }

    fn ready(&self, pool: &mut SlotPool, size: (i32, i32), pixel: [u8; 4]) -> bool {
        if size.0 <= 0 || size.1 <= 0 {
            return true;
        }

        (self.child.visible && self.pixel == Some(pixel)) || self.child.ready(pool)
    }

    fn hide(&mut self) {
        self.child.hide();
    }
}

struct CornerSurface {
    child: ChildSurface,
    mask: CornerMask,
    opacity: Option<u8>,
}

impl CornerSurface {
    fn new(
        parent: &wl_surface::WlSurface,
        pool: &mut SlotPool,
        mask: CornerMask,
        context: &SurfaceContext<'_>,
    ) -> Self {
        let size = mask.size() as i32;
        let child = ChildSurface::new(parent, pool, (size, size), context);
        child
            .viewport
            .set_source(0.0, 0.0, f64::from(size), f64::from(size));

        Self {
            child,
            mask,
            opacity: None,
        }
    }

    fn show(&mut self, pool: &mut SlotPool, position: (i32, i32), size: i32, opacity: u8) {
        if size <= 0 {
            self.hide();

            return;
        }

        self.child.layout(position, (size, size));
        if self.child.visible && self.opacity == Some(opacity) {
            self.child.surface.commit();

            return;
        }

        let mask = &self.mask;
        self.child.redraw(pool, |canvas| {
            mask.render(opacity, canvas);
        });
        self.opacity = Some(opacity);
    }

    fn ready(&self, pool: &mut SlotPool, size: i32, opacity: u8) -> bool {
        if size <= 0 {
            return true;
        }

        (self.child.visible && self.opacity == Some(opacity)) || self.child.ready(pool)
    }

    fn hide(&mut self) {
        self.child.hide();
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
        let mut pool = SlotPool::new(buffer_size * 2 + 4096, shm).unwrap();
        let (background, canvas) = pool
            .create_buffer(
                width as i32,
                height as i32,
                stride,
                wl_shm::Format::Argb8888,
            )
            .unwrap();
        copy_frame(frame, canvas, |channel| {
            multiply_channel(channel, DIM_FACTOR)
        });
        let (highlight, canvas) = pool
            .create_buffer(
                width as i32,
                height as i32,
                stride,
                wl_shm::Format::Argb8888,
            )
            .unwrap();
        copy_frame(frame, canvas, std::convert::identity);

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

        let (highlight_subsurface, highlight_surface) = child_surface(layer.wl_surface(), context);
        let highlight_viewport =
            context
                .viewporter
                .get_viewport(&highlight_surface, context.qh, ());

        let veil = SolidSurface::new(layer.wl_surface(), &mut pool, context);
        let borders =
            std::array::from_fn(|_| SolidSurface::new(layer.wl_surface(), &mut pool, context));
        let scale = frame.scale();
        let corners = std::array::from_fn(|index| {
            CornerSurface::new(
                layer.wl_surface(),
                &mut pool,
                CornerMask::new(scale, CORNERS[index]),
                context,
            )
        });
        layer.commit();

        Self {
            id: frame.output.clone(),
            logical_geometry: frame.logical_geometry,
            wl_output,
            layer,
            viewport,
            highlight_subsurface,
            highlight_surface,
            highlight_viewport,
            veil,
            borders,
            corners,
            pool,
            background,
            highlight_buffer: highlight,
            width,
            height,
            configured_size: None,
            pending_frame: None,
            reveal_acknowledged: None,
            content_state: ContentState::Empty,
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

        self.initialize_content();

        let visible = if let Some(global_selection) = selection
            && let Some((left, top, right, bottom)) =
                projected_selection(self.logical_geometry, global_selection, configured_size)
        {
            let (configured_width, configured_height) = configured_size;
            let target_width = right - left;
            let target_height = bottom - top;
            let source_scale_x = f64::from(self.width) / f64::from(configured_width);
            let source_scale_y = f64::from(self.height) / f64::from(configured_height);
            let source = Rect::new(
                f64::from(left) * source_scale_x,
                f64::from(top) * source_scale_y,
                f64::from(target_width) * source_scale_x,
                f64::from(target_height) * source_scale_y,
            );

            let has_top = global_selection.top() >= self.logical_geometry.top();
            let has_bottom = global_selection.bottom() <= self.logical_geometry.bottom();
            let has_left = global_selection.left() >= self.logical_geometry.left();
            let has_right = global_selection.right() <= self.logical_geometry.right();
            let horizontal_border_width = (BORDER_WIDTH.round().max(1.0) as i32).min(target_height);
            let vertical_border_width = (BORDER_WIDTH.round().max(1.0) as i32).min(target_width);
            let radius = (CORNER_RADIUS.round() as i32)
                .min(target_width / 2)
                .min(target_height / 2);
            let opacity = (reveal * 255.0).round() as u8;
            let border_pixel = border_pixel_with_alpha(opacity);
            let veil_pixel = [
                0,
                0,
                0,
                (f32::from(255 - DIM_FACTOR) * (1.0 - reveal)).round() as u8,
            ];
            let corner_visibility = [
                has_top && has_left,
                has_top && has_right,
                has_bottom && has_left,
                has_bottom && has_right,
            ];
            let [top_left, top_right, bottom_left, bottom_right] = corner_visibility;
            let border_layout = [
                (
                    (left + i32::from(top_left) * radius, top),
                    (
                        target_width - i32::from(top_left) * radius - i32::from(top_right) * radius,
                        horizontal_border_width,
                    ),
                    has_top,
                ),
                (
                    (
                        left + i32::from(bottom_left) * radius,
                        bottom - horizontal_border_width,
                    ),
                    (
                        target_width
                            - i32::from(bottom_left) * radius
                            - i32::from(bottom_right) * radius,
                        horizontal_border_width,
                    ),
                    has_bottom,
                ),
                (
                    (left, top + i32::from(top_left) * radius),
                    (
                        vertical_border_width,
                        target_height
                            - i32::from(top_left) * radius
                            - i32::from(bottom_left) * radius,
                    ),
                    has_left,
                ),
                (
                    (
                        right - vertical_border_width,
                        top + i32::from(top_right) * radius,
                    ),
                    (
                        vertical_border_width,
                        target_height
                            - i32::from(top_right) * radius
                            - i32::from(bottom_right) * radius,
                    ),
                    has_right,
                ),
            ];
            let corner_layout = [
                ((left, top), top_left),
                ((right - radius, top), top_right),
                ((left, bottom - radius), bottom_left),
                ((right - radius, bottom - radius), bottom_right),
            ];

            let veil_ready =
                self.veil
                    .ready(&mut self.pool, (target_width, target_height), veil_pixel);
            let borders_ready =
                self.borders
                    .iter()
                    .zip(border_layout)
                    .all(|(border, (_, size, visible))| {
                        !visible || border.ready(&mut self.pool, size, border_pixel)
                    });
            let corners_ready =
                self.corners
                    .iter()
                    .zip(corner_layout)
                    .all(|(corner, (_, visible))| {
                        !visible || corner.ready(&mut self.pool, radius, opacity)
                    });
            if !(veil_ready && borders_ready && corners_ready) {
                return;
            }

            self.highlight_viewport.set_source(
                source.left(),
                source.top(),
                source.width(),
                source.height(),
            );
            self.highlight_viewport
                .set_destination(target_width, target_height);
            self.highlight_subsurface.set_position(left, top);
            if self.content_state == ContentState::HighlightHidden {
                self.highlight_subsurface
                    .place_above(self.layer.wl_surface());
                self.content_state = ContentState::HighlightVisible;
            }
            self.highlight_surface.commit();

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

            for (corner, (position, visible)) in self.corners.iter_mut().zip(corner_layout) {
                if visible {
                    corner.show(&mut self.pool, position, radius, opacity);
                } else {
                    corner.hide();
                }
            }

            true
        } else {
            self.hide_selection();

            false
        };

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

    fn initialize_content(&mut self) {
        if self.content_state != ContentState::Empty {
            return;
        }

        self.background.attach_to(self.layer.wl_surface()).unwrap();
        damage(
            self.layer.wl_surface(),
            self.width as i32,
            self.height as i32,
        );
        self.highlight_viewport
            .set_source(0.0, 0.0, f64::from(self.width), f64::from(self.height));
        self.highlight_viewport.set_destination(1, 1);
        self.highlight_subsurface
            .place_below(self.layer.wl_surface());
        self.highlight_buffer
            .attach_to(&self.highlight_surface)
            .unwrap();
        damage(
            &self.highlight_surface,
            self.width as i32,
            self.height as i32,
        );
        self.highlight_surface.commit();
        self.content_state = ContentState::HighlightHidden;
    }

    fn hide_selection(&mut self) {
        if self.content_state == ContentState::HighlightVisible {
            self.highlight_subsurface
                .place_below(self.layer.wl_surface());
            self.content_state = ContentState::HighlightHidden;
        }
        self.veil.hide();
        self.borders.iter_mut().for_each(SolidSurface::hide);
        self.corners.iter_mut().for_each(CornerSurface::hide);
    }
}

impl Drop for OutputOverlay {
    fn drop(&mut self) {
        self.highlight_viewport.destroy();
        self.viewport.destroy();
    }
}
