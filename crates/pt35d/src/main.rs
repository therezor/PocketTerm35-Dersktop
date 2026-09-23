//! `pt35d` — the pt35-desktop session daemon.
//!
//! Owns everything that is not drawing: sway IPC, power and battery, volume and
//! backlight, the menu and pointer processes, hooks, and the status feed the
//! bar subscribes to.

use anyhow::Result;
use pt35_common::ipc::Event;
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod audio;
mod backlight;
mod events;
mod hardware;
mod hooks;
mod modes;
mod recents;
mod server;
mod state;
mod vkbd;

/// How often the sampled hardware is re-read. 2 s is invisible on a clock that
/// shows minutes and costs a few sysfs reads.
///
/// The window list does not wait for this: `events` watches sway directly. The
/// poll re-reads it anyway, cheaply, as a safety net for a dropped event.
const POLL: Duration = Duration::from_secs(2);

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let session = Arc::new(Mutex::new(state::Session::new()));
    let subscribers = Arc::new(server::Subscribers::default());

    hooks::fire("startup", &[]);

    {
        let session = Arc::clone(&session);
        let subscribers = Arc::clone(&subscribers);
        std::thread::Builder::new()
            .name("poll".into())
            .spawn(move || loop {
                {
                    let mut session = session.lock().expect("session");
                    if session.compositor_gone() {
                        log::info!("sway is gone, exiting");
                        std::process::exit(0);
                    }
                    session.refresh();
                    let notes = session.take_notifications();
                    if !subscribers.is_empty() {
                        subscribers.broadcast(&Event::Status(session.status.clone()));
                        for note in &notes {
                            subscribers.broadcast(note);
                        }
                    }
                }
                std::thread::sleep(POLL);
            })?;
    }

    {
        let session = Arc::clone(&session);
        let subscribers = Arc::clone(&subscribers);
        std::thread::Builder::new()
            .name("sway-events".into())
            .spawn(move || events::run(session, subscribers))?;
    }

    let server = server::Server::bind(session, subscribers)?;
    server.serve();
    Ok(())
}
