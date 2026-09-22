//! Key handling for a device with a D-pad and no mouse.
//!
//! The shell's UI only ever needs a dozen logical actions. Mapping xkb keysyms
//! to them in one place keeps the menu, the bar and the pointer consistent, and
//! makes the whole navigation model unit-testable without a compositor.

/// A key press, as the Wayland keyboard reports it (xkb keysym + printable text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub sym: u32,
    pub text: Option<char>,
    pub ctrl: bool,
    pub shift: bool,
}

impl Key {
    pub fn new(sym: u32) -> Self {
        Self {
            sym,
            text: None,
            ctrl: false,
            shift: false,
        }
    }

    pub fn with_text(sym: u32, text: char) -> Self {
        Self {
            sym,
            text: Some(text),
            ctrl: false,
            shift: false,
        }
    }
}

/// xkb keysyms we care about (from `xkbcommon-keysyms.h`).
pub mod sym {
    pub const ESCAPE: u32 = 0xff1b;
    pub const RETURN: u32 = 0xff0d;
    pub const KP_ENTER: u32 = 0xff8d;
    pub const BACKSPACE: u32 = 0xff08;
    pub const TAB: u32 = 0xff09;
    pub const ISO_LEFT_TAB: u32 = 0xfe20;
    pub const LEFT: u32 = 0xff51;
    pub const UP: u32 = 0xff52;
    pub const RIGHT: u32 = 0xff53;
    pub const DOWN: u32 = 0xff54;
    pub const PAGE_UP: u32 = 0xff55;
    pub const PAGE_DOWN: u32 = 0xff56;
    pub const HOME: u32 = 0xff50;
    pub const END: u32 = 0xff57;
    pub const SPACE: u32 = 0x0020;
}

/// What a key press means to a list-shaped screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Navigation {
    Up,
    Down,
    PageUp,
    PageDown,
    First,
    Last,
    /// Enter / D-pad centre.
    Activate,
    /// Escape — close the screen.
    Cancel,
    /// Backspace with an empty filter — go up one menu level.
    Back,
    /// A digit key used as a direct 1-9 shortcut.
    Select(usize),
    /// A printable character appended to the filter.
    Filter(char),
    /// Backspace with a non-empty filter.
    FilterBackspace,
    /// Nothing this screen reacts to.
    Ignored,
}

/// Translate a key press. `filtering` says whether a filter string is currently
/// non-empty, which is what decides Backspace's meaning and whether digits are
/// shortcuts or literal text.
pub fn navigate(key: &Key, filtering: bool) -> Navigation {
    match key.sym {
        sym::UP => Navigation::Up,
        sym::DOWN => Navigation::Down,
        sym::PAGE_UP => Navigation::PageUp,
        sym::PAGE_DOWN => Navigation::PageDown,
        sym::HOME => Navigation::First,
        sym::END => Navigation::Last,
        sym::RETURN | sym::KP_ENTER | sym::RIGHT => Navigation::Activate,
        sym::ESCAPE => Navigation::Cancel,
        sym::LEFT => Navigation::Back,
        sym::TAB => Navigation::Down,
        sym::ISO_LEFT_TAB => Navigation::Up,
        sym::BACKSPACE => {
            if filtering {
                Navigation::FilterBackspace
            } else {
                Navigation::Back
            }
        }
        _ => match key.text {
            // Digits are shortcuts until the user starts typing a filter.
            Some(ch @ '1'..='9') if !filtering => Navigation::Select(ch as usize - '1' as usize),
            Some(ch) if ch.is_ascii_graphic() || ch == ' ' => Navigation::Filter(ch),
            _ => Navigation::Ignored,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpad_drives_the_list() {
        assert_eq!(navigate(&Key::new(sym::UP), false), Navigation::Up);
        assert_eq!(navigate(&Key::new(sym::DOWN), false), Navigation::Down);
        assert_eq!(navigate(&Key::new(sym::RIGHT), false), Navigation::Activate);
        assert_eq!(navigate(&Key::new(sym::LEFT), false), Navigation::Back);
        assert_eq!(
            navigate(&Key::new(sym::RETURN), false),
            Navigation::Activate
        );
        assert_eq!(navigate(&Key::new(sym::ESCAPE), false), Navigation::Cancel);
    }

    #[test]
    fn digits_are_shortcuts_until_you_type() {
        assert_eq!(
            navigate(&Key::with_text(0x0033, '3'), false),
            Navigation::Select(2)
        );
        assert_eq!(
            navigate(&Key::with_text(0x0033, '3'), true),
            Navigation::Filter('3')
        );
    }

    #[test]
    fn backspace_leaves_the_filter_before_it_leaves_the_menu() {
        assert_eq!(
            navigate(&Key::new(sym::BACKSPACE), true),
            Navigation::FilterBackspace
        );
        assert_eq!(navigate(&Key::new(sym::BACKSPACE), false), Navigation::Back);
    }

    #[test]
    fn unprintable_keys_are_ignored() {
        assert_eq!(navigate(&Key::new(0xffe1), false), Navigation::Ignored); // shift
        assert_eq!(
            navigate(&Key::with_text(0x0020, ' '), true),
            Navigation::Filter(' ')
        );
    }
}
