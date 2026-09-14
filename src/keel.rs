//! The boat's position from omakeel: its socket and `state` messages,
//! version 1, as omakeel's docs/protocol.md describes them.

use serde_json::Value;
use std::path::PathBuf;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::UnixStream,
    sync::mpsc,
    time::{Duration, sleep},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boat {
    pub lat: f64,
    pub lon: f64,
    /// An `ok` fix; otherwise the last position omakeel knows.
    pub current: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Update {
    /// omakeel's latest fix: None when it has no position.
    Boat(Option<Boat>),
    /// Not connected to omakeel.
    Lost,
    /// omakeel speaks another protocol version.
    Incompatible(u64),
}

/// `$XDG_RUNTIME_DIR/omakeel/keel.sock`.
pub fn default_socket() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute())
        .map(|d| d.join("omakeel").join("keel.sock"))
}

/// What one line from omakeel says about the boat, if anything.
pub fn read(line: &str) -> Option<Update> {
    let m: Value = serde_json::from_str(line).ok()?;
    let v = m.get("v")?.as_u64()?;
    if v != 1 {
        return Some(Update::Incompatible(v));
    }
    if m.get("type")?.as_str()? != "state" {
        return None;
    }
    let fix = m.get("fix")?;
    let status = fix.get("status").and_then(Value::as_str).unwrap_or("none");
    let (lat, lon) = (
        fix.get("lat").and_then(Value::as_f64),
        fix.get("lon").and_then(Value::as_f64),
    );
    let boat = match (lat, lon) {
        (Some(lat), Some(lon))
            if status != "none"
                && (-90.0..=90.0).contains(&lat)
                && (-180.0..=180.0).contains(&lon) =>
        {
            Some(Boat {
                lat,
                lon,
                current: status == "ok",
            })
        }
        _ => None,
    };
    Some(Update::Boat(boat))
}

/// Follows omakeel for as long as the receiver lives, reconnecting every
/// 2 seconds, or every 30 while it speaks another version.
pub async fn follow(path: PathBuf, tx: mpsc::Sender<Update>) {
    loop {
        let mut wait = Duration::from_secs(2);
        if let Ok(stream) = UnixStream::connect(&path).await {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Some(update) = read(&line) else { continue };
                if tx.send(update).await.is_err() {
                    return;
                }
                if let Update::Incompatible(_) = update {
                    wait = Duration::from_secs(30);
                    break;
                }
            }
            if wait.as_secs() == 2 && tx.send(Update::Lost).await.is_err() {
                return;
            }
        }
        if tx.is_closed() {
            return;
        }
        sleep(wait).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_fix_from_omakeel_state() {
        let ok = r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#;
        assert_eq!(
            read(ok),
            Some(Update::Boat(Some(Boat {
                lat: 37.8647,
                lon: -122.3207,
                current: true
            })))
        );
        let stale = ok.replace("\"ok\"", "\"stale\"");
        assert_eq!(
            read(&stale),
            Some(Update::Boat(Some(Boat {
                lat: 37.8647,
                lon: -122.3207,
                current: false
            })))
        );
        let none = r#"{"type":"state","v":1,"fix":{"status":"none"},"sources":[]}"#;
        assert_eq!(read(none), Some(Update::Boat(None)));
        assert_eq!(read(r#"{"type":"targets","v":1,"targets":[]}"#), None);
        assert_eq!(
            read(r#"{"type":"hello","v":2}"#),
            Some(Update::Incompatible(2))
        );
        assert_eq!(read("{}"), None);
        assert_eq!(read("garbage"), None);
    }
}
