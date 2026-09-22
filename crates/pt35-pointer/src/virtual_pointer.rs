//! The `wlr-virtual-pointer-unstable-v1` client.
//!
//! sway implements this protocol, so the cursor we drive with the D-pad is the
//! compositor's real cursor: applications cannot tell the difference, which is
//! the whole point.

use anyhow::{Context, Result};
use wayland_client::globals::GlobalList;
use wayland_client::{Dispatch, QueueHandle};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

/// Axis values are in the same units as a mouse wheel notch (15 "degrees").
const WHEEL_NOTCH: f64 = 15.0;

pub struct VirtualPointer {
    pointer: ZwlrVirtualPointerV1,
    /// Accumulated sub-pixel motion: sending 0.4 px every frame would round to
    /// nothing, so the remainder is carried over.
    residual: (f64, f64),
    started: std::time::Instant,
}

impl VirtualPointer {
    pub fn new<S>(globals: &GlobalList, qh: &QueueHandle<S>) -> Result<Self>
    where
        S: Dispatch<ZwlrVirtualPointerManagerV1, ()> + Dispatch<ZwlrVirtualPointerV1, ()> + 'static,
    {
        let manager: ZwlrVirtualPointerManagerV1 = globals
            .bind(qh, 1..=2, ())
            .context("this compositor has no wlr-virtual-pointer; pt35-pointer needs sway")?;
        let pointer = manager.create_virtual_pointer(None, qh, ());
        Ok(Self {
            pointer,
            residual: (0.0, 0.0),
            started: std::time::Instant::now(),
        })
    }

    fn time(&self) -> u32 {
        self.started.elapsed().as_millis() as u32
    }

    /// Relative motion in pixels; fractions are carried to the next call.
    pub fn motion(&mut self, dx: f32, dy: f32) {
        let x = self.residual.0 + dx as f64;
        let y = self.residual.1 + dy as f64;
        let (ix, iy) = (x.trunc(), y.trunc());
        self.residual = (x - ix, y - iy);
        if ix == 0.0 && iy == 0.0 {
            return;
        }
        self.pointer.motion(self.time(), ix, iy);
        self.pointer.frame();
    }

    /// Absolute move, used by grid jump. `extent` is the output size.
    pub fn motion_absolute(&mut self, x: f32, y: f32, extent: (u32, u32)) {
        self.pointer.motion_absolute(
            self.time(),
            x.max(0.0) as u32,
            y.max(0.0) as u32,
            extent.0,
            extent.1,
        );
        self.pointer.frame();
        self.residual = (0.0, 0.0);
    }

    pub fn click(&mut self, button: u32) {
        use wayland_client::protocol::wl_pointer::ButtonState;
        self.pointer
            .button(self.time(), button, ButtonState::Pressed);
        self.pointer.frame();
        self.pointer
            .button(self.time(), button, ButtonState::Released);
        self.pointer.frame();
    }

    pub fn scroll(&mut self, notches: f64) {
        use wayland_client::protocol::wl_pointer::Axis;
        self.pointer
            .axis(self.time(), Axis::VerticalScroll, notches * WHEEL_NOTCH);
        self.pointer.frame();
    }
}

impl Drop for VirtualPointer {
    fn drop(&mut self) {
        self.pointer.destroy();
    }
}

/// The pointer protocol has no events, so dispatching is a formality.
#[macro_export]
macro_rules! delegate_virtual_pointer {
    ($state:ty) => {
        impl wayland_client::Dispatch<
            wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
            (),
        > for $state
        {
            fn event(
                _: &mut Self,
                _: &wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
                _: <wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 as wayland_client::Proxy>::Event,
                _: &(),
                _: &wayland_client::Connection,
                _: &wayland_client::QueueHandle<Self>,
            ) {
            }
        }

        impl wayland_client::Dispatch<
            wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
            (),
        > for $state
        {
            fn event(
                _: &mut Self,
                _: &wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
                _: <wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1 as wayland_client::Proxy>::Event,
                _: &(),
                _: &wayland_client::Connection,
                _: &wayland_client::QueueHandle<Self>,
            ) {
            }
        }
    };
}
