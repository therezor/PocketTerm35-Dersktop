//! The drawing and input half of the pt35-desktop shell.
//!
//! Everything is software-rendered into a shared-memory buffer: on a 640x480
//! panel a full repaint is a fraction of a millisecond, and avoiding GL keeps
//! both the binary and the resident set small. GTK would cost more RAM than the
//! rest of the session put together.
//!
//! The modules split cleanly in two:
//!   * portable — `canvas`, `font`, `list`, `keys`: pure logic and pixels,
//!     unit-tested on any host;
//!   * Linux-only — `layer`: the Wayland layer-shell surface that puts those
//!     pixels on screen.

pub mod canvas;
pub mod font;
pub mod icon;
pub mod keys;
pub mod list;

#[cfg(target_os = "linux")]
pub mod layer;

pub use canvas::Canvas;
pub use font::Font;
pub use keys::{Key, Navigation};
pub use list::ListState;
