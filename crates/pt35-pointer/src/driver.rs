//! The pointer lives on its own Wayland connection.
//!
//! The overlay surface (keyboard + drawing) and the virtual pointer are two
//! independent clients of the same compositor: that keeps the layer-shell event
//! loop in pt35-ui generic, and the protocol has no events to interleave anyway.

use anyhow::Result;
use std::sync::mpsc::{channel, Sender};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::Connection;

use crate::delegate_virtual_pointer;
use crate::virtual_pointer::VirtualPointer;

/// One thing to do with the cursor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    Motion(f32, f32),
    Absolute { x: f32, y: f32, extent: (u32, u32) },
    Click(u32),
    Scroll(f64),
    Stop,
}

struct State;
delegate_virtual_pointer!(State);

/// The registry itself sends events (globals coming and going); we ignore them.
impl wayland_client::Dispatch<wayland_client::protocol::wl_registry::WlRegistry, GlobalListContents>
    for State
{
    fn event(
        _: &mut Self,
        _: &wayland_client::protocol::wl_registry::WlRegistry,
        _: <wayland_client::protocol::wl_registry::WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

/// Start the driver thread; the returned sender is how everything else moves
/// the cursor.
pub fn spawn() -> Result<Sender<Cmd>> {
    let (tx, rx) = channel::<Cmd>();
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let mut pointer = VirtualPointer::new(&globals, &qh)?;

    std::thread::Builder::new()
        .name("pointer".into())
        .spawn(move || {
            let mut state = State;
            for cmd in rx {
                match cmd {
                    Cmd::Motion(dx, dy) => pointer.motion(dx, dy),
                    Cmd::Absolute { x, y, extent } => pointer.motion_absolute(x, y, extent),
                    Cmd::Click(button) => pointer.click(button),
                    Cmd::Scroll(notches) => pointer.scroll(notches),
                    Cmd::Stop => break,
                }
                if let Err(e) = queue.roundtrip(&mut state) {
                    log::error!("virtual pointer connection lost: {e}");
                    break;
                }
            }
        })?;

    Ok(tx)
}
