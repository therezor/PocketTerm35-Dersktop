//! A tiny sway/i3 IPC client.
//!
//! The wire format is trivial (`i3-ipc` magic, u32 length, u32 type, payload),
//! so we speak it directly rather than pulling in a crate: no async runtime, no
//! extra ~1 MB of binary, and it keeps working across sway releases.

use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

const MAGIC: &[u8; 6] = b"i3-ipc";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MessageType {
    RunCommand = 0,
    GetWorkspaces = 1,
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

    pub fn connect() -> Result<Self> {
        let path = Self::socket_path()?;
        let stream = UnixStream::connect(&path)
            .with_context(|| format!("connecting to sway at {}", path.display()))?;
        Ok(Self { stream })
    }

    /// Send a message and return the raw JSON payload of the reply.
    pub fn request(&mut self, kind: MessageType, payload: &str) -> Result<String> {
        let mut header = Vec::with_capacity(14 + payload.len());
        header.extend_from_slice(MAGIC);
        header.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
        header.extend_from_slice(&(kind as u32).to_ne_bytes());
        header.extend_from_slice(payload.as_bytes());
        self.stream
            .write_all(&header)
            .context("writing to the sway socket")?;
        self.stream.flush()?;
        self.read_reply()
    }

    fn read_reply(&mut self) -> Result<String> {
        let mut head = [0u8; 14];
        self.stream
            .read_exact(&mut head)
            .context("short read from the sway socket")?;
        if &head[..6] != MAGIC {
            bail!("sway sent a reply without the i3-ipc magic");
        }
        let len = u32::from_ne_bytes(head[6..10].try_into().unwrap()) as usize;
        let mut body = vec![0u8; len];
        self.stream.read_exact(&mut body)?;
        Ok(String::from_utf8(body)?)
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
    fn ignores_a_focused_workspace_with_no_windows() {
        let tree: serde_json::Value = serde_json::from_str(
            r#"{"type":"workspace","focused":true,"name":"3","nodes":[],"floating_nodes":[]}"#,
        )
        .unwrap();
        assert_eq!(focused_window_name(&tree), None);
    }
}
