//! A virtual keyboard in the kernel, for the keys the face buttons stand for.
//!
//! `wtype` was the first way, and it breaks XWayland: it hands the compositor a
//! keymap of its own for every key, and XWayland misreads it, so Enter, Tab and
//! Escape all reached Thonny as Escape. A uinput device is a keyboard like the
//! real one, with sway's keymap, and every client reads it the same way.
//!
//! Needs write access to `/dev/uinput` (the udev rule gives it to `input`).
//! Without it, `pt35d` falls back to `wtype`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;

/// The keys a binding can send, by evdev code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Escape,
    Tab,
    /// Opens the menu bar in GTK apps, Firefox and LibreOffice.
    F10,
}

impl Key {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "enter" | "return" => Key::Enter,
            "escape" | "esc" => Key::Escape,
            "tab" => Key::Tab,
            "f10" => Key::F10,
            _ => return None,
        })
    }

    fn code(self) -> u16 {
        match self {
            Key::Escape => 1,
            Key::Tab => 15,
            Key::Enter => 28,
            Key::F10 => 68,
        }
    }

    /// The keysym `wtype -k` takes, for the fallback.
    pub fn keysym(self) -> &'static str {
        match self {
            Key::Enter => "Return",
            Key::Escape => "Escape",
            Key::Tab => "Tab",
            Key::F10 => "F10",
        }
    }

    const ALL: [Key; 4] = [Key::Enter, Key::Escape, Key::Tab, Key::F10];
}

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const BUS_VIRTUAL: u16 = 6;

const REL_X: u16 = 0;
const REL_Y: u16 = 1;
const REL_WHEEL: u16 = 8;
const REL_WHEEL_HI_RES: u16 = 11;
const BTN_LEFT: u16 = 0x110;

// _IOW('U', nr, size) and _IO('U', nr) from linux/uinput.h.
const UI_SET_EVBIT: u64 = 0x4004_5564;
const UI_SET_KEYBIT: u64 = 0x4004_5565;
const UI_SET_RELBIT: u64 = 0x4004_5566;
const UI_DEV_SETUP: u64 = 0x405c_5503;
const UI_DEV_CREATE: u64 = 0x5501;

extern "C" {
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

/// `struct uinput_setup`: input_id, an 80-byte name, ff_effects_max.
#[repr(C)]
struct Setup {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
    name: [u8; 80],
    ff_effects_max: u32,
}

pub struct Keyboard {
    file: File,
}

/// Make a uinput device with these event types, keys and relative axes.
fn create(
    name: &[u8],
    product: u16,
    events: &[u16],
    keys: &[u16],
    rels: &[u16],
) -> std::io::Result<File> {
    let file = OpenOptions::new().write(true).open("/dev/uinput")?;
    let fd = file.as_raw_fd();
    let mut setup = Setup {
        bustype: BUS_VIRTUAL,
        vendor: 0x1209,
        product,
        version: 1,
        name: [0; 80],
        ff_effects_max: 0,
    };
    setup.name[..name.len()].copy_from_slice(name);
    // SAFETY: each request is given the argument type linux/uinput.h
    // declares for it, and `setup` outlives the call.
    let ok = unsafe {
        events
            .iter()
            .all(|&e| ioctl(fd, UI_SET_EVBIT, e as i32) >= 0)
            && keys
                .iter()
                .all(|&k| ioctl(fd, UI_SET_KEYBIT, k as i32) >= 0)
            && rels
                .iter()
                .all(|&r| ioctl(fd, UI_SET_RELBIT, r as i32) >= 0)
            && ioctl(fd, UI_DEV_SETUP, &setup as *const Setup) >= 0
            && ioctl(fd, UI_DEV_CREATE) >= 0
    };
    if !ok {
        return Err(std::io::Error::last_os_error());
    }
    Ok(file)
}

impl Keyboard {
    pub fn open() -> std::io::Result<Self> {
        let keys: Vec<u16> = Key::ALL.iter().map(|key| key.code()).collect();
        let file = create(b"pt35 buttons", 0x3535, &[EV_KEY], &keys, &[])?;
        Ok(Self { file })
    }

    /// Press and release `key`.
    pub fn tap(&mut self, key: Key) -> std::io::Result<()> {
        let mut out = Vec::with_capacity(4 * EVENT);
        for value in [1, 0] {
            out.extend(event(EV_KEY, key.code(), value));
            out.extend(event(EV_SYN, 0, 0));
        }
        self.file.write_all(&out)
    }
}

/// A mouse that only ever turns its wheel: X and Y in Mouse mode.
///
/// sway 1.10's `seat cursor press button4` sends a pointer frame with no
/// scroll in it, so nothing scrolled. The axes and the button are declared
/// but never sent: libinput only takes a device for a pointer with them.
pub struct Wheel {
    file: File,
}

impl Wheel {
    pub fn open() -> std::io::Result<Self> {
        let file = create(
            b"pt35 wheel",
            0x3536,
            &[EV_KEY, EV_REL],
            &[BTN_LEFT],
            &[REL_X, REL_Y, REL_WHEEL, REL_WHEEL_HI_RES],
        )?;
        Ok(Self { file })
    }

    /// One notch. A wheel turned away from you is positive, and scrolls up.
    pub fn turn(&mut self, down: bool) -> std::io::Result<()> {
        let notch = if down { -1 } else { 1 };
        let mut out = Vec::with_capacity(3 * EVENT);
        out.extend(event(EV_REL, REL_WHEEL, notch));
        out.extend(event(EV_REL, REL_WHEEL_HI_RES, notch * 120));
        out.extend(event(EV_SYN, 0, 0));
        self.file.write_all(&out)
    }
}

/// `struct input_event` on a 64-bit kernel: a zero timeval (the kernel stamps
/// it), then type, code and value.
const EVENT: usize = 24;

fn event(kind: u16, code: u16, value: i32) -> [u8; EVENT] {
    let mut out = [0u8; EVENT];
    out[16..18].copy_from_slice(&kind.to_ne_bytes());
    out[18..20].copy_from_slice(&code.to_ne_bytes());
    out[20..24].copy_from_slice(&value.to_ne_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_are_what_a_binding_writes() {
        assert_eq!(Key::parse("Return"), Some(Key::Enter));
        assert_eq!(Key::parse("esc"), Some(Key::Escape));
        assert_eq!(Key::parse("F10"), Some(Key::F10));
        assert_eq!(Key::parse("f13"), None);
    }

    #[test]
    fn an_event_is_laid_out_the_way_the_kernel_reads_it() {
        assert_eq!(std::mem::size_of::<Setup>(), 92, "UI_DEV_SETUP encodes 92");
        let bytes = event(EV_KEY, 28, 1);
        assert_eq!(&bytes[..16], &[0; 16]);
        assert_eq!(u16::from_ne_bytes([bytes[16], bytes[17]]), EV_KEY);
        assert_eq!(u16::from_ne_bytes([bytes[18], bytes[19]]), 28);
        assert_eq!(
            i32::from_ne_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
            1
        );
    }
}
