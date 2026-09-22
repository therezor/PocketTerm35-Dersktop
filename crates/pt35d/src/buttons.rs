//! Button mode.
//!
//! Six of the twelve controls are the literal letters a b x y l r. Inside an
//! app they type, which is right in a terminal and wrong everywhere else. In
//! button mode the compositor grabs them and they act as buttons instead, by
//! synthesising the key the app expects.
//!
//! The synthesis goes through `wtype`, because sway has no "send key" command.

/// What each face button does while button mode is on.
pub const BINDINGS: &[(&str, &str)] = &[
    ("a", "exec wtype -k Return"),
    ("b", "exec wtype -k Escape"),
    ("x", "exec wtype -k Tab"),
    ("y", "exec pt35ctl window fullscreen"),
    ("l", "workspace prev_on_output"),
    ("r", "workspace next_on_output"),
];

/// sway commands that turn the grab on.
pub fn enable() -> Vec<String> {
    BINDINGS
        .iter()
        .map(|(key, action)| format!("bindsym --no-repeat {key} {action}"))
        .collect()
}

/// sway commands that hand the keys back to the application.
pub fn disable() -> Vec<String> {
    BINDINGS
        .iter()
        .map(|(key, _)| format!("unbindsym {key}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_face_button_is_bound_and_unbound() {
        assert_eq!(enable().len(), 6);
        assert_eq!(disable().len(), 6);
        assert!(enable()[0].starts_with("bindsym --no-repeat a "));
        assert_eq!(disable()[0], "unbindsym a");
    }

    #[test]
    fn the_letters_are_the_six_face_buttons() {
        let keys: Vec<&str> = BINDINGS.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, ["a", "b", "x", "y", "l", "r"]);
    }
}
