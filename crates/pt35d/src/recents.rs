//! What the launcher learned about you.
//!
//! `apps.toml` is a `BTreeMap`, so the launcher was sorted by app id, which is
//! an order nobody chose. The five things you opened last go on top instead.

use std::io::Write;
use std::path::PathBuf;

/// How many entries are kept, and how many the launcher shows.
pub const KEEP: usize = 5;

fn path() -> PathBuf {
    pt35_common::paths::recents_path()
}

/// The ids most recently launched, newest first.
pub fn read() -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path()) else {
        return Vec::new();
    };
    parse(&text)
}

/// Put `id` at the front, drop any older mention of it, and keep [`KEEP`].
pub fn record(id: &str) {
    let mut list = read();
    list.retain(|seen| seen != id);
    list.insert(0, id.to_string());
    list.truncate(KEEP);
    let path = path();
    if let Some(dir) = path.parent() {
        // Losing the recents list is not worth failing a launch over.
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    if let Ok(mut file) = std::fs::File::create(&path) {
        let _ = writeln!(file, "{}", list.join("\n"));
    }
}

fn parse(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || out.iter().any(|seen| seen == line) {
            continue;
        }
        out.push(line.to_string());
        if out.len() == KEEP {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_no_repeats_and_capped() {
        let text = "editor\nbrowser\neditor\nfiles\n\nmusic\nterminal\nshell\n";
        let list = parse(text);
        assert_eq!(list, ["editor", "browser", "files", "music", "terminal"]);
        assert_eq!(list.len(), KEEP);
    }

    #[test]
    fn an_absent_file_is_not_an_error() {
        assert!(parse("").is_empty());
    }
}
