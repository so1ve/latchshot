use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState, SimpleGlobal};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
};
use smithay_client_toolkit::seat::pointer::{
    BTN_LEFT, BTN_RIGHT, CursorIcon, PointerEvent, PointerEventKind, PointerHandler, ThemeSpec,
    ThemedPointer,
};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::subcompositor::SubcompositorState;
use smithay_client_toolkit::{delegate_registry, registry_handlers};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::viewporter::client::wp_viewport::{self, WpViewport};
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;

use super::highlight::{Highlight, PendingReveal};
use super::surfaces::{OutputOverlay, SurfaceContext};
use crate::{DesktopFrame, Rect, Scene, SelectionResult, Selector};

const DRAG_THRESHOLD: f64 = 4.0;

/// Runs one selection session and returns the selection together with the
/// frozen desktop frame it refers to.
pub fn select(
    scene: Scene,
    frame: DesktopFrame,
    animations: bool,
) -> Result<(SelectionResult, DesktopFrame)> {
    let connection = Connection::connect_to_env().context("failed to connect to Wayland")?;
    let (globals, mut event_queue) =
        registry_queue_init(&connection).context("failed to read Wayland globals")?;
    let qh = event_queue.handle();

    let compositor =
        CompositorState::bind(&globals, &qh).context("wl_compositor is not available")?;
    let subcompositor = SubcompositorState::bind(compositor.wl_compositor().clone(), &globals, &qh)
        .context("wl_subcompositor is not available")?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).context("wlr-layer-shell is not available")?;
    let shm = Shm::bind(&globals, &qh).context("wl_shm is not available")?;
    let viewporter = SimpleGlobal::<WpViewporter, 1>::bind(&globals, &qh)
        .context("wp_viewporter is not available")?;

    let mut state = State {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        compositor: compositor.clone(),
        shm,
        viewporter,
        selector: Selector::new(scene, DRAG_THRESHOLD),
        frame,
        outputs: Vec::new(),
        keyboard: None,
        pointer: None,
        highlight: Highlight::new(animations),
        result: None,
        failure: None,
    };

    event_queue
        .roundtrip(&mut state)
        .context("failed to read Wayland output information")?;
    state.create_outputs(&compositor, &subcompositor, &layer_shell, &qh)?;

    loop {
        event_queue
            .blocking_dispatch(&mut state)
            .context("Wayland event dispatch failed")?;

        if let Some(error) = state.failure.take() {
            return Err(error);
        }
        if let Some(result) = state.result.take() {
            return Ok((result, state.frame));
        }

        let plan = state.frame_plan(Instant::now());
        for output in &mut state.outputs {
            output.present(&plan, &qh);
        }
    }
}

pub(super) struct State {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    compositor: CompositorState,
    shm: Shm,
    viewporter: SimpleGlobal<WpViewporter, 1>,
    selector: Selector,
    frame: DesktopFrame,
    outputs: Vec<OutputOverlay>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<ThemedPointer>,
    highlight: Highlight,
    result: Option<SelectionResult>,
    failure: Option<anyhow::Error>,
}

impl State {
    fn create_outputs(
        &mut self,
        compositor: &CompositorState,
        subcompositor: &SubcompositorState,
        layer_shell: &LayerShell,
        qh: &QueueHandle<Self>,
    ) -> Result<()> {
        let outputs = &self.selector.scene().outputs;
        let context = SurfaceContext {
            compositor,
            subcompositor,
            viewporter: self.viewporter.get().unwrap(),
            qh,
        };
        let mut overlays = Vec::with_capacity(outputs.len());

        for output in outputs {
            let frame = self
                .frame
                .outputs
                .iter()
                .find(|frame| frame.output == output.id)
                .unwrap();
            let wl_output = self.wl_output_named(output.id.as_str())?;
            overlays.push(OutputOverlay::new(
                frame,
                wl_output,
                layer_shell,
                &self.shm,
                &context,
            ));
        }
        self.outputs = overlays;

        Ok(())
    }

    fn wl_output_named(&self, name: &str) -> Result<wl_output::WlOutput> {
        self.output_state
            .outputs()
            .find(|output| {
                self.output_state
                    .info(output)
                    .and_then(|info| info.name)
                    .as_deref()
                    == Some(name)
            })
            .with_context(|| format!("Wayland output {name} is not available"))
    }

    fn frame_plan(&mut self, now: Instant) -> FramePlan {
        if let Some(region) = self.selector.region() {
            self.highlight.clear();

            return FramePlan {
                selection: Some(region),
                reveal: 1.0,
                animating: false,
                pending_reveal: None,
            };
        }

        self.highlight
            .set_target(self.selector.target_geometry(), now);
        let pending_reveal = self.acknowledge_reveal(now);
        let (selection, reveal, animating) = self.highlight.sample(now);

        FramePlan {
            selection,
            reveal,
            animating,
            pending_reveal,
        }
    }

    fn acknowledge_reveal(&mut self, now: Instant) -> Option<PendingReveal> {
        let reveal = self.highlight.pending_reveal()?;
        let mut pending = false;

        for output in &self.outputs {
            pending |= output.needs_reveal_frame(reveal)?;
        }
        if pending {
            Some(reveal)
        } else {
            self.highlight.start_reveal(now);

            None
        }
    }

    fn update_pointer(&mut self, event: &PointerEvent) {
        let before = (self.selector.target_geometry(), self.selector.region());
        let position = self
            .output_for_surface(&event.surface)
            .point_at(event.position);
        self.selector.pointer_moved(position);
        let after = (self.selector.target_geometry(), self.selector.region());

        if before != after {
            self.mark_all_dirty();
        }
    }

    fn mark_all_dirty(&mut self) {
        for output in &mut self.outputs {
            output.mark_dirty();
        }
    }

    fn output_for_surface(&mut self, surface: &wl_surface::WlSurface) -> &mut OutputOverlay {
        self.outputs
            .iter_mut()
            .find(|output| output.matches_surface(surface))
            .unwrap()
    }
}

#[derive(Clone, Copy)]
pub(super) struct FramePlan {
    pub(super) selection: Option<Rect>,
    pub(super) reveal: f32,
    pub(super) animating: bool,
    pub(super) pending_reveal: Option<PendingReveal>,
}

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        // Defer acknowledgement until this dispatch has updated the target.
        self.output_for_surface(surface).frame_done();
    }

    fn surface_enter(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _connection: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        let id = self.output_for_surface(layer.wl_surface()).id().clone();
        self.failure = Some(anyhow!("compositor closed the overlay for {id}"));
    }

    fn configure(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        self.output_for_surface(layer.wl_surface())
            .configure(configure.new_size);
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
    ) {
    }

    fn new_capability(
        &mut self,
        _connection: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = Some(self.seat_state.get_keyboard(qh, &seat, None).unwrap());
        }

        if capability == Capability::Pointer && self.pointer.is_none() {
            let surface = self.compositor.create_surface(qh);
            self.pointer = Some(
                self.seat_state
                    .get_pointer_with_theme::<_, ()>(
                        qh,
                        &seat,
                        self.shm.wl_shm(),
                        surface,
                        ThemeSpec::default(),
                    )
                    .unwrap(),
            );
        }
    }

    fn remove_capability(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard
            && let Some(keyboard) = self.keyboard.take()
        {
            keyboard.release();
        }

        if capability == Capability::Pointer {
            self.pointer = None;
        }
    }

    fn remove_seat(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
    ) {
    }
}

impl KeyboardHandler for State {
    fn enter(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if event.keysym == Keysym::Escape {
            self.result = Some(SelectionResult::Cancelled);
        }
    }

    fn repeat_key(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: KeyEvent,
    ) {
    }

    fn release_key(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: Modifiers,
        _raw_modifiers: RawModifiers,
        _layout: u32,
    ) {
    }
}

impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        connection: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match event.kind {
                PointerEventKind::Enter { .. } => {
                    self.pointer
                        .as_ref()
                        .unwrap()
                        .set_cursor(connection, CursorIcon::Crosshair)
                        .unwrap();
                    self.update_pointer(event);
                }
                PointerEventKind::Motion { .. } => {
                    self.update_pointer(event);
                }
                PointerEventKind::Press {
                    button: BTN_LEFT, ..
                } => {
                    self.update_pointer(event);
                    self.selector.press();

                    if self.selector.target_geometry().is_none() {
                        self.highlight.clear();
                        self.mark_all_dirty();
                    }
                }
                PointerEventKind::Press {
                    button: BTN_RIGHT, ..
                } => {
                    self.result = Some(SelectionResult::Cancelled);
                }
                PointerEventKind::Release {
                    button: BTN_LEFT, ..
                } => {
                    self.update_pointer(event);
                    let before = (self.selector.target_geometry(), self.selector.region());
                    self.result = self.selector.release();
                    let after = (self.selector.target_geometry(), self.selector.region());

                    if before != after {
                        self.mark_all_dirty();
                    }
                }
                PointerEventKind::Leave { .. }
                | PointerEventKind::Axis { .. }
                | PointerEventKind::Press { .. }
                | PointerEventKind::Release { .. } => {}
            }

            if self.result.is_some() {
                break;
            }
        }
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if let Some(id) = self
            .outputs
            .iter()
            .find(|overlay| overlay.matches_output(&output))
            .map(|overlay| overlay.id().clone())
        {
            self.failure = Some(anyhow!("output {id} disappeared during selection"));
        }
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

wayland_client::delegate_noop!(State: WpViewporter);

impl Dispatch<WpViewport, ()> for State {
    fn event(
        _state: &mut Self,
        _viewport: &WpViewport,
        _event: wp_viewport::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        unreachable!("wp_viewport version 1 has no events");
    }
}

impl AsMut<SimpleGlobal<WpViewporter, 1>> for State {
    fn as_mut(&mut self) -> &mut SimpleGlobal<WpViewporter, 1> {
        &mut self.viewporter
    }
}

delegate_registry!(State);

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(State);
