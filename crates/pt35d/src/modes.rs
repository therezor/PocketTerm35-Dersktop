//! The two input modes.
//!
//! Twelve controls, two things to do with them. In Buttons mode the D-pad is
//! the arrow keys and A is Enter; in Mouse mode the D-pad moves the cursor and
//! A is a left click. Select switches, and is bound in the sway config so
//! neither mode can lose it. L closes the window and R opens the picker, in
//! both.
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

/// Held together. Written in press order, which is the order sway matches.
fn chord() -> String {
    format!("{L}+{R}")
}

/// What both modes share: the shoulders switch app, and together they close.
fn common() -> Vec<Bind> {
    vec![
        Bind::new("--release", L, "exec pt35ctl window prev"),
        Bind::new("--release", R, "exec pt35ctl window next"),
        Bind::new("--no-repeat", &chord(), "exec pt35ctl window close"),
        // Without this, letting go of the chord runs one of the two release
        // bindings above and the focus jumps after the window closes.
        Bind::new("--release", &chord(), "nop"),
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
    fn the_shoulders_switch_app_and_close_together_in_both_modes() {
        for mode in [InputMode::Buttons, InputMode::Mouse] {
            let binds = binds(mode, &Pointer::default());
            assert!(binds
                .iter()
                .any(|b| b.key == "XF86Tools+XF86Launch5" && b.action.ends_with("window close")));
            assert!(binds
                .iter()
                .any(|b| b.key == "XF86Tools" && b.flags == "--release"));
        }
    }

    #[test]
    fn the_shoulders_fire_on_release_so_the_chord_can_win() {
        // sway matches a binding as soon as its keys are down: a press binding
        // on L would run before R could join it.
        for bind in binds(InputMode::Buttons, &Pointer::default()) {
            if bind.key == "XF86Tools" || bind.key == "XF86Launch5" {
                assert_eq!(bind.flags, "--release", "{} fires too early", bind.key);
            }
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
