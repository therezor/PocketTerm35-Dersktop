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
mod hardware;
mod hooks;
mod server;
mod state;

/// How often the sampled hardware is re-read. 2 s is invisible on a clock that
/// shows minutes and costs a few sysfs reads.
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
                    session.refresh();
                    if !subscribers.is_empty() {
                        subscribers.broadcast(&Event::Status(session.status.clone()));
                    }
                }
                std::thread::sleep(POLL);
            })?;
    }

    let server = server::Server::bind(session, subscribers)?;
    server.serve();
    Ok(())
}
