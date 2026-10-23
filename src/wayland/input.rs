//! Input-idle v2 observation. No key capture, surfaces, or media inhibitors.
use crate::clock::boot_time;
use anyhow::{Context, Result, ensure};
use smithay_client_toolkit::{
    delegate_registry, delegate_seat,
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, globals::registry_queue_init, protocol::wl_seat,
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::ExtIdleNotifierV1,
};

fn seconds() -> u64 {
    boot_time().as_secs()
}

/// Observed since connection or the most recent seat topology change.
pub struct Activity {
    pub resumed: bool,
    pub idle_since: Option<u64>,
}
pub struct InputMonitor {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
}
struct Seat {
    seat: wl_seat::WlSeat,
    notification: ExtIdleNotificationV1,
    idle_since: Option<u64>,
}
struct State {
    registry: RegistryState,
    seats: SeatState,
    notifier: ExtIdleNotifierV1,
    inputs: Vec<Seat>,
    resumed: bool,
    idle_floor: u64,
}

impl InputMonitor {
    pub fn connect() -> Result<Self> {
        super::connect(Self::connect_inner)
    }
    pub(super) fn connect_inner() -> Result<Self> {
        let connection = Connection::connect_to_env().context("Connecting to Wayland")?;
        let (globals, mut queue) = registry_queue_init(&connection)?;
        let qh = queue.handle();
        let notifier = globals
            .bind(&qh, 2..=2, ())
            .context("ext-idle-notify v2 input-idle (ignoring inhibitors) is required")?;
        let mut state = State {
            registry: RegistryState::new(&globals),
            seats: SeatState::new(&globals, &qh),
            notifier,
            inputs: Vec::new(),
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
    pub fn dispatch(&mut self) -> Result<()> {
        super::dispatch(&self.connection, &mut self.queue, &mut self.state)
    }
    /// All seats must be idle; input on any seat interrupts a break.
    pub fn activity(&mut self) -> Activity {
        Activity {
            resumed: std::mem::take(&mut self.state.resumed),
            idle_since: aggregate_idle(
                self.state.idle_floor,
                self.state.inputs.iter().map(|seat| seat.idle_since),
            ),
        }
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
        self.inputs.push(Seat {
            seat,
            notification,
            idle_since: None,
        });
    }
    fn new_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        if let Some(index) = self.inputs.iter().position(|input| input.seat == seat) {
            self.inputs.remove(index).notification.destroy();
        }
        self.idle_floor = seconds().saturating_add(1);
        self.resumed = true;
    }
}
impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![SeatState];
}
delegate_seat!(State);
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
