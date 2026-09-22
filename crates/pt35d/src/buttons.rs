//! What the six face buttons do inside an app.
//!
//! The patched keyboard firmware sends F13-F18 for them, which the default
//! layout reports as XF86Tools and XF86Launch5-9. Nothing else on the device
//! sends those, so the compositor can hold them all the time and the letters
//! keep typing.
//!
//! The binds are dropped while the menu is open, because the menu reads the
//! same keys itself and a compositor binding beats any surface.
//!
//! Enter, Escape and Tab go through `wtype`: sway has no "send key" command.

/// Face button, the keysym the firmware produces, and what sway does with it.
pub const BINDINGS: &[(&str, &str)] = &[
    ("XF86Launch9", "exec wtype -k Return"),           // A
    ("XF86Launch8", "exec wtype -k Escape"),           // B
    ("XF86Launch6", "exec wtype -k Tab"),              // X
    ("XF86Launch7", "exec pt35ctl window fullscreen"), // Y
    ("XF86Tools", "exec pt35ctl window prev"),         // L
    ("XF86Launch5", "exec pt35ctl window next"),       // R
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
        assert!(enable()[0].starts_with("bindsym --no-repeat XF86Launch9 "));
        assert_eq!(disable()[0], "unbindsym XF86Launch9");
    }

    #[test]
    fn no_binding_is_a_character_key() {
        for (key, _) in BINDINGS {
            assert!(
                key.starts_with("XF86"),
                "{key} would stop the keyboard typing it"
            );
        }
    }
}
