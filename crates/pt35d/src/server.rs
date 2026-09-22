//! The unix-socket server: one thread per connection, line-delimited JSON.
//!
//! Threads rather than an async runtime on purpose — the whole daemon handles a
//! few messages per second and a thread stack costs less than tokio's binary
//! and RSS on a 2 GB Pi.

use anyhow::{Context, Result};
use pt35_common::ipc::{Event, Request, Response};
use pt35_common::paths;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use crate::state::Session;

/// Clients that asked for a live feed (the bar).
#[derive(Default)]
pub struct Subscribers(Mutex<Vec<Sender<Event>>>);

impl Subscribers {
    pub fn add(&self) -> Receiver<Event> {
        let (tx, rx) = channel();
        self.0.lock().expect("subscriber list").push(tx);
        rx
    }

    /// Broadcast, dropping any subscriber whose socket has gone.
    pub fn broadcast(&self, event: &Event) {
        let mut list = self.0.lock().expect("subscriber list");
        list.retain(|tx| tx.send(event.clone()).is_ok());
    }

    pub fn len(&self) -> usize {
        self.0.lock().expect("subscriber list").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct Server {
    listener: UnixListener,
    session: Arc<Mutex<Session>>,
    subscribers: Arc<Subscribers>,
}

impl Server {
    pub fn bind(session: Arc<Mutex<Session>>, subscribers: Arc<Subscribers>) -> Result<Self> {
        let path = paths::socket_path();
        // On a session restart the previous pt35d can still hold the socket for
        // a second or two, until it notices sway is gone. Wait for it rather
        // than leaving the new session with no daemon.
        for attempt in 0..12 {
            if !path.exists() {
                break;
            }
            if UnixStream::connect(&path).is_err() {
                std::fs::remove_file(&path).ok();
                break;
            }
            if attempt == 11 {
                anyhow::bail!("another pt35d is already listening on {}", path.display());
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let listener =
            UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
        log::info!("listening on {}", path.display());
        Ok(Self {
            listener,
            session,
            subscribers,
        })
    }

    pub fn serve(&self) {
        for stream in self.listener.incoming() {
            match stream {
                Ok(stream) => {
                    let session = Arc::clone(&self.session);
                    let subscribers = Arc::clone(&self.subscribers);
                    std::thread::spawn(move || {
                        if let Err(e) = handle_client(stream, session, subscribers) {
                            log::debug!("client ended: {e}");
                        }
                    });
                }
                Err(e) => log::warn!("accept failed: {e}"),
            }
        }
    }
}

fn handle_client(
    stream: UnixStream,
    session: Arc<Mutex<Session>>,
    subscribers: Arc<Subscribers>,
) -> Result<()> {
    let reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(e) => {
                let response = Response::Error {
                    message: format!("bad request: {e}"),
                };
                writeln!(writer, "{}", serde_json::to_string(&response)?)?;
                writer.flush()?;
                continue;
            }
        };

        let subscribe = matches!(request, Request::Subscribe);
        let response = session.lock().expect("session").handle(request);
        writeln!(writer, "{}", serde_json::to_string(&response)?)?;
        writer.flush()?;

        if subscribe {
            // This connection now belongs to the event stream until it drops.
            let events = subscribers.add();
            for event in events {
                if writeln!(writer, "{}", serde_json::to_string(&event)?).is_err() {
                    break;
                }
                if writer.flush().is_err() {
                    break;
                }
            }
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pt35_common::ipc::Status;

    #[test]
    fn subscribers_are_dropped_when_their_receiver_goes_away() {
        let subs = Subscribers::default();
        let rx = subs.add();
        assert_eq!(subs.len(), 1);
        drop(rx);
        subs.broadcast(&Event::Status(Status::default()));
        assert!(subs.is_empty(), "a dead subscriber must not be retained");
    }

    #[test]
    fn broadcast_reaches_live_subscribers() {
        let subs = Subscribers::default();
        let rx = subs.add();
        subs.broadcast(&Event::Status(Status {
            workspace: 4,
            ..Status::default()
        }));
        match rx.recv().unwrap() {
            Event::Status(s) => assert_eq!(s.workspace, 4),
            other => panic!("unexpected {other:?}"),
        }
    }
}
