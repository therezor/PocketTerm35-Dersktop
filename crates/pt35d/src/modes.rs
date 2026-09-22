//! The two input modes.
//!
//! Twelve controls, two things to do with them. In Buttons mode the D-pad is
//! the arrow keys and A is Enter; in Mouse mode the D-pad moves the cursor and
//! A is a left click. L switches mode and R closes the window, in both. Start
//! (the menu) and Select (the window manager) are bound in the sway config, so
//! neither mode can lose them.
//!
//! Both modes are plain sway bindings on the keysyms the patched firmware
//! sends, so nothing runs in the background and the letters keep typing. Enter,
//! Escape and Tab go through `wtype`, because sway has no "send key" command;
//! the clicks and the cursor moves are sway's own `seat cursor` commands.

use pt35_common::ipc::InputMode;
use pt35_common::theme::Pointer;

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

/// What both modes share: L switches mode, R closes the window. Neither
/// repeats: holding R must not close every window you own.
fn common() -> Vec<Bind> {
    vec![
        Bind::new("--no-repeat", L, "exec pt35ctl mode toggle"),
        Bind::new("--no-repeat", R, "exec pt35ctl window close"),
    ]
}

fn buttons() -> Vec<Bind> {
    vec![
        Bind::new("", A, "exec wtype -k Return"),
        Bind::new("", B, "exec wtype -k Escape"),
        Bind::new("", X, "exec wtype -k Tab"),
        Bind::new("--no-repeat", Y, "exec pt35ctl window fullscreen"),
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
    // Press and release separately, so holding A drags and holding X scrolls.
    for (key, button) in [(A, 1), (B, 3), (X, 4), (Y, 5)] {
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

/// Everything else a mode needs from sway, after the bindings.
pub fn settings(mode: InputMode, pointer: &Pointer) -> Vec<String> {
    match mode {
        // One press of the D-pad, one row.
        InputMode::Buttons => vec![
            "input type:keyboard repeat_delay 500".into(),
            "input type:keyboard repeat_rate 8".into(),
            format!("seat {SEAT} hide_cursor 1500"),
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
    fn the_shoulders_switch_mode_and_close_in_both_modes() {
        for mode in [InputMode::Buttons, InputMode::Mouse] {
            let binds = binds(mode, &Pointer::default());
            let find = |key: &str| {
                binds
                    .iter()
                    .find(|b| b.key == key)
                    .unwrap_or_else(|| panic!("{key} is not bound in {mode:?}"))
            };
            assert!(find(L).action.ends_with("mode toggle"));
            assert!(find(R).action.ends_with("window close"));
            // Holding R must not close every window you own.
            assert_eq!(find(R).flags, "--no-repeat");
        }
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
    }
}
