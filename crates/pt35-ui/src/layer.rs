//! The Wayland layer-shell surface that the bar, the menu and the pointer
//! overlay all draw into.
//!
//! One event loop, one shm buffer, keyboard input delivered as [`Key`], and a
//! repaint only when the application asks for one — an idle bar costs nothing.

use anyhow::{Context, Result};
use pt35_common::theme::Rgb;
use smithay_client_toolkit::reexports::calloop::{
    timer::{TimeoutAction, Timer},
    EventLoop,
};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Modifiers},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::time::Duration;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_shm, wl_surface},
    Connection, QueueHandle,
};

use crate::canvas::Canvas;
use crate::keys::Key;

/// Where a surface sits and how much space it reserves.
pub struct SurfaceSpec {
    pub namespace: &'static str,
    pub layer: Layer,
    pub anchor: Anchor,
    /// 0 means "as tall as the output" (the menu); anything else reserves that
    /// many pixels of exclusive space (the bar).
    pub height: u32,
    pub keyboard: bool,
    /// Empty input region: clicks and touches go straight through to whatever
    /// is underneath. The pointer overlay needs this — it must not swallow the
    /// clicks it is synthesising.
    pub passthrough: bool,
}

impl SurfaceSpec {
    pub fn bar(height: u32) -> Self {
        Self {
            namespace: "pt35-bar",
            layer: Layer::Top,
            anchor: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
            height,
            keyboard: false,
            passthrough: false,
        }
    }

    pub fn overlay(namespace: &'static str) -> Self {
        Self {
            namespace,
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            height: 0,
            keyboard: true,
            passthrough: false,
        }
    }

    /// A transparent, click-through overlay that still takes the keyboard:
    /// what `pt35-pointer` draws its grid on.
    pub fn passthrough_overlay(namespace: &'static str) -> Self {
        Self {
            passthrough: true,
            ..Self::overlay(namespace)
        }
    }
}

/// What a client of this module implements.
pub trait App {
    /// Paint one frame. The canvas is already sized to the surface.
    fn draw(&mut self, canvas: &mut Canvas);

    /// React to a key press. Return `false` to quit the event loop.
    fn key(&mut self, _key: Key) -> bool {
        true
    }

    /// React to a key release — needed by anything that repeats while a key is
    /// held, such as the pointer's D-pad movement.
    fn key_release(&mut self, _key: Key) {}

    /// Draw onto a fully transparent surface instead of [`App::background`].
    fn transparent(&self) -> bool {
        false
    }

    /// Called on a timer (see [`App::tick_interval`]); return true to repaint.
    /// The bar uses this to pick up status updates from pt35d.
    fn tick(&mut self) -> bool {
        false
    }

    /// How often [`App::tick`] runs. `None` (the default) means the surface only
    /// wakes for Wayland events — which is what the menu and the pointer want.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    /// Background colour used before the first draw.
    fn background(&self) -> Rgb {
        Rgb(0x10, 0x14, 0x18)
    }
}

struct State<A: App + 'static> {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    modifiers: Modifiers,
    width: u32,
    height: u32,
    configured: bool,
    dirty: bool,
    running: bool,
    app: A,
}

/// Run `app` on its own layer surface until it asks to stop.
pub fn run<A: App + 'static>(app: A, spec: SurfaceSpec) -> Result<()> {
    let conn = Connection::connect_to_env().context("connecting to the Wayland display")?;
    let (globals, queue) = registry_queue_init(&conn)?;
    let qh: QueueHandle<State<A>> = queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).context("wl_compositor")?;
    let layer_shell = LayerShell::bind(&globals, &qh).context(
        "this compositor has no wlr-layer-shell; pt35 needs sway or another wlroots compositor",
    )?;
    let shm = Shm::bind(&globals, &qh).context("wl_shm")?;

    let surface = compositor.create_surface(&qh);
    if spec.passthrough {
        // An empty input region: the overlay is visible but not clickable, so
        // the clicks pt35-pointer synthesises land on the app underneath.
        let region = Region::new(&compositor).context("creating an empty input region")?;
        surface.set_input_region(Some(region.wl_region()));
    }
    let layer =
        layer_shell.create_layer_surface(&qh, surface, spec.layer, Some(spec.namespace), None);
    layer.set_anchor(spec.anchor);
    layer.set_keyboard_interactivity(if spec.keyboard {
        KeyboardInteractivity::Exclusive
    } else {
        KeyboardInteractivity::None
    });
    if spec.height > 0 {
        layer.set_size(0, spec.height);
        layer.set_exclusive_zone(spec.height as i32);
    } else {
        layer.set_size(0, 0);
    }
    layer.commit();

    let pool = SlotPool::new(640 * 480 * 4, &shm).context("allocating the shm pool")?;

    let mut state = State {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        shm,
        pool,
        layer,
        keyboard: None,
        modifiers: Modifiers::default(),
        width: 640,
        height: spec.height.max(1),
        configured: false,
        dirty: true,
        running: true,
        app,
    };

    // calloop lets one loop wait on both the Wayland socket and a timer, so an
    // idle surface costs nothing while the bar can still tick its clock.
    let mut event_loop: EventLoop<State<A>> = EventLoop::try_new()?;
    WaylandSource::new(conn.clone(), queue)
        .insert(event_loop.handle())
        .map_err(|e| anyhow::anyhow!("inserting the wayland source: {e}"))?;

    if let Some(interval) = state.app.tick_interval() {
        event_loop
            .handle()
            .insert_source(
                Timer::from_duration(interval),
                move |_, _, state: &mut State<A>| {
                    if state.app.tick() {
                        state.dirty = true;
                    }
                    TimeoutAction::ToDuration(interval)
                },
            )
            .map_err(|e| anyhow::anyhow!("inserting the tick timer: {e}"))?;
    }

    while state.running {
        event_loop.dispatch(Duration::from_millis(500), &mut state)?;
        if state.configured && state.dirty {
            state.draw(&qh);
        }
    }
    Ok(())
}

impl<A: App + 'static> State<A> {
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        let (width, height) = (self.width, self.height);
        let stride = width as i32 * 4;
        let (buffer, slot) = match self.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(pair) => pair,
            Err(e) => {
                log::error!("shm buffer: {e}");
                return;
            }
        };

        let mut canvas = Canvas::new(width, height);
        if self.app.transparent() {
            canvas.clear_transparent();
        } else {
            canvas.fill(self.app.background());
        }
        self.app.draw(&mut canvas);

        // SlotPool hands back the whole slot, which may be larger than this
        // buffer needs — it reuses any slot big enough, and the surface shrinks
        // whenever the output scale changes. Copy only what we drew.
        let pixels = canvas.as_bytes();
        match slot.len().cmp(&pixels.len()) {
            std::cmp::Ordering::Less => {
                log::error!("shm slot too small: {} < {}", slot.len(), pixels.len());
                return;
            }
            _ => slot[..pixels.len()].copy_from_slice(pixels),
        }

        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, width as i32, height as i32);
        surface.frame(qh, surface.clone());
        if let Err(e) = buffer.attach_to(surface) {
            log::error!("attaching buffer: {e}");
            return;
        }
        surface.commit();
        self.dirty = false;
    }
}

impl<A: App + 'static> CompositorHandler for State<A> {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl<A: App + 'static> LayerShellHandler for State<A> {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.running = false;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let (width, height) = configure.new_size;
        if width > 0 {
            self.width = width;
        }
        if height > 0 {
            self.height = height;
        }
        self.configured = true;
        self.dirty = true;
    }
}

impl<A: App + 'static> SeatHandler for State<A> {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }

    fn new_seat(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wayland_client::protocol::wl_seat::WlSeat,
    ) {
    }

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wayland_client::protocol::wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            match self.seats.get_keyboard(qh, &seat, None) {
                Ok(keyboard) => self.keyboard = Some(keyboard),
                Err(e) => log::warn!("no keyboard on this seat: {e}"),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wayland_client::protocol::wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
        }
    }

    fn remove_seat(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wayland_client::protocol::wl_seat::WlSeat,
    ) {
    }
}

impl<A: App + 'static> KeyboardHandler for State<A> {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[smithay_client_toolkit::seat::keyboard::Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        let key = Key {
            sym: event.keysym.raw(),
            text: event.utf8.as_deref().and_then(|s| s.chars().next()),
            ctrl: self.modifiers.ctrl,
            shift: self.modifiers.shift,
        };
        if !self.app.key(key) {
            self.running = false;
        }
        self.dirty = true;
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.app.key_release(Key {
            sym: event.keysym.raw(),
            text: event.utf8.as_deref().and_then(|s| s.chars().next()),
            ctrl: self.modifiers.ctrl,
            shift: self.modifiers.shift,
        });
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}

impl<A: App + 'static> OutputHandler for State<A> {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl<A: App + 'static> ShmHandler for State<A> {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl<A: App + 'static> ProvidesRegistryState for State<A> {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(@<A: App + 'static> State<A>);
delegate_output!(@<A: App + 'static> State<A>);
delegate_shm!(@<A: App + 'static> State<A>);
delegate_seat!(@<A: App + 'static> State<A>);
delegate_keyboard!(@<A: App + 'static> State<A>);
delegate_layer!(@<A: App + 'static> State<A>);
delegate_registry!(@<A: App + 'static> State<A>);
