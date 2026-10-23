//! Wayland adapters with separate capabilities: observe input or own test cards.
mod cards;
mod input;

pub use cards::TestCards;
pub use input::{Activity, InputMonitor};

use anyhow::{Context, Result};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use wayland_client::{Connection, EventQueue};

/// Non-mutating validation of all protocols required for background operation.
pub fn check() -> Result<InputMonitor> {
    connect(|| {
        let input = InputMonitor::connect_inner()?;
        cards::check()?;
        Ok(input)
    })
}

fn connect<T: Send + 'static>(
    initialize: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(initialize());
    });
    // Registry roundtrips may stall. On timeout the caller exits; teardown closes
    // worker-owned surfaces. The guardian must not wait indefinitely for Wayland.
    rx.recv_timeout(std::time::Duration::from_secs(2))
        .context("Wayland initialization timed out; stopping safely")?
}

/// Never block the guardian's independent emergency-stop/heartbeat loop.
fn dispatch<S: 'static>(
    connection: &Connection,
    queue: &mut EventQueue<S>,
    state: &mut S,
) -> Result<()> {
    queue.dispatch_pending(state)?;
    connection.flush()?;
    if let Some(guard) = queue.prepare_read() {
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
    queue.dispatch_pending(state)?;
    Ok(())
}
