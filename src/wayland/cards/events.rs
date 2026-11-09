//! SCTK protocol callbacks. Card connection and software drawing stay in the owner.
use super::{Seat, State};
use smithay_client_toolkit::{
    compositor::CompositorHandler,
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
    shell::{
        WaylandSurface,
        wlr_layer::{LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
    },
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    Connection, QueueHandle, delegate_noop,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
};

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.inputs.push(Seat {
            seat,
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
            if let Some(keyboard) = input.keyboard {
                keyboard.release();
            }
            if let Some(pointer) = input.pointer {
                pointer.release();
            }
        }
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
            self.cards[index].size = Some(configure.new_size);
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
delegate_dispatch2!(State);
delegate_registry!(State);
