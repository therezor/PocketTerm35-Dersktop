//! A tiny sway/i3 IPC client.
//!
//! The wire format is trivial (`i3-ipc` magic, u32 length, u32 type, payload),
//! so we speak it directly rather than pulling in a crate: no async runtime, no
//! extra ~1 MB of binary, and it keeps working across sway releases.

use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

const MAGIC: &[u8; 6] = b"i3-ipc";

/// sway tags an event reply by setting the top bit of the type word. A reply to
/// a request never has it, so one bit separates the two streams.
const EVENT_BIT: u32 = 0x8000_0000;

/// How long a command connection waits for sway before giving up. A sway that
/// stops answering without closing the socket would otherwise block `read_exact`
/// for ever, and the caller holds the session lock while it waits.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MessageType {
    RunCommand = 0,
    GetWorkspaces = 1,
    Subscribe = 2,
    GetTree = 4,
}

pub struct Sway {
    stream: UnixStream,
}

impl Sway {
    pub fn socket_path() -> Result<PathBuf> {
        if let Some(p) = std::env::var_os("SWAYSOCK") {
            return Ok(PathBuf::from(p));
        }
        if let Some(p) = std::env::var_os("I3SOCK") {
            return Ok(PathBuf::from(p));
        }
        bail!("SWAYSOCK is not set — pt35d must run inside a sway session")
    }

    /// A connection for commands and queries. It times out, because every caller
    /// is holding a lock while it waits.
    pub fn connect() -> Result<Self> {
        Self::open(Some(COMMAND_TIMEOUT))
    }

    /// A connection for the event stream. No timeout: it is idle for minutes at
    /// a time and a timeout would look like a dropped socket.
    pub fn connect_untimed() -> Result<Self> {
        Self::open(None)
    }

    fn open(timeout: Option<Duration>) -> Result<Self> {
        let path = Self::socket_path()?;
        let stream = UnixStream::connect(&path)
            .with_context(|| format!("connecting to sway at {}", path.display()))?;
        stream.set_read_timeout(timeout)?;
        Ok(Self { stream })
    }

    /// Send a message and return the raw JSON payload of the reply.
    pub fn request(&mut self, kind: MessageType, payload: &str) -> Result<String> {
        self.send(kind, payload)?;
        let (kind, body) = self.read_message()?;
        if kind & EVENT_BIT != 0 {
            bail!("sway answered a request with an event ({kind:#x})");
        }
        Ok(body)
    }

    fn send(&mut self, kind: MessageType, payload: &str) -> Result<()> {
        let mut header = Vec::with_capacity(14 + payload.len());
        header.extend_from_slice(MAGIC);
        header.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
        header.extend_from_slice(&(kind as u32).to_ne_bytes());
        header.extend_from_slice(payload.as_bytes());
        self.stream
            .write_all(&header)
            .context("writing to the sway socket")?;
        self.stream.flush()?;
        Ok(())
    }

    /// Ask for an event stream on this connection. Nothing else may be sent on
    /// it afterwards: replies and events would interleave.
    pub fn subscribe(&mut self, events: &[&str]) -> Result<()> {
        let payload = serde_json::to_string(events)?;
        self.send(MessageType::Subscribe, &payload)?;
        let (_, body) = self.read_message()?;
        let parsed: serde_json::Value = serde_json::from_str(&body)?;
        if parsed.get("success").and_then(|v| v.as_bool()) != Some(true) {
            bail!("sway refused the subscription: {body}");
        }
        Ok(())
    }

    /// Block until the next event. Only meaningful after [`Sway::subscribe`].
    pub fn next_event(&mut self) -> Result<(u32, String)> {
        loop {
            let (kind, body) = self.read_message()?;
            if kind & EVENT_BIT != 0 {
                return Ok((kind & !EVENT_BIT, body));
            }
        }
    }

    /// One frame: the raw type word and the JSON payload. The type is returned
    /// whole rather than as a [`MessageType`], because an event sets a bit no
    /// variant of that enum can hold.
    fn read_message(&mut self) -> Result<(u32, String)> {
        let mut head = [0u8; 14];
        self.stream
            .read_exact(&mut head)
            .context("short read from the sway socket")?;
        if &head[..6] != MAGIC {
            bail!("sway sent a reply without the i3-ipc magic");
        }
        let len = u32::from_ne_bytes(head[6..10].try_into().unwrap()) as usize;
        let kind = u32::from_ne_bytes(head[10..14].try_into().unwrap());
        let mut body = vec![0u8; len];
        self.stream.read_exact(&mut body)?;
        Ok((kind, String::from_utf8(body)?))
    }

    /// Run one or more sway commands (`, `-separated, as sway expects).
    pub fn command(&mut self, cmd: &str) -> Result<()> {
        let reply = self.request(MessageType::RunCommand, cmd)?;
        // Reply is [{"success":true}, ...]; surface the first failure verbatim.
        let parsed: serde_json::Value = serde_json::from_str(&reply)?;
        if let Some(items) = parsed.as_array() {
            for item in items {
                if item.get("success").and_then(|v| v.as_bool()) == Some(false) {
                    let err = item
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("sway rejected the command");
                    bail!("{cmd:?}: {err}");
                }
            }
        }
        Ok(())
    }

    /// Focused workspace number and the focused window's name, if any.
    pub fn focus(&mut self) -> Result<(u8, Option<String>)> {
        let reply = self.request(MessageType::GetWorkspaces, "")?;
        let workspaces: serde_json::Value = serde_json::from_str(&reply)?;
        let focused = workspaces
            .as_array()
            .and_then(|list| list.iter().find(|w| w["focused"].as_bool() == Some(true)));
        let number = focused
            .and_then(|w| w["num"].as_i64())
            .unwrap_or(1)
            .clamp(0, 9) as u8;

        let tree: serde_json::Value =
            serde_json::from_str(&self.request(MessageType::GetTree, "")?)?;
        Ok((number, focused_window_name(&tree)))
    }
}

/// One open window, as the dock and the switcher need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub id: i64,
    pub workspace: u8,
    /// app_id on Wayland, class on XWayland.
    pub app: String,
    pub title: String,
    pub focused: bool,
    /// Floating windows are dialogs. They never tile, so the shell leaves them
    /// where they are.
    pub floating: bool,
}

/// Flatten a sway tree into the windows it holds, in workspace order.
///
/// The scratchpad is skipped. sway gives `__i3_scratch` `num: -1`, which would
/// clamp to workspace 0 and sort a hidden window to the front of the dock.
pub fn windows(node: &serde_json::Value) -> Vec<Window> {
    fn walk(node: &serde_json::Value, workspace: u8, floating: bool, out: &mut Vec<Window>) {
        let kind = node["type"].as_str().unwrap_or("");
        if kind == "workspace" && node["num"].as_i64().unwrap_or(0) < 1 {
            return;
        }
        let workspace = if kind == "workspace" {
            node["num"].as_i64().unwrap_or(0).clamp(0, 9) as u8
        } else {
            workspace
        };
        let is_window = node.get("pid").is_some()
            || node["app_id"].as_str().is_some()
            || node["window"].as_i64().is_some();
        if is_window && kind != "workspace" {
            if let Some(id) = node["id"].as_i64() {
                out.push(Window {
                    id,
                    workspace,
                    app: node["app_id"]
                        .as_str()
                        .or_else(|| node["window_properties"]["class"].as_str())
                        .unwrap_or("")
                        .to_string(),
                    title: node["name"].as_str().unwrap_or("").to_string(),
                    focused: node["focused"].as_bool() == Some(true),
                    floating,
                });
            }
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = node[key].as_array() {
                let floating = floating || key == "floating_nodes";
                for child in children {
                    walk(child, workspace, floating, out);
                }
            }
        }
    }

    let mut out = Vec::new();
    walk(node, 0, false, &mut out);
    out.sort_by_key(|w| (w.workspace, w.id));
    out
}

/// Walk a sway tree and return the name of the focused node.
pub fn focused_window_name(node: &serde_json::Value) -> Option<String> {
    if node["focused"].as_bool() == Some(true) {
        if let Some(name) = node["name"].as_str() {
            if node["type"].as_str() != Some("workspace") && node["type"].as_str() != Some("output")
            {
                return Some(name.to_string());
            }
        }
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = node[key].as_array() {
            for child in children {
                if let Some(found) = focused_window_name(child) {
                    return Some(found);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_focused_window_name() {
        let tree: serde_json::Value = serde_json::from_str(
            r#"{
              "type":"root","focused":false,"name":"root","nodes":[
                {"type":"output","focused":false,"name":"HDMI-A-1","nodes":[
                  {"type":"workspace","focused":false,"name":"1","nodes":[
                    {"type":"con","focused":false,"name":"foot"},
                    {"type":"con","focused":true,"name":"helix — main.rs"}
                  ],"floating_nodes":[]}
                ],"floating_nodes":[]}
              ],"floating_nodes":[]}"#,
        )
        .unwrap();
        assert_eq!(
            focused_window_name(&tree).as_deref(),
            Some("helix — main.rs")
        );
    }

    #[test]
    fn lists_windows_with_their_workspace() {
        let tree: serde_json::Value = serde_json::from_str(
            r#"{"type":"root","name":"root","nodes":[
                 {"type":"output","name":"HDMI-A-1","nodes":[
                   {"type":"workspace","num":1,"name":"1","nodes":[
                     {"type":"con","id":12,"name":"foot","app_id":"foot","pid":9,"focused":false}
                   ],"floating_nodes":[]},
                   {"type":"workspace","num":2,"name":"2","nodes":[
                     {"type":"con","id":34,"name":"Home","window":5,
                      "window_properties":{"class":"Pcmanfm"},"pid":10,"focused":true}
                   ],"floating_nodes":[]}
                 ],"floating_nodes":[]}
               ],"floating_nodes":[]}"#,
        )
        .unwrap();
        let list = windows(&tree);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].app, "foot");
        assert_eq!(list[0].workspace, 1);
        assert_eq!(list[1].app, "Pcmanfm");
        assert!(list[1].focused);
    }

    #[test]
    fn the_scratchpad_is_not_part_of_the_dock() {
        let tree: serde_json::Value = serde_json::from_str(
            r#"{"type":"root","name":"root","nodes":[
                 {"type":"output","name":"__i3","nodes":[
                   {"type":"workspace","num":-1,"name":"__i3_scratch","nodes":[
                     {"type":"con","id":7,"name":"hidden","app_id":"foot","pid":3,"focused":false}
                   ],"floating_nodes":[]}
                 ],"floating_nodes":[]},
                 {"type":"output","name":"HDMI-A-1","nodes":[
                   {"type":"workspace","num":1,"name":"1","nodes":[
                     {"type":"con","id":12,"name":"foot","app_id":"foot","pid":9,"focused":true}
                   ],"floating_nodes":[]}
                 ],"floating_nodes":[]}
               ],"floating_nodes":[]}"#,
        )
        .unwrap();
        let list = windows(&tree);
        assert_eq!(list.len(), 1, "a scratchpad window is not an open window");
        assert_eq!(list[0].id, 12);
    }

    #[test]
    fn an_event_frame_is_told_apart_from_a_reply() {
        // sway sets the top bit of the type word on an event. The enum cannot
        // hold that, which is why read_message hands back the raw u32.
        assert_eq!(EVENT_BIT & MessageType::GetTree as u32, 0);
        assert_eq!((EVENT_BIT | 3) & !EVENT_BIT, 3, "window events are type 3");
        assert_ne!(EVENT_BIT | 3, 3);
    }

    #[test]
    fn ignores_a_focused_workspace_with_no_windows() {
        let tree: serde_json::Value = serde_json::from_str(
            r#"{"type":"workspace","focused":true,"name":"3","nodes":[],"floating_nodes":[]}"#,
        )
        .unwrap();
        assert_eq!(focused_window_name(&tree), None);
    }
}
