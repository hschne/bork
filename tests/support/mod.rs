//! Disposable fullscreen sample desktop and synthetic input, only inside the test namespace.
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    os::fd::AsFd,
};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, WEnum, delegate_noop,
    globals::registry_queue_init,
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_shm_pool,
        wl_surface,
    },
};
use wayland_protocols::{
    ext::session_lock::v1::client::{
        ext_session_lock_manager_v1::ExtSessionLockManagerV1,
        ext_session_lock_surface_v1::{self, ExtSessionLockSurfaceV1},
        ext_session_lock_v1::{self, ExtSessionLockV1},
    },
    wp::idle_inhibit::zv1::client::{
        zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1,
        zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1,
    },
    xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base},
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

#[derive(Default)]
pub struct State {
    pub keys: usize,
    pub clicks: usize,
    pub motions: usize,
    pub configured: bool,
    locked: bool,
    buffer: Option<wl_buffer::WlBuffer>,
}

pub struct Desktop {
    _connection: Connection,
    queue: EventQueue<State>,
    pub state: State,
    keyboard: ZwpVirtualKeyboardV1,
    pointer: ZwlrVirtualPointerV1,
    _file: File,
    _buffer: wl_buffer::WlBuffer,
    _pool: wl_shm_pool::WlShmPool,
    _inhibitor: ZwpIdleInhibitorV1,
    _surface: wl_surface::WlSurface,
    clock: u32,
    compositor: wl_compositor::WlCompositor,
    lock_manager: ExtSessionLockManagerV1,
    outputs: Vec<wl_output::WlOutput>,
}

pub struct LockSession {
    lock: ExtSessionLockV1,
    surfaces: Vec<(ExtSessionLockSurfaceV1, wl_surface::WlSurface)>,
}

impl Desktop {
    pub fn new() -> Self {
        assert_eq!(std::env::var("SIGNAL_LOSS_ISOLATED").as_deref(), Ok("1"));
        let connection = Connection::connect_to_env().unwrap();
        let (globals, mut queue) = registry_queue_init::<State>(&connection).unwrap();
        let qh = queue.handle();
        let lock_manager: ExtSessionLockManagerV1 = globals.bind(&qh, 1..=1, ()).unwrap();
        let outputs = globals.contents().with_list(|list| {
            list.iter()
                .filter(|global| global.interface == "wl_output")
                .map(|global| {
                    globals
                        .registry()
                        .bind(global.name, global.version.min(4), &qh, ())
                })
                .collect()
        });
        let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=6, ()).unwrap();
        let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).unwrap();
        let seat: wl_seat::WlSeat = globals.bind(&qh, 7..=9, ()).unwrap();
        let keyboard_manager: ZwpVirtualKeyboardManagerV1 = globals.bind(&qh, 1..=1, ()).unwrap();
        let pointer_manager: ZwlrVirtualPointerManagerV1 = globals.bind(&qh, 1..=2, ()).unwrap();
        let keyboard = keyboard_manager.create_virtual_keyboard(&seat, &qh, ());
        let pointer = pointer_manager.create_virtual_pointer(Some(&seat), &qh, ());
        let keymap = b"xkb_keymap { xkb_keycodes \"minimal\" { minimum=8; maximum=255; <AC01>=38; }; xkb_types \"minimal\" { type \"ONE_LEVEL\" { modifiers=None; map[None]=Level1; level_name[Level1]=\"Any\"; }; }; xkb_compatibility \"minimal\" {}; xkb_symbols \"minimal\" { key <AC01> { [ a ] }; }; };\0";
        let mut keymap_file = tempfile::tempfile().unwrap();
        keymap_file.write_all(keymap).unwrap();
        keyboard.keymap(1, keymap_file.as_fd(), keymap.len() as u32);
        let mut state = State::default();
        queue.roundtrip(&mut state).unwrap();
        seat.get_keyboard(&qh, ());
        seat.get_pointer(&qh, ());
        let shell: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=6, ()).unwrap();
        let surface = compositor.create_surface(&qh, ());
        let xdg_surface = shell.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg_surface.get_toplevel(&qh, ());
        toplevel.set_title("Signal Loss disposable input target".into());
        toplevel.set_app_id("signal-loss-proof-target".into());
        toplevel.set_fullscreen(None);
        surface.commit();
        queue.roundtrip(&mut state).unwrap();
        assert!(state.configured);
        let image = signal_loss::artwork::desktop();
        let mut file = tempfile::tempfile().unwrap();
        for pixel in image.rgba().chunks_exact(4) {
            let colour = (255u32 << 24)
                | ((pixel[0] as u32) << 16)
                | ((pixel[1] as u32) << 8)
                | pixel[2] as u32;
            file.write_all(&colour.to_ne_bytes()).unwrap();
        }
        file.seek(SeekFrom::Start(0)).unwrap();
        let pool = shm.create_pool(file.as_fd(), image.rgba().len() as i32, &qh, ());
        let buffer = pool.create_buffer(0, 1280, 720, 1280 * 4, wl_shm::Format::Argb8888, &qh, ());
        state.buffer = Some(buffer.clone());
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, 1280, 720);
        surface.commit();
        let inhibitor_manager: ZwpIdleInhibitManagerV1 = globals.bind(&qh, 1..=1, ()).unwrap();
        let inhibitor = inhibitor_manager.create_inhibitor(&surface, &qh, ());
        queue.roundtrip(&mut state).unwrap();
        Self {
            _connection: connection,
            queue,
            state,
            keyboard,
            pointer,
            _file: file,
            _buffer: buffer,
            _pool: pool,
            _inhibitor: inhibitor,
            _surface: surface,
            clock: 0,
            compositor,
            lock_manager,
            outputs,
        }
    }

    pub fn lock(&mut self) -> LockSession {
        let qh = self.queue.handle();
        let lock = self.lock_manager.lock(&qh, ());
        let surfaces = self
            .outputs
            .iter()
            .map(|output| {
                let surface = self.compositor.create_surface(&qh, ());
                let layer = lock.get_lock_surface(&surface, output, &qh, surface.clone());
                (layer, surface)
            })
            .collect();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        while !self.state.locked {
            assert!(
                std::time::Instant::now() < deadline,
                "Isolated locker did not lock"
            );
            self.dispatch();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        LockSession { lock, surfaces }
    }

    pub fn unlock(&mut self, session: LockSession) {
        session.lock.unlock_and_destroy();
        for (layer, surface) in session.surfaces {
            layer.destroy();
            surface.destroy();
        }
        self.state.locked = false;
        self.dispatch();
    }

    pub fn dispatch(&mut self) {
        self.queue.roundtrip(&mut self.state).unwrap();
    }

    pub fn key(&mut self) {
        self.clock += 10;
        self.keyboard.key(self.clock, 30, 1);
        self.keyboard.key(self.clock + 1, 30, 0);
        self.dispatch();
    }

    pub fn click(&mut self) {
        self.clock += 10;
        self.pointer
            .motion_absolute(self.clock, 400, 300, 2560, 720);
        self.pointer.frame();
        self.pointer
            .button(self.clock + 1, 0x110, wl_pointer::ButtonState::Pressed);
        self.pointer
            .button(self.clock + 2, 0x110, wl_pointer::ButtonState::Released);
        self.pointer.frame();
        self.dispatch();
    }

    pub fn motion(&mut self) {
        self.clock += 10;
        self.pointer.motion(self.clock, 8.0, 0.0);
        self.pointer.frame();
        self.dispatch();
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn event(
        _: &mut Self,
        shell: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            shell.pong(serial);
        }
    }
}
impl Dispatch<xdg_surface::XdgSurface, ()> for State {
    fn event(
        state: &mut Self,
        surface: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            surface.ack_configure(serial);
            state.configured = true;
        }
    }
}
impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key {
            state: WEnum::Value(wl_keyboard::KeyState::Pressed),
            ..
        } = event
        {
            state.keys += 1;
        }
    }
}
impl Dispatch<wl_pointer::WlPointer, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Button {
                state: WEnum::Value(wl_pointer::ButtonState::Pressed),
                ..
            } => state.clicks += 1,
            wl_pointer::Event::Motion { .. } => state.motions += 1,
            _ => (),
        }
    }
}

impl Dispatch<ExtSessionLockV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtSessionLockV1,
        event: ext_session_lock_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_v1::Event::Locked = event {
            state.locked = true;
        }
    }
}
impl Dispatch<ExtSessionLockSurfaceV1, wl_surface::WlSurface> for State {
    fn event(
        state: &mut Self,
        layer: &ExtSessionLockSurfaceV1,
        event: ext_session_lock_surface_v1::Event,
        surface: &wl_surface::WlSurface,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
        {
            assert_eq!((width, height), (1280, 720));
            layer.ack_configure(serial);
            surface.attach(state.buffer.as_ref(), 0, 0);
            surface.damage_buffer(0, 0, 1280, 720);
            surface.commit();
        }
    }
}

delegate_noop!(State: ignore ExtSessionLockManagerV1);
delegate_noop!(State: ignore wl_output::WlOutput);
delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_buffer::WlBuffer);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore wl_seat::WlSeat);
delegate_noop!(State: ignore xdg_toplevel::XdgToplevel);
delegate_noop!(State: ignore ZwpVirtualKeyboardManagerV1);
delegate_noop!(State: ignore ZwpVirtualKeyboardV1);
delegate_noop!(State: ignore ZwlrVirtualPointerManagerV1);
delegate_noop!(State: ignore ZwlrVirtualPointerV1);
delegate_noop!(State: ignore ZwpIdleInhibitManagerV1);
delegate_noop!(State: ignore ZwpIdleInhibitorV1);

impl
    Dispatch<
        wayland_client::protocol::wl_registry::WlRegistry,
        wayland_client::globals::GlobalListContents,
    > for State
{
    fn event(
        _: &mut Self,
        _: &wayland_client::protocol::wl_registry::WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &wayland_client::globals::GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
