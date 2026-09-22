//! Keeps a current [`Status`] by subscribing to pt35d, and survives the daemon
//! restarting underneath it.

use pt35_common::ipc::{Event, Request, Response, Status};
use pt35_common::paths;
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Shared slot the Wayland thread reads on every tick.
#[derive(Clone, Default)]
pub struct StatusFeed {
    inner: Arc<Mutex<Option<Status>>>,
}

impl StatusFeed {
    pub fn get(&self) -> Option<Status> {
        self.inner.lock().expect("status slot").clone()
    }

    fn set(&self, status: Option<Status>) {
        *self.inner.lock().expect("status slot") = status;
    }

    /// Start the background reader. It reconnects for as long as the bar lives:
    /// pt35d may be restarted without taking the bar down with it.
    pub fn spawn(&self) {
        let feed = self.clone();
        std::thread::Builder::new()
            .name("status".into())
            .spawn(move || loop {
                if let Err(e) = feed.stream() {
                    log::debug!("status stream ended: {e}");
                }
                feed.set(None);
                std::thread::sleep(Duration::from_secs(2));
            })
            .expect("spawning the status thread");
    }

    #[cfg(unix)]
    fn stream(&self) -> anyhow::Result<()> {
        use std::os::unix::net::UnixStream;

        let stream = UnixStream::connect(paths::socket_path())?;
        let mut writer = stream.try_clone()?;
        writeln!(writer, "{}", serde_json::to_string(&Request::Subscribe)?)?;
        writer.flush()?;

        for line in BufReader::new(stream).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            // The first line is the Response to Subscribe; the rest are Events.
            if let Ok(Response::Status(status)) = serde_json::from_str::<Response>(&line) {
                self.set(Some(status));
                continue;
            }
            match serde_json::from_str::<Event>(&line) {
                Ok(Event::Status(status)) => self.set(Some(status)),
                Ok(Event::Notification { summary, .. }) => log::info!("notification: {summary}"),
                Err(e) => log::debug!("unparsed line from pt35d: {e}"),
            }
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn stream(&self) -> anyhow::Result<()> {
        anyhow::bail!("the status feed needs a unix socket")
    }
}
