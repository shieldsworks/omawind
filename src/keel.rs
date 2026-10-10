//! The boat's position from omakeel: its socket and `state` messages,
//! version 1, as omakeel's docs/protocol.md describes them.

use omakeel_protocol::{Message, ReadError};
use std::path::PathBuf;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    net::UnixStream,
    sync::mpsc,
    time::{Duration, sleep},
};

/// omakeel's longest line, a thousand vessels' worth, is well under this.
const MAX_LINE: u64 = 4 << 20;

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
pub fn default_socket() -> Result<Option<PathBuf>, String> {
    Ok(
        crate::config::xdg_base("XDG_RUNTIME_DIR")?
            .map(|dir| dir.join("omakeel").join("keel.sock")),
    )
}

/// What one line from omakeel says about the boat, if anything.
pub fn read(line: &str) -> Option<Update> {
    match Message::from_line(line) {
        Ok(Some(Message::State { fix, .. })) => {
            let boat = fix
                .position()
                .filter(|position| {
                    (-90.0..=90.0).contains(&position.place.lat)
                        && (-180.0..=180.0).contains(&position.place.lon)
                })
                .map(|position| Boat {
                    lat: position.place.lat,
                    lon: position.place.lon,
                    current: position.current,
                });
            Some(Update::Boat(boat))
        }
        Err(ReadError::Version { found: Some(found) }) => {
            Some(Update::Incompatible(u64::from(found)))
        }
        Ok(Some(Message::Hello { .. } | Message::Targets { .. })) | Ok(None) | Err(_) => None,
    }
}

/// Follows omakeel for as long as the receiver lives, reconnecting every
/// 2 seconds, or every 30 while it speaks another version.
pub async fn follow(path: PathBuf, tx: mpsc::Sender<Update>) {
    loop {
        let mut wait = Duration::from_secs(2);
        if let Ok(stream) = UnixStream::connect(&path).await {
            let mut reader = BufReader::new(stream);
            let mut line = Vec::new();
            loop {
                line.clear();
                // A line this long isn't omakeel's: let go, and try again.
                let got = (&mut reader)
                    .take(MAX_LINE + 1)
                    .read_until(b'\n', &mut line)
                    .await;
                if !matches!(got, Ok(n) if n > 0 && n as u64 <= MAX_LINE) {
                    break;
                }
                let Some(update) = read(&String::from_utf8_lossy(&line)) else {
                    continue;
                };
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

    #[test]
    fn a_position_outside_the_degree_ranges_clears_the_boat() {
        let lat = r#"{"type":"state","v":1,"fix":{"status":"ok","lat":91.0,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#;
        assert_eq!(read(lat), Some(Update::Boat(None)));
        let lon = r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":181.0,"ageSeconds":0},"sources":[]}"#;
        assert_eq!(read(lon), Some(Update::Boat(None)));
    }

    #[test]
    fn a_stale_fix_keeps_the_position() {
        let line = r#"{"type":"state","v":1,"fix":{"status":"stale","lat":37.8647,"lon":-122.3207,"ageSeconds":6},"sources":[]}"#;
        assert_eq!(
            read(line),
            Some(Update::Boat(Some(Boat {
                lat: 37.8647,
                lon: -122.3207,
                current: false,
            })))
        );
    }
}
