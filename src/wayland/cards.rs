//! Software test cards and exclusive ordinary input on all outputs.
use crate::artwork;
use anyhow::{Context, Result, ensure};
use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    registry::RegistryState,
    seat::{SeatHandler, SeatState},
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface},
    },
    shm::{Shm, slot::SlotPool},
};
use wayland_client::{
    Connection, EventQueue, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm},
};

mod events;

pub struct TestCards {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
}
struct Seat {
    seat: wl_seat::WlSeat,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
}
struct Card {
    output: wl_output::WlOutput,
    layer: LayerSurface,
    size: Option<(u32, u32)>,
    scale: i32,
}
struct State {
    registry: RegistryState,
    seats: SeatState,
    outputs: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    inputs: Vec<Seat>,
    cards: Vec<Card>,
    closed_outputs: Vec<(wl_output::WlOutput, std::time::Instant)>,
    remaining: u64,
    error: Option<String>,
}

/// Bind required card protocols without creating a surface.
pub(super) fn check() -> Result<()> {
    let connection = Connection::connect_to_env()?;
    let (globals, queue) = registry_queue_init::<State>(&connection)?;
    let qh = queue.handle();
    CompositorState::bind(&globals, &qh)?;
    LayerShell::bind(&globals, &qh).context("wlr-layer-shell is required")?;
    Shm::bind(&globals, &qh)?;
    Ok(())
}

impl TestCards {
    pub fn show(remaining: u64) -> Result<Self> {
        super::connect(move || Self::connect_inner(remaining))
    }

    fn connect_inner(remaining: u64) -> Result<Self> {
        let connection = Connection::connect_to_env().context("Connecting to Wayland")?;
        let (globals, mut queue) = registry_queue_init(&connection)?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh)?;
        let layer_shell = LayerShell::bind(&globals, &qh).context("wlr-layer-shell is required")?;
        let shm = Shm::bind(&globals, &qh)?;
        let pool = SlotPool::new(4, &shm)?;
        let mut state = State {
            registry: RegistryState::new(&globals),
            seats: SeatState::new(&globals, &qh),
            outputs: OutputState::new(&globals, &qh),
            compositor,
            layer_shell,
            shm,
            pool,
            inputs: Vec::new(),
            cards: Vec::new(),
            closed_outputs: Vec::new(),
            remaining,
            error: None,
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
        super::dispatch(&self.connection, &mut self.queue, &mut self.state)?;
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
        if self.cards.iter().any(|card| card.output == output) {
            return;
        }
        self.closed_outputs.retain(|(closed, _)| closed != &output);
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("bork"),
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
            size: None,
            scale: 1,
        });
    }

    fn draw(&mut self, index: usize) -> Result<()> {
        let card = &self.cards[index];
        let Some((width, height)) = card.size else {
            return Ok(());
        };
        let width = width
            .checked_mul(card.scale as u32)
            .context("Invalid card width")?;
        let height = height
            .checked_mul(card.scale as u32)
            .context("Invalid card height")?;
        ensure!(
            width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 33_554_432,
            "Unsupported output dimensions {width}x{height}"
        );
        let image = artwork::test_card(1280, 720, self.remaining);
        let (buffer, canvas) = self.pool.create_buffer(
            width as i32,
            height as i32,
            width as i32 * 4,
            wl_shm::Format::Argb8888,
        )?;
        image.write_argb_scaled(width as usize, height as usize, canvas);
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
