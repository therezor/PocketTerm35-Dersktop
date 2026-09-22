//! The two input modes.
//!
//! Twelve controls, two things to do with them. In Buttons mode the D-pad is
//! the arrow keys and A is Enter; in Mouse mode the D-pad moves the cursor and
//! A is a left click. Select switches, and is bound in the sway config so
//! neither mode can lose it. L and R switch app in both.
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

/// What the config asks for otherwise: one press of the D-pad, one row.
const NAV_REPEAT_DELAY: u32 = 500;
const NAV_REPEAT_RATE: u32 = 8;

/// Face buttons, by the keysym the firmware sends for each.
const A: &str = "XF86Launch9";
const B: &str = "XF86Launch8";
const X: &str = "XF86Launch6";
const Y: &str = "XF86Launch7";
const L: &str = "XF86Tools";
const R: &str = "XF86Launch5";

/// Every key either mode grabs. Unbinding this list is what leaves the keyboard
/// alone while the menu is up.
const GRABBED: &[&str] = &[A, B, X, Y, L, R, "Up", "Down", "Left", "Right"];

/// The shoulders switch app whichever mode you are in.
fn common_binds() -> Vec<String> {
    [
        (L, "exec pt35ctl window prev"),
        (R, "exec pt35ctl window next"),
    ]
    .iter()
    .map(|(key, action)| format!("bindsym --no-repeat {key} {action}"))
    .collect()
}

fn buttons_binds() -> Vec<String> {
    [
        (A, "exec wtype -k Return"),
        (B, "exec wtype -k Escape"),
        (X, "exec wtype -k Tab"),
        (Y, "exec pt35ctl window fullscreen"),
    ]
    .iter()
    .map(|(key, action)| format!("bindsym {key} {action}"))
    .collect()
}

fn mouse_binds(step: i32) -> Vec<String> {
    let mut out = Vec::new();
    for (key, dx, dy) in [
        ("Up", 0, -step),
        ("Down", 0, step),
        ("Left", -step, 0),
        ("Right", step, 0),
    ] {
        out.push(format!("bindsym {key} seat {SEAT} cursor move {dx} {dy}"));
    }
    // Press and release separately, so holding A drags and holding X scrolls.
    for (key, button) in [(A, 1), (B, 3), (X, 4), (Y, 5)] {
        out.push(format!(
            "bindsym --no-repeat {key} seat {SEAT} cursor press button{button}"
        ));
        out.push(format!(
            "bindsym --release {key} seat {SEAT} cursor release button{button}"
        ));
    }
    out
}

/// sway commands that put the device in `mode`, from whatever it was in.
pub fn apply(mode: InputMode, pointer: &Pointer) -> Vec<String> {
    let mut out: Vec<String> = GRABBED
        .iter()
        .map(|key| format!("unbindsym {key}"))
        .collect();
    match mode {
        InputMode::Buttons => {
            out.extend(common_binds());
            out.extend(buttons_binds());
            out.push(format!(
                "input type:keyboard repeat_delay {NAV_REPEAT_DELAY}"
            ));
            out.push(format!("input type:keyboard repeat_rate {NAV_REPEAT_RATE}"));
            out.push(format!("seat {SEAT} hide_cursor 1500"));
        }
        InputMode::Mouse => {
            out.extend(common_binds());
            out.extend(mouse_binds(pointer.step));
            out.push(format!(
                "input type:keyboard repeat_delay {}",
                pointer.repeat_delay
            ));
            out.push(format!(
                "input type:keyboard repeat_rate {}",
                pointer.repeat_rate
            ));
            // The cursor is hidden after 1.5s of stillness, which is wrong when
            // the D-pad is what moves it.
            out.push(format!("seat {SEAT} hide_cursor 0"));
        }
    }
    out
}

/// Hand every grabbed key back. The menu reads them itself, and a sway binding
/// beats any surface.
pub fn release() -> Vec<String> {
    let mut out: Vec<String> = GRABBED
        .iter()
        .map(|key| format!("unbindsym {key}"))
        .collect();
    out.push(format!(
        "input type:keyboard repeat_delay {NAV_REPEAT_DELAY}"
    ));
    out.push(format!("input type:keyboard repeat_rate {NAV_REPEAT_RATE}"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_mode_drops_the_other_mode_first() {
        let commands = apply(InputMode::Mouse, &Pointer::default());
        for key in GRABBED {
            let unbind = format!("unbindsym {key}");
            let at = commands.iter().position(|c| *c == unbind).expect("unbound");
            let bind = commands
                .iter()
                .position(|c| c.contains(&format!(" {key} ")));
            assert!(
                bind.is_none() || bind.unwrap() > at,
                "{key} rebound too early"
            );
        }
    }

    #[test]
    fn no_grabbed_key_is_a_character() {
        for key in GRABBED {
            assert!(
                key.starts_with("XF86") || matches!(*key, "Up" | "Down" | "Left" | "Right"),
                "{key} would stop the keyboard typing it"
            );
        }
    }

    #[test]
    fn a_and_b_are_the_mouse_buttons() {
        let mouse = mouse_binds(16);
        assert!(mouse
            .iter()
            .any(|c| c == "bindsym --no-repeat XF86Launch9 seat seat0 cursor press button1"));
        assert!(mouse
            .iter()
            .any(|c| c == "bindsym --release XF86Launch8 seat seat0 cursor release button3"));
    }

    #[test]
    fn the_shoulders_switch_app_in_both_modes() {
        for mode in [InputMode::Buttons, InputMode::Mouse] {
            let commands = apply(mode, &Pointer::default());
            for key in [L, R] {
                assert!(
                    commands.iter().any(|c| c
                        .starts_with(&format!("bindsym --no-repeat {key} exec pt35ctl window"))),
                    "{key} does not switch app in {mode:?}"
                );
            }
        }
    }

    #[test]
    fn releasing_leaves_nothing_bound() {
        assert!(
            release()
                .iter()
                .filter(|c| c.starts_with("bindsym"))
                .count()
                == 0
        );
    }
}
