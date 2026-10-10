//! The boat's position from omakeel: its socket and `state` messages,
//! version 1, as omakeel's docs/protocol.md describes them.

use omakeel_protocol::Message;
use serde_json::Value;
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
        Ok(Some(Message::Hello { .. } | Message::Targets { .. })) | Ok(None) => None,
        // `None` keeps the last fix. A line the crate rejects still has to
        // clear the boat, or name a version, the way the field walk did.
        Err(_) => read_rejected(line),
    }
}

fn read_rejected(line: &str) -> Option<Update> {
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

    /// The field walk from before `omakeel-protocol`. The table is its result.
    fn read_before_protocol(line: &str) -> Option<Update> {
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

    #[test]
    fn read_matches_the_walk_it_replaced() {
        let here = Boat {
            lat: 37.8647,
            lon: -122.3207,
            current: true,
        };
        let last = Boat {
            current: false,
            ..here
        };
        let kept = |boat: Boat| Some(Update::Boat(Some(boat)));
        let cleared = Some(Update::Boat(None));
        let ok = r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#;
        let mut cases: Vec<(&str, String, Option<Update>)> = [
            (
                "unknown fix status keeps a non-current boat",
                r#"{"type":"state","v":1,"fix":{"status":"survey","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                kept(last),
            ),
            (
                "unknown fix status without coordinates clears the boat",
                r#"{"type":"state","v":1,"fix":{"status":"survey"},"sources":[]}"#,
                cleared,
            ),
            (
                "unknown fix status outside the degree ranges clears the boat",
                r#"{"type":"state","v":1,"fix":{"status":"survey","lat":91.0,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                cleared,
            ),
            (
                "unknown source status is ignored",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[{"name":"gps","status":"asleep","sentences":1,"rejected":0}]}"#,
                kept(here),
            ),
            (
                "a source missing its name is ignored",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[{"status":"ok","sentences":0,"rejected":0}]}"#,
                kept(here),
            ),
            (
                "missing sources still carries the fix",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0}}"#,
                kept(here),
            ),
            (
                "sources that are not an array are ignored",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":{}}"#,
                kept(here),
            ),
            (
                "a none fix with no sources array clears the boat",
                r#"{"type":"state","v":1,"fix":{"status":"none"}}"#,
                cleared,
            ),
            (
                "extra fields are ignored",
                r#"{"type":"state","v":1,"note":"bay","fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0,"receiver":"gps"},"sources":[]}"#,
                kept(here),
            ),
            (
                "a null extra field is ignored",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[],"note":null}"#,
                kept(here),
            ),
            (
                "a null age is ignored",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":null},"sources":[]}"#,
                kept(here),
            ),
            (
                "an age of 0.0 is ignored",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0.0},"sources":[]}"#,
                kept(here),
            ),
            (
                "a fractional age is ignored",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":1.5},"sources":[]}"#,
                kept(here),
            ),
            (
                "a satellite count the crate cannot store is ignored",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0,"satellites":300},"sources":[]}"#,
                kept(here),
            ),
            (
                "a non-numeric speed is ignored",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0,"sogKn":"fast"},"sources":[]}"#,
                kept(here),
            ),
            (
                "v as a string is not a version",
                r#"{"type":"state","v":"1","fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                None,
            ),
            (
                "v as a fraction is not a version",
                r#"{"type":"state","v":1.5,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                None,
            ),
            (
                "v as a negative number is not a version",
                r#"{"type":"state","v":-1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                None,
            ),
            (
                "v written as 1.0 is not an integer version",
                r#"{"type":"state","v":1.0,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                None,
            ),
            (
                "v past u32::MAX is another version",
                r#"{"type":"state","v":4294967296,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                Some(Update::Incompatible(4_294_967_296)),
            ),
            (
                "v of 0 is another version",
                r#"{"type":"hello","v":0}"#,
                Some(Update::Incompatible(0)),
            ),
            (
                "v of 2 with a null field is another version",
                r#"{"type":"hello","v":2,"keel":null}"#,
                Some(Update::Incompatible(2)),
            ),
            (
                "a source missing its sentence count is ignored",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[{"name":"gps","status":"ok"}]}"#,
                kept(here),
            ),
            (
                "status none with only a latitude clears the boat",
                r#"{"type":"state","v":1,"fix":{"status":"none","lat":37.8647},"sources":[]}"#,
                cleared,
            ),
            (
                "lat without lon clears the boat",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"ageSeconds":0},"sources":[]}"#,
                cleared,
            ),
            (
                "lon without lat clears the boat",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                cleared,
            ),
            (
                "coordinates stored as strings clear the boat",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":"37.8647","lon":"-122.3207","ageSeconds":0},"sources":[]}"#,
                cleared,
            ),
            (
                "nofix with coordinates keeps a non-current boat",
                r#"{"type":"state","v":1,"fix":{"status":"nofix","lat":37.8647,"lon":-122.3207,"ageSeconds":4},"sources":[]}"#,
                kept(last),
            ),
            (
                "nofix with coordinates and no age keeps a non-current boat",
                r#"{"type":"state","v":1,"fix":{"status":"nofix","lat":37.8647,"lon":-122.3207},"sources":[]}"#,
                kept(last),
            ),
            (
                "nofix without coordinates clears the boat",
                r#"{"type":"state","v":1,"fix":{"status":"nofix"},"sources":[]}"#,
                cleared,
            ),
            (
                "nofix outside the degree ranges clears the boat",
                r#"{"type":"state","v":1,"fix":{"status":"nofix","lat":37.8647,"lon":181.0,"ageSeconds":4},"sources":[]}"#,
                cleared,
            ),
            (
                "an ok fix with no age keeps a current boat",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":37.8647,"lon":-122.3207},"sources":[]}"#,
                kept(here),
            ),
            (
                "a missing status clears the boat",
                r#"{"type":"state","v":1,"fix":{"lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                cleared,
            ),
            (
                "status none with coordinates clears the boat",
                r#"{"type":"state","v":1,"fix":{"status":"none","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                cleared,
            ),
            (
                "status NONE is not none, so the coordinates stay",
                r#"{"type":"state","v":1,"fix":{"status":"NONE","lat":37.8647,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                kept(last),
            ),
            (
                "a null fix clears the boat",
                r#"{"type":"state","v":1,"fix":null,"sources":[]}"#,
                cleared,
            ),
            (
                "a missing fix is not a state the boat can use",
                r#"{"type":"state","v":1,"sources":[]}"#,
                None,
            ),
            (
                "a fix that is not an object clears the boat",
                r#"{"type":"state","v":1,"fix":"ok","sources":[]}"#,
                cleared,
            ),
            (
                "NaN is not JSON",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":NaN,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                None,
            ),
            (
                "Infinity is not JSON",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":Infinity,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                None,
            ),
            (
                "a coordinate spelled NaN clears the boat",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":"NaN","lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                cleared,
            ),
            (
                "a coordinate past what f64 can hold is not JSON",
                r#"{"type":"state","v":1,"fix":{"status":"ok","lat":1e9999,"lon":-122.3207,"ageSeconds":0},"sources":[]}"#,
                None,
            ),
        ]
        .into_iter()
        .map(|(name, line, expected)| (name, line.to_string(), expected))
        .collect();
        cases.push((
            "a trailing newline is still the fix",
            format!("{ok}\n"),
            kept(here),
        ));
        let mut misses = Vec::new();
        for (name, line, expected) in &cases {
            let before = read_before_protocol(line);
            let after = read(line);
            if before != *expected {
                misses.push(format!(
                    "{name}: pasted walk {before:?}, table {expected:?}"
                ));
            }
            if after != *expected {
                misses.push(format!("{name}: read {after:?}, table {expected:?}"));
            }
        }
        assert!(misses.is_empty(), "{}", misses.join("\n"));
    }
}
