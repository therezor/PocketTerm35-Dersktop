//! The two input modes.
//!
//! Twelve controls, two things to do with them. In Buttons mode the D-pad is
//! the arrow keys and A is Enter; in Mouse mode the D-pad moves the cursor and
//! A is a left click. In both, R switches mode and L closes the window. Start
//! (the menu) and Select (the window switcher) are bound by pt35d outside the
//! modes, so neither mode can lose them.
//!
//! Both modes are plain sway bindings on the keysyms the patched firmware
//! sends, so nothing runs in the background and the letters keep typing. Enter,
//! Escape and Tab go through `pt35ctl key`, because sway has no "send key" command;
//! the clicks and the cursor moves are sway's own `seat cursor` commands.

use pt35_common::ipc::InputMode;
use pt35_common::theme::{Buttons, Pointer};

/// The one seat. sway names it `seat0` unless a config says otherwise, and ours
/// does not.
const SEAT: &str = "seat0";

/// One sway binding, kept whole so it can be taken back exactly as it was given:
/// `unbindsym` matches on the flags too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    pub flags: &'static str,
    pub key: String,
    pub action: String,
}

impl Bind {
    fn new(flags: &'static str, key: &str, action: &str) -> Self {
        Self {
            flags,
            key: key.to_string(),
            action: action.to_string(),
        }
    }

    pub fn bind(&self) -> String {
        format!("bindsym {} {} {}", self.flags, self.key, self.action)
    }

    pub fn unbind(&self) -> String {
        format!("unbindsym {} {}", self.flags, self.key)
    }
}

/// Face buttons, by the keysym the firmware sends for each.
const A: &str = "XF86Launch9";
const B: &str = "XF86Launch8";
const X: &str = "XF86Launch6";
const Y: &str = "XF86Launch7";
const L: &str = "XF86Tools";
const R: &str = "XF86Launch5";

/// What both modes share.
///
/// R switches between Buttons and Mouse, L closes the focused window. The
/// window switcher moved to Select. Neither repeats: holding a shoulder must
/// not close a row of windows one after another.
fn common() -> Vec<Bind> {
    vec![
        Bind::new("--no-repeat", L, "exec pt35ctl window close"),
        Bind::new("--no-repeat", R, "exec pt35ctl mode toggle"),
    ]
}

fn buttons() -> Vec<Bind> {
    vec![
        Bind::new("", A, "exec pt35ctl key enter"),
        Bind::new("", B, "exec pt35ctl key escape"),
        Bind::new("", X, "exec pt35ctl key tab"),
        // The app's own menu bar. Fullscreen stays on $mod+f.
        Bind::new("--no-repeat", Y, "exec pt35ctl key f10"),
    ]
}

fn mouse(step: i32) -> Vec<Bind> {
    let mut out = Vec::new();
    for (key, dx, dy) in [
        ("Up", 0, -step),
        ("Down", 0, step),
        ("Left", -step, 0),
        ("Right", step, 0),
    ] {
        out.push(Bind::new(
            "",
            key,
            &format!("seat {SEAT} cursor move {dx} {dy}"),
        ));
    }
    // Press and release separately, so holding A drags.
    for (key, button) in [(A, 1), (B, 3)] {
        out.push(Bind::new(
            "--no-repeat",
            key,
            &format!("seat {SEAT} cursor press button{button}"),
        ));
        out.push(Bind::new(
            "--release",
            key,
            &format!("seat {SEAT} cursor release button{button}"),
        ));
    }
    // The wheel through pt35d's uinput mouse: sway's `cursor press button4`
    // sends no scroll. Repeating, so holding X or Y keeps scrolling.
    out.push(Bind::new("", X, "exec pt35ctl wheel up"));
    out.push(Bind::new("", Y, "exec pt35ctl wheel down"));
    out
}

/// Every binding a mode holds.
pub fn binds(mode: InputMode, pointer: &Pointer) -> Vec<Bind> {
    let mut out = common();
    match mode {
        InputMode::Buttons => out.extend(buttons()),
        InputMode::Mouse => out.extend(mouse(pointer.step)),
    }
    out
}

/// What stays bound while the menu is up. In Mouse mode the cursor keeps
/// working over the menu, so the D-pad, the clicks and the wheel stay; the
/// shoulders go to the menu, which does their job itself. In Buttons mode
/// everything goes to the menu.
pub fn menu_binds(mode: InputMode, pointer: &Pointer) -> Vec<Bind> {
    match mode {
        InputMode::Buttons => Vec::new(),
        InputMode::Mouse => mouse(pointer.step),
    }
}

/// Everything else a mode needs from sway, after the bindings.
pub fn settings(mode: InputMode, pointer: &Pointer, buttons: &Buttons) -> Vec<String> {
    match mode {
        // One press of the D-pad, one row.
        InputMode::Buttons => vec![
            format!("input type:keyboard repeat_delay {}", buttons.repeat_delay),
            format!("input type:keyboard repeat_rate {}", buttons.repeat_rate),
            format!("seat {SEAT} hide_cursor {}", buttons.hide_cursor),
        ],
        InputMode::Mouse => vec![
            format!("input type:keyboard repeat_delay {}", pointer.repeat_delay),
            format!("input type:keyboard repeat_rate {}", pointer.repeat_rate),
            // Hiding the cursor after 1.5s of stillness is wrong when the D-pad
            // is what moves it.
            format!("seat {SEAT} hide_cursor 0"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binding_is_taken_back_with_the_flags_it_was_given() {
        let bind = Bind::new("--release", "XF86Tools", "exec pt35ctl window prev");
        assert_eq!(
            bind.bind(),
            "bindsym --release XF86Tools exec pt35ctl window prev"
        );
        assert_eq!(bind.unbind(), "unbindsym --release XF86Tools");
    }

    #[test]
    fn the_shoulders_close_and_switch_mode_in_both_modes() {
        for mode in [InputMode::Buttons, InputMode::Mouse] {
            let binds = binds(mode, &Pointer::default());
            let find = |key: &str| {
                binds
                    .iter()
                    .find(|b| b.key == key)
                    .unwrap_or_else(|| panic!("{key} is not bound in {mode:?}"))
            };
            assert_eq!(find(L).action, "exec pt35ctl window close");
            assert_eq!(find(R).action, "exec pt35ctl mode toggle");
            assert_eq!(find(L).flags, "--no-repeat");
            assert_eq!(find(R).flags, "--no-repeat");
        }
    }

    #[test]
    fn the_menu_keeps_the_cursor_in_mouse_mode_but_not_the_shoulders() {
        let pointer = Pointer::default();
        assert!(menu_binds(InputMode::Buttons, &pointer).is_empty());
        let mouse = menu_binds(InputMode::Mouse, &pointer);
        assert!(mouse.iter().any(|b| b.key == "Up"));
        assert!(mouse.iter().all(|b| b.key != L && b.key != R));
    }

    #[test]
    fn no_bound_key_is_a_character() {
        for mode in [InputMode::Buttons, InputMode::Mouse] {
            for bind in binds(mode, &Pointer::default()) {
                assert!(
                    bind.key.starts_with("XF86")
                        || matches!(bind.key.as_str(), "Up" | "Down" | "Left" | "Right"),
                    "{} would stop the keyboard typing it",
                    bind.key
                );
            }
        }
    }

    #[test]
    fn a_and_b_are_the_mouse_buttons() {
        let mouse = mouse(16);
        assert!(mouse
            .iter()
            .any(|b| b.key == A && b.action == "seat seat0 cursor press button1"));
        assert!(mouse
            .iter()
            .any(|b| b.key == B && b.action == "seat seat0 cursor release button3"));
        assert!(mouse
            .iter()
            .any(|b| b.key == Y && b.action == "exec pt35ctl wheel down"));
    }
}
