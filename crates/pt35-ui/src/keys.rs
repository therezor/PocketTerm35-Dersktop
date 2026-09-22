//! Input for a device whose face buttons are letters.
//!
//! The PocketTerm35's RP2040 presents a plain HID keyboard — there is no
//! gamepad. The twelve controls arrive as (captured from hardware, and
//! corroborated by the TrailCurrent Tracer project's keymap):
//!
//! | control | key |
//! |---|---|
//! | D-pad | arrows |
//! | A B X Y L R | the literal letters `a b x y l r` |
//! | Start | `KEY_PAUSE` |
//! | Select | `KEY_SYSRQ` (the `Print`/`Sys_Req` keysym) |
//!
//! The patched keyboard firmware in `firmware/` sends F13-F18 for the six face
//! buttons instead, so on a flashed unit none of the twelve is a character and
//! the letters below are only a fallback for stock firmware.
//!
//! On stock firmware six of the twelve are typeable, which forces a modal
//! design: in [`Mode::Nav`] letters act as buttons, in [`Mode::Filter`] they
//! type. Start and Select carry no character, so they mean the same thing in
//! both modes.
//!
//! Start closes the menu it opened. Select switches input mode and is bound in
//! the sway config, and a sway binding beats any surface, so the menu normally
//! never sees it. It is still handled here for a unit where that binding is
//! missing. Confirm is A or Enter, back is B.

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
    /// Start, via `KEY_PAUSE`.
    pub const PAUSE: u32 = 0xff13;
    /// Select, via `KEY_SYSRQ` — xkb reports it as `Print` or `Sys_Req`.
    pub const PRINT: u32 = 0xff61;
    pub const SYS_REQ: u32 = 0xff15;

    /// The six face buttons on the patched keyboard firmware (`firmware/`),
    /// which sends F13-F18 for them. The default `us` layout does not turn
    /// those into the `F13`..`F18` keysyms: `inet(evdev)` claims them first.
    pub const BUTTON_L: u32 = 0x1008ff81; // KEY_F13, XF86Tools
    pub const BUTTON_R: u32 = 0x1008ff45; // KEY_F14, XF86Launch5
    pub const BUTTON_X: u32 = 0x1008ff46; // KEY_F15, XF86Launch6
    pub const BUTTON_Y: u32 = 0x1008ff47; // KEY_F16, XF86Launch7
    pub const BUTTON_B: u32 = 0x1008ff48; // KEY_F17, XF86Launch8
    pub const BUTTON_A: u32 = 0x1008ff49; // KEY_F18, XF86Launch9

    /// The same keys under a layout that does map them to F13-F18.
    pub const F13: u32 = 0xffca;
    pub const F14: u32 = 0xffcb;
    pub const F15: u32 = 0xffcc;
    pub const F16: u32 = 0xffcd;
    pub const F17: u32 = 0xffce;
    pub const F18: u32 = 0xffcf;
}

/// The twelve physical controls, once translated out of keysyms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    L,
    R,
    Start,
    Select,
}

impl Button {
    /// The label drawn in the hint bar.
    pub fn label(self) -> &'static str {
        match self {
            Button::Up => "^",
            Button::Down => "v",
            Button::Left => "<",
            Button::Right => ">",
            Button::A => "A",
            Button::B => "B",
            Button::X => "X",
            Button::Y => "Y",
            Button::L => "L",
            Button::R => "R",
            Button::Start => "Start",
            Button::Select => "Sel",
        }
    }
}

/// Which button a key press is, if any. Letters only count in [`Mode::Nav`];
/// in [`Mode::Filter`] they are text the user is typing.
pub fn button(key: &Key, mode: Mode) -> Option<Button> {
    let by_sym = match key.sym {
        sym::UP => Some(Button::Up),
        sym::DOWN => Some(Button::Down),
        sym::LEFT => Some(Button::Left),
        sym::RIGHT => Some(Button::Right),
        sym::PAUSE => Some(Button::Start),
        sym::PRINT | sym::SYS_REQ => Some(Button::Select),
        sym::BUTTON_A | sym::F18 => Some(Button::A),
        sym::BUTTON_B | sym::F17 => Some(Button::B),
        sym::BUTTON_X | sym::F15 => Some(Button::X),
        sym::BUTTON_Y | sym::F16 => Some(Button::Y),
        sym::BUTTON_L | sym::F13 => Some(Button::L),
        sym::BUTTON_R | sym::F14 => Some(Button::R),
        _ => None,
    };
    if by_sym.is_some() || mode == Mode::Filter {
        return by_sym;
    }
    match key.text.map(|c| c.to_ascii_lowercase()) {
        Some('a') => Some(Button::A),
        Some('b') => Some(Button::B),
        Some('x') => Some(Button::X),
        Some('y') => Some(Button::Y),
        Some('l') => Some(Button::L),
        Some('r') => Some(Button::R),
        _ => None,
    }
}

/// Nav mode consumes the letter buttons; filter mode types them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Nav,
    Filter,
}

/// What a key press means to a list-shaped screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Navigation {
    Up,
    Down,
    /// D-pad left. A grid moves one tile. A list uses it to change a setting in
    /// place, and otherwise ignores it: back is B, and a nudge on the D-pad
    /// must not throw away the screen you are looking at.
    Left,
    /// D-pad right. The mirror of [`Navigation::Left`].
    Right,
    PageUp,
    PageDown,
    First,
    Last,
    /// A / Enter — open the thing under the cursor.
    Activate,
    /// Start / Select / Escape — leave the menu entirely.
    Cancel,
    /// B or Backspace on an empty filter — up one level. Not Left.
    Back,
    /// X — start typing a filter.
    StartFilter,
    /// Y — the screen's secondary action (jump home, in the menu).
    Secondary,
    /// A digit used as a direct 1-9 shortcut.
    Select(usize),
    /// A printable character appended to the filter (filter mode only).
    Filter(char),
    FilterBackspace,
    /// Nothing this screen reacts to.
    Ignored,
}

/// Translate a key press for the given mode.
pub fn navigate(key: &Key, mode: Mode) -> Navigation {
    // Start and Select first: they are the only controls that survive both
    // modes, so nothing else is allowed to shadow them.
    match button(key, mode) {
        Some(Button::Start) | Some(Button::Select) => return Navigation::Cancel,
        Some(Button::Up) => return Navigation::Up,
        Some(Button::Down) => return Navigation::Down,
        Some(Button::Right) => return Navigation::Right,
        Some(Button::Left) => return Navigation::Left,
        Some(Button::A) => return Navigation::Activate,
        Some(Button::B) => return Navigation::Back,
        Some(Button::X) => return Navigation::StartFilter,
        Some(Button::Y) => return Navigation::Secondary,
        Some(Button::L) => return Navigation::PageUp,
        Some(Button::R) => return Navigation::PageDown,
        None => {}
    }

    match key.sym {
        sym::PAGE_UP => Navigation::PageUp,
        sym::PAGE_DOWN => Navigation::PageDown,
        sym::HOME => Navigation::First,
        sym::END => Navigation::Last,
        sym::RETURN | sym::KP_ENTER => Navigation::Activate,
        sym::ESCAPE => Navigation::Cancel,
        sym::TAB => Navigation::Down,
        sym::ISO_LEFT_TAB => Navigation::Up,
        sym::BACKSPACE => match mode {
            Mode::Filter => Navigation::FilterBackspace,
            Mode::Nav => Navigation::Back,
        },
        _ => match (mode, key.text) {
            // Digits stay shortcuts while navigating; in filter mode they type.
            (Mode::Nav, Some(ch @ '1'..='9')) => Navigation::Select(ch as usize - '1' as usize),
            (Mode::Nav, Some('/')) => Navigation::StartFilter,
            (Mode::Filter, Some(ch)) if ch.is_ascii_graphic() || ch == ' ' => {
                Navigation::Filter(ch)
            }
            _ => Navigation::Ignored,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letter(ch: char) -> Key {
        Key::with_text(ch as u32, ch)
    }

    #[test]
    fn dpad_drives_the_list() {
        assert_eq!(navigate(&Key::new(sym::UP), Mode::Nav), Navigation::Up);
        assert_eq!(navigate(&Key::new(sym::DOWN), Mode::Nav), Navigation::Down);
        assert_eq!(
            navigate(&Key::new(sym::RIGHT), Mode::Nav),
            Navigation::Right
        );
        assert_eq!(navigate(&Key::new(sym::LEFT), Mode::Nav), Navigation::Left);
    }

    #[test]
    fn face_buttons_navigate_while_in_nav_mode() {
        assert_eq!(navigate(&letter('a'), Mode::Nav), Navigation::Activate);
        assert_eq!(navigate(&letter('b'), Mode::Nav), Navigation::Back);
        assert_eq!(navigate(&letter('x'), Mode::Nav), Navigation::StartFilter);
        assert_eq!(navigate(&letter('y'), Mode::Nav), Navigation::Secondary);
        assert_eq!(navigate(&letter('l'), Mode::Nav), Navigation::PageUp);
        assert_eq!(navigate(&letter('r'), Mode::Nav), Navigation::PageDown);
    }

    #[test]
    fn the_patched_firmware_sends_buttons_that_are_not_letters() {
        for mode in [Mode::Nav, Mode::Filter] {
            assert_eq!(
                navigate(&Key::new(sym::BUTTON_A), mode),
                Navigation::Activate
            );
            assert_eq!(navigate(&Key::new(sym::BUTTON_B), mode), Navigation::Back);
            assert_eq!(
                navigate(&Key::new(sym::BUTTON_X), mode),
                Navigation::StartFilter
            );
            assert_eq!(
                navigate(&Key::new(sym::BUTTON_Y), mode),
                Navigation::Secondary
            );
            assert_eq!(navigate(&Key::new(sym::BUTTON_L), mode), Navigation::PageUp);
            assert_eq!(
                navigate(&Key::new(sym::BUTTON_R), mode),
                Navigation::PageDown
            );
        }
    }

    #[test]
    fn the_same_letters_type_once_filtering() {
        for ch in ['a', 'b', 'x', 'y', 'l', 'r'] {
            assert_eq!(navigate(&letter(ch), Mode::Filter), Navigation::Filter(ch));
        }
    }

    #[test]
    fn start_and_select_mean_the_same_in_both_modes() {
        for mode in [Mode::Nav, Mode::Filter] {
            assert_eq!(navigate(&Key::new(sym::PAUSE), mode), Navigation::Cancel);
            assert_eq!(navigate(&Key::new(sym::PRINT), mode), Navigation::Cancel);
            assert_eq!(navigate(&Key::new(sym::SYS_REQ), mode), Navigation::Cancel);
        }
    }

    #[test]
    fn digits_are_shortcuts_until_you_filter() {
        assert_eq!(navigate(&letter('3'), Mode::Nav), Navigation::Select(2));
        assert_eq!(
            navigate(&letter('3'), Mode::Filter),
            Navigation::Filter('3')
        );
    }

    #[test]
    fn backspace_edits_the_filter_then_goes_back() {
        assert_eq!(
            navigate(&Key::new(sym::BACKSPACE), Mode::Filter),
            Navigation::FilterBackspace
        );
        assert_eq!(
            navigate(&Key::new(sym::BACKSPACE), Mode::Nav),
            Navigation::Back
        );
    }

    #[test]
    fn unprintable_keys_are_ignored() {
        assert_eq!(navigate(&Key::new(0xffe1), Mode::Nav), Navigation::Ignored); // shift
        assert_eq!(
            navigate(&Key::new(0xffe1), Mode::Filter),
            Navigation::Ignored
        );
    }

    #[test]
    fn button_labels_exist_for_the_hint_bar() {
        assert_eq!(Button::Start.label(), "Start");
        assert_eq!(Button::Select.label(), "Sel");
        assert_eq!(Button::A.label(), "A");
    }
}
