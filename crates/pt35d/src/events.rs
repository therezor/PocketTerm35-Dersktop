//! The sway event stream.
//!
//! Polling the tree every two seconds is what made a closed window keep its
//! slot in the dock. sway will tell us instead: one connection of its own, kept
//! apart from the one commands go out on, because replies and events share a
//! socket and would interleave.

use anyhow::Result;
use pt35_common::ipc::Event;
use pt35_common::sway::Sway;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::server::Subscribers;
use crate::state::Session;

/// What we ask sway for. `workspace` covers the focus moving between apps,
/// `window` covers everything else.
const EVENTS: &[&str] = &["window", "workspace"];

/// `window::title` fires on every prompt redraw. A build scrolling past would
/// otherwise cost one `get_tree` per line.
const COALESCE: Duration = Duration::from_millis(50);

/// Changes worth a redraw. Anything else (`urgent`, `mark`, `rename`) leaves
/// the dock looking the same.
const INTERESTING: &[&str] = &[
    "new",
    "close",
    "focus",
    "move",
    "floating",
    "fullscreen_mode",
    "title",
    "init",
    "empty",
];

pub fn run(session: Arc<Mutex<Session>>, subscribers: Arc<Subscribers>) {
    let mut last = Instant::now() - COALESCE;
    loop {
        if let Err(e) = stream(&session, &subscribers, &mut last) {
            log::debug!("sway event stream ended: {e}");
        }
        // sway going away is the session ending; the poll thread notices and
        // takes the process down with it. Until then, try again.
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn stream(
    session: &Arc<Mutex<Session>>,
    subscribers: &Arc<Subscribers>,
    last: &mut Instant,
) -> Result<()> {
    let mut sway = Sway::connect_untimed()?;
    sway.subscribe(EVENTS)?;
    log::info!("subscribed to sway {EVENTS:?} events");
    loop {
        let (_, body) = sway.next_event()?;
        if !worth_refreshing(&body) {
            continue;
        }
        if last.elapsed() < COALESCE {
            continue;
        }
        *last = Instant::now();
        let (status, notes) = {
            let mut session = session.lock().expect("session");
            session.refresh_windows();
            (session.status.clone(), session.take_notifications())
        };
        if !subscribers.is_empty() {
            subscribers.broadcast(&Event::Status(status));
            for note in &notes {
                subscribers.broadcast(note);
            }
        }
    }
}

/// Every sway event carries a `change` saying what happened.
fn worth_refreshing(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        // An event we cannot read is still an event. Better a redraw than a
        // missed close.
        return true;
    };
    match value["change"].as_str() {
        Some(change) => INTERESTING.contains(&change),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_close_is_worth_a_redraw_and_a_mark_is_not() {
        assert!(worth_refreshing(
            r#"{"change":"close","container":{"id":4}}"#
        ));
        assert!(worth_refreshing(r#"{"change":"new"}"#));
        assert!(worth_refreshing(r#"{"change":"title"}"#));
        assert!(!worth_refreshing(r#"{"change":"mark"}"#));
        assert!(!worth_refreshing(r#"{"change":"urgent"}"#));
    }

    #[test]
    fn an_unreadable_event_still_refreshes() {
        assert!(worth_refreshing("not json"));
        assert!(worth_refreshing("{}"));
    }
}
