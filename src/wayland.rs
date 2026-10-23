//! Wayland input-idle and software layer-shell adapter. No global key capture.
use crate::artwork;
use anyhow::{Context, Result, ensure};
use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    time::{ClockId, clock_gettime},
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_seat,
    delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, delegate_noop,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::ExtIdleNotifierV1,
};

pub(crate) fn boot_time() -> std::time::Duration {
    let time = clock_gettime(ClockId::Boottime);
    std::time::Duration::new(time.tv_sec as u64, time.tv_nsec as u32)
}

pub fn seconds() -> u64 {
    boot_time().as_secs()
}

pub struct Wayland {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
}

struct Seat {
    seat: wl_seat::WlSeat,
    notification: ExtIdleNotificationV1,
    idle_since: Option<u64>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
}

struct Card {
    output: wl_output::WlOutput,
    layer: LayerSurface,
    width: u32,
    height: u32,
    scale: i32,
    configured: bool,
}

struct State {
    registry: RegistryState,
    seats: SeatState,
    outputs: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    notifier: ExtIdleNotifierV1,
    shm: Shm,
    pool: SlotPool,
    inputs: Vec<Seat>,
    cards: Vec<Card>,
    closed_outputs: Vec<(wl_output::WlOutput, std::time::Instant)>,
    overlay: bool,
    remaining: u64,
    error: Option<String>,
    resumed: bool,
    idle_floor: u64,
}

impl Wayland {
    /// No surfaces are created for a monitor-only connection.
    pub fn connect(overlay: bool, remaining: u64) -> Result<Self> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = tx.send(Self::connect_inner(overlay, remaining));
        });
        // A stalled compositor must not trap the guardian in registry/initial roundtrips.
        // On timeout the command exits; process teardown closes any worker-owned surfaces.
        rx.recv_timeout(std::time::Duration::from_secs(2))
            .context("Wayland initialization timed out; stopping safely")?
    }

    fn connect_inner(overlay: bool, remaining: u64) -> Result<Self> {
        let connection = Connection::connect_to_env().context("Connecting to Wayland")?;
        let (globals, mut queue) = registry_queue_init(&connection)?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh)?;
        let layer_shell = LayerShell::bind(&globals, &qh).context("wlr-layer-shell is required")?;
        let notifier: ExtIdleNotifierV1 = globals
            .bind(&qh, 2..=2, ())
            .context("ext-idle-notify v2 input-idle (ignoring inhibitors) is required")?;
        let shm = Shm::bind(&globals, &qh)?;
        let pool = SlotPool::new(4, &shm)?;
        let mut state = State {
            registry: RegistryState::new(&globals),
            seats: SeatState::new(&globals, &qh),
            outputs: OutputState::new(&globals, &qh),
            compositor,
            layer_shell,
            notifier,
            shm,
            pool,
            inputs: Vec::new(),
            cards: Vec::new(),
            closed_outputs: Vec::new(),
            overlay,
            remaining,
            error: None,
            resumed: false,
            idle_floor: seconds().saturating_add(1),
        };
        for seat in state.seats.seats().collect::<Vec<_>>() {
            state.new_seat(&connection, &qh, seat);
        }
        queue.roundtrip(&mut state)?;
        ensure!(!state.inputs.is_empty(), "No Wayland input seat advertised");
        Ok(Self {
            connection,
            queue,
            state,
        })
    }

    /// Nonblocking dispatch allows the guardian's independent heartbeat lease to run.
    pub fn dispatch(&mut self) -> Result<()> {
        self.queue.dispatch_pending(&mut self.state)?;
        self.connection.flush()?;
        if let Some(guard) = self.queue.prepare_read() {
            let fd = guard.connection_fd();
            let mut fds = [PollFd::new(&fd, PollFlags::IN)];
            poll(
                &mut fds,
                Some(&Timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                }),
            )?;
            if !fds[0].revents().is_empty() {
                guard.read()?;
            }
        }
        self.queue.dispatch_pending(&mut self.state)?;
        self.state
            .closed_outputs
            .retain(|(output, _)| self.state.outputs.info(output).is_some());
        ensure!(
            !self
                .state
                .closed_outputs
                .iter()
                .any(|(_, closed)| closed.elapsed() > std::time::Duration::from_millis(500)),
            "Compositor closed a test card on a connected output; stopping safely"
        );
        if let Some(error) = self.state.error.take() {
            anyhow::bail!(error);
        }
        Ok(())
    }

    /// All advertised seats must be idle; activity on ANY seat interrupts the break.
    pub fn activity(&mut self) -> (bool, Option<u64>) {
        let resumed = std::mem::take(&mut self.state.resumed);
        let since = aggregate_idle(
            self.state.idle_floor,
            self.state.inputs.iter().map(|seat| seat.idle_since),
        );
        (resumed, since)
    }

    pub fn countdown(&mut self, remaining: u64) -> Result<()> {
        if self.state.remaining != remaining {
            self.state.remaining = remaining;
            for index in 0..self.state.cards.len() {
                self.state.draw(index)?;
            }
        }
        Ok(())
    }
}

impl State {
    fn add_output(&mut self, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if !self.overlay || self.cards.iter().any(|card| card.output == output) {
            return;
        }
        self.closed_outputs.retain(|(closed, _)| closed != &output);
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("signal-loss"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, 0);
        layer.set_exclusive_zone(-1); // Cover reserved panels too, without changing work area.
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        // Default input region is the complete surface: pointer events terminate here.
        layer.commit();
        self.cards.push(Card {
            output,
            layer,
            width: 0,
            height: 0,
            scale: 1,
            configured: false,
        });
    }

    fn draw(&mut self, index: usize) -> Result<()> {
        let card = &self.cards[index];
        if !card.configured {
            return Ok(());
        }
        let width = card
            .width
            .checked_mul(card.scale as u32)
            .context("Invalid card width")?;
        let height = card
            .height
            .checked_mul(card.scale as u32)
            .context("Invalid card height")?;
        ensure!(
            width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 33_554_432,
            "Unsupported output dimensions {width}x{height}"
        );
        let image = artwork::test_card(width as usize, height as usize, self.remaining);
        let (buffer, canvas) = self.pool.create_buffer(
            width as i32,
            height as i32,
            width as i32 * 4,
            wl_shm::Format::Argb8888,
        )?;
        for (pixel, rgba) in canvas.chunks_exact_mut(4).zip(image.rgba.chunks_exact(4)) {
            let colour = (255u32 << 24)
                | ((rgba[0] as u32) << 16)
                | ((rgba[1] as u32) << 8)
                | rgba[2] as u32;
            pixel.copy_from_slice(&colour.to_ne_bytes());
        }
        card.layer.wl_surface().set_buffer_scale(card.scale);
        card.layer
            .wl_surface()
            .damage_buffer(0, 0, width as i32, height as i32);
        buffer.attach_to(card.layer.wl_surface())?;
        card.layer.commit();
        // No frame callback or continuous rendering: redraw only configure/scale/countdown.
        Ok(())
    }
}

impl Dispatch<ExtIdleNotifierV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ExtIdleNotifierV1,
        _: <ExtIdleNotifierV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotificationV1, wl_seat::WlSeat> for State {
    fn event(
        state: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        seat: &wl_seat::WlSeat,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(input) = state.inputs.iter_mut().find(|input| &input.seat == seat) else {
            return;
        };
        match event {
            ext_idle_notification_v1::Event::Idled => {
                // One-second observation granularity: round conservatively, never
                // complete a break early because a timestamp was truncated.
                input.idle_since = Some(seconds())
            }
            ext_idle_notification_v1::Event::Resumed => {
                input.idle_since = None;
                state.idle_floor = seconds().saturating_add(1);
                state.resumed = true;
            }
            _ => (),
        }
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.idle_floor = seconds().saturating_add(1);
        self.resumed = true;
        let notification = self
            .notifier
            .get_input_idle_notification(1000, &seat, qh, seat.clone());
        // Start counting observation at connection time, never guess pre-launch idle time.
        self.inputs.push(Seat {
            seat,
            notification,
            idle_since: None,
            keyboard: None,
            pointer: None,
        });
    }
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if !self.overlay {
            return;
        }
        if let Some(input) = self.inputs.iter_mut().find(|input| input.seat == seat) {
            match capability {
                Capability::Keyboard if input.keyboard.is_none() => {
                    input.keyboard = Some(seat.get_keyboard(qh, ()))
                }
                Capability::Pointer if input.pointer.is_none() => {
                    input.pointer = Some(seat.get_pointer(qh, ()))
                }
                _ => (),
            }
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if let Some(input) = self.inputs.iter_mut().find(|input| input.seat == seat) {
            match capability {
                Capability::Keyboard => {
                    if let Some(keyboard) = input.keyboard.take() {
                        keyboard.release();
                    }
                }
                Capability::Pointer => {
                    if let Some(pointer) = input.pointer.take() {
                        pointer.release();
                    }
                }
                _ => (),
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        if let Some(index) = self.inputs.iter().position(|input| input.seat == seat) {
            let input = self.inputs.remove(index);
            input.notification.destroy();
            if let Some(keyboard) = input.keyboard {
                keyboard.release();
            }
            if let Some(pointer) = input.pointer {
                pointer.release();
            }
        }
        self.idle_floor = seconds().saturating_add(1);
        self.resumed = true;
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.add_output(qh, output);
    }
    fn update_output(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.add_output(qh, output);
    }
    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.cards.retain(|card| card.output != output);
        self.closed_outputs.retain(|(closed, _)| closed != &output);
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some(index) = self.cards.iter().position(|card| &card.layer == layer) {
            let card = self.cards.remove(index);
            // Hyprland closes the layer before announcing monitor removal. Allow
            // the registry removal to arrive instead of tearing down other monitors.
            self.closed_outputs
                .push((card.output, std::time::Instant::now()));
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(index) = self.cards.iter().position(|card| &card.layer == layer) {
            self.cards[index].width = configure.new_size.0;
            self.cards[index].height = configure.new_size.1;
            self.cards[index].configured = true;
            if let Err(error) = self.draw(index) {
                self.error = Some(format!("Drawing test card: {error:#}"));
            }
        }
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if let Some(index) = self
            .cards
            .iter()
            .position(|card| card.layer.wl_surface() == surface)
        {
            self.cards[index].scale = factor.max(1);
            if let Err(error) = self.draw(index) {
                self.error = Some(format!("Scaling test card: {error:#}"));
            }
        }
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

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}
impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_noop!(State: ignore wl_keyboard::WlKeyboard);
delegate_noop!(State: ignore wl_pointer::WlPointer);
delegate_compositor!(State);
delegate_output!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_layer!(State);
delegate_registry!(State);

fn aggregate_idle(floor: u64, starts: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    // Empty seats mean no input is possible, not "idle since boot". A new or
    // removed seat also bounds history: old idle seats cannot erase recent debt.
    let mut since = floor;
    for start in starts {
        since = since.max(start?);
    }
    Some(since)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seat_changes_never_backdate_inactivity_to_boot_or_an_older_seat() {
        assert_eq!(aggregate_idle(100, [].into_iter()), Some(100));
        assert_eq!(aggregate_idle(100, [Some(5)].into_iter()), Some(100));
        assert_eq!(aggregate_idle(100, [Some(5), None].into_iter()), None);
        assert_eq!(
            aggregate_idle(100, [Some(110), Some(120)].into_iter()),
            Some(120)
        );
    }
}
