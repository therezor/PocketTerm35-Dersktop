//! Packaging-only crate: `cargo deb -p pt35-desktop` builds the single .deb
//! that carries every pt35 binary, the sway/keyd/foot configuration and the
//! Waveshare device-tree overlay. There is no code here on purpose.

/// The version the installer reports and the release artefacts are named after.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert!(!super::VERSION.is_empty());
    }
}
