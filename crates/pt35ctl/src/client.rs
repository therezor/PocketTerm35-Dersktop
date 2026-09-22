//! Minimal line-delimited JSON client for the `pt35d` socket.

use anyhow::{Context, Result};
use pt35_common::ipc::{Request, Response};
use pt35_common::paths;
use std::io::{BufRead, BufReader, Write};

#[cfg(unix)]
pub fn send(request: &Request) -> Result<Response> {
    use std::os::unix::net::UnixStream;

    let path = paths::socket_path();
    let stream = UnixStream::connect(&path).with_context(|| {
        format!("pt35d is not running (no socket at {})", path.display())
    })?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;

    let mut writer = &stream;
    writeln!(writer, "{}", serde_json::to_string(request)?)?;
    writer.flush()?;

    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).context("reading reply from pt35d")?;
    serde_json::from_str(line.trim_end()).with_context(|| format!("bad reply: {line:?}"))
}

#[cfg(not(unix))]
pub fn send(_request: &Request) -> Result<Response> {
    anyhow::bail!("pt35ctl needs a unix socket")
}
