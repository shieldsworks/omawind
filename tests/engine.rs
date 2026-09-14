//! The engine end to end: a cached run in, an app's view out, the boat's
//! position from a stand-in omakeel. The clock is pinned inside the run.

use omawind::{engine, time};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    net::{UnixListener, UnixStream, unix::OwnedWriteHalf},
    time::{Duration, sleep, timeout},
};

const RUN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/hrrr/2026091403"
);

fn four_utc() -> i64 {
    time::unix(2026, 9, 14, 4, 0, 0)
}

fn six_utc() -> i64 {
    time::unix(2026, 9, 14, 6, 0, 0)
}

/// A scratch folder holding a copy of the fixture run as the cache.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("omawind-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let run = dir.join("cache/hrrr/2026091403");
    std::fs::create_dir_all(&run).unwrap();
    for f in ["f00.grib2", "f01.grib2", "f02.grib2", "region"] {
        std::fs::copy(Path::new(RUN).join(f), run.join(f)).unwrap();
    }
    dir
}

fn config(dir: &Path, clock: fn() -> i64) -> engine::Config {
    engine::Config {
        socket: dir.join("wind.sock"),
        keel: Some(dir.join("keel.sock")),
        cache: dir.join("cache"),
        settings: dir.join("config.toml"),
        fetch: false,
        clock,
    }
}

struct App {
    lines: Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
    writer: OwnedWriteHalf,
}

impl App {
    async fn connect(socket: &Path) -> App {
        loop {
            if let Ok(stream) = UnixStream::connect(socket).await {
                let (r, writer) = stream.into_split();
                return App {
                    lines: BufReader::new(r).lines(),
                    writer,
                };
            }
            sleep(Duration::from_millis(10)).await;
        }
    }

    async fn next(&mut self) -> Value {
        let line = timeout(Duration::from_secs(10), self.lines.next_line())
            .await
            .expect("engine went quiet")
            .unwrap()
            .expect("engine closed");
        serde_json::from_str(&line).unwrap()
    }

    /// The next message of a type, skipping others.
    async fn next_of(&mut self, kind: &str) -> Value {
        loop {
            let m = self.next().await;
            if m["type"] == kind {
                return m;
            }
        }
    }

    async fn send(&mut self, text: &str) {
        self.writer.write_all(text.as_bytes()).await.unwrap();
        self.writer.write_all(b"\n").await.unwrap();
    }
}

#[tokio::test]
async fn an_app_sees_the_wind_at_home_then_at_the_boat() {
    let dir = scratch("boat");
    let wind = tokio::spawn(engine::run(config(&dir, four_utc)));
    let mut app = App::connect(&dir.join("wind.sock")).await;

    let hello = app.next().await;
    assert_eq!(
        (hello["type"].as_str(), hello["v"].as_u64()),
        (Some("hello"), Some(1))
    );
    let state = app.next_of("state").await;
    let f = &state["forecast"];
    assert_eq!(f["status"], "ok");
    assert_eq!(f["model"], "HRRR");
    assert_eq!(f["run"], "2026-09-14T03:00:00Z");
    assert_eq!(f["ageMinutes"], 60);
    assert_eq!(f["hours"], 3);
    assert_eq!(f["last"], "2026-09-14T05:00:00Z");
    assert_eq!(state["fetch"]["status"], "off");
    assert_eq!(state["keel"], "lost");
    let here = &state["here"];
    assert_eq!(here["at"], "home");
    assert_eq!(here["time"], "2026-09-14T04:00:00Z");
    assert!(
        here["speedKn"].is_number()
            && here["gustKn"].is_number()
            && here["pressureHpa"].is_number()
    );
    // The hour under way and the ones after it.
    let times: Vec<&str> = state["outlook"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["time"].as_str().unwrap())
        .collect();
    assert_eq!(times, ["2026-09-14T04:00:00Z", "2026-09-14T05:00:00Z"]);
    assert!(state.get("problems").is_none());

    // omakeel comes up with the boat on HRRR's point 3000, where pyproj
    // and eccodes put the wind at 4.769293 m/s from 285.78° true.
    let keel = UnixListener::bind(dir.join("keel.sock")).unwrap();
    let hub = tokio::spawn(async move {
        let (mut stream, _) = keel.accept().await.unwrap();
        let fix = json!({"type": "state", "v": 1, "sources": [],
                         "fix": {"status": "ok", "lat": 37.652249, "lon": -122.408667, "ageSeconds": 0}});
        let text = format!("{{\"type\":\"hello\",\"v\":1,\"keel\":\"0.1.0\"}}\n{fix}\n");
        stream.write_all(text.as_bytes()).await.unwrap();
        sleep(Duration::from_secs(60)).await;
    });
    let state = loop {
        let s = app.next_of("state").await;
        if s["here"]["at"] == "boat" {
            break s;
        }
    };
    assert_eq!(state["keel"], "connected");
    assert_eq!(state["here"]["speedKn"], 9.3);
    assert_eq!(state["here"]["dirDeg"], 286);
    assert!(state["here"].get("stale").is_none());

    // A wind field for the chart, thinned.
    app.send(
        r#"{"type":"field","id":7,"south":37.6,"west":-122.6,"north":37.9,"east":-122.2,"max":50}"#,
    )
    .await;
    let field = app.next_of("field").await;
    assert_eq!(field["id"], 7);
    assert_eq!(field["time"], "2026-09-14T04:00:00Z");
    assert_eq!(field["run"], "2026-09-14T03:00:00Z");
    let points = field["points"].as_array().unwrap();
    assert!(!points.is_empty() && points.len() <= 50, "{}", points.len());
    for p in points {
        let (lat, lon) = (p["lat"].as_f64().unwrap(), p["lon"].as_f64().unwrap());
        assert!((37.6..=37.9).contains(&lat) && (-122.6..=-122.2).contains(&lon));
        assert!(p["speedKn"].is_number() && p["dirDeg"].is_number());
    }
    app.send(r#"{"type":"field","id":8,"south":37.6,"west":-122.6,"north":37.9,"east":-122.2,"time":"2026-09-14T05:00:00Z"}"#)
        .await;
    let later = app.next_of("field").await;
    assert_eq!(
        (later["id"].as_u64(), later["time"].as_str()),
        (Some(8), Some("2026-09-14T05:00:00Z"))
    );

    // Bad requests are answered, and the connection carries on.
    for (request, id, says) in [
        ("nonsense", None, "not a JSON object"),
        (
            r#"{"type":"field","id":9,"south":1}"#,
            Some(9),
            "west must be a number",
        ),
        (
            r#"{"type":"field","id":10,"south":37,"west":-123,"north":38,"east":-122,"time":"2026-09-15T00:00:00Z"}"#,
            Some(10),
            "outside the forecast",
        ),
        (
            r#"{"type":"field","id":11,"south":38,"west":-123,"north":37,"east":-122}"#,
            Some(11),
            "south must be below north",
        ),
        (r#"{"type":"tide","id":12}"#, Some(12), "unknown type tide"),
    ] {
        app.send(request).await;
        let e = app.next_of("error").await;
        assert_eq!(e["id"].as_u64(), id, "{request}");
        assert!(
            e["message"].as_str().unwrap().contains(says),
            "{request}: {}",
            e["message"]
        );
    }
    let long = "x".repeat(70 * 1024);
    app.send(&long).await;
    assert_eq!(app.next_of("error").await["message"], "line too long");
    app.send(
        r#"{"type":"field","id":13,"south":37.6,"west":-122.6,"north":37.9,"east":-122.2,"max":5}"#,
    )
    .await;
    assert_eq!(app.next_of("field").await["id"], 13);

    // One engine to a socket.
    let second = engine::run(config(&dir, four_utc)).await;
    assert_eq!(second.unwrap_err().kind(), std::io::ErrorKind::AddrInUse);

    hub.abort();
    wind.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_forecast_that_has_run_out_says_so() {
    let dir = scratch("expired");
    std::fs::write(dir.join("config.toml"), "home = 37.8, -122.45\nbogus = 1\n").unwrap();
    let wind = tokio::spawn(engine::run(config(&dir, six_utc)));
    let mut app = App::connect(&dir.join("wind.sock")).await;
    let state = app.next_of("state").await;
    assert_eq!(state["forecast"]["status"], "expired");
    assert_eq!(state["here"]["lat"], 37.8);
    assert!(state["here"].get("speedKn").is_none());
    assert!(state["here"]["note"].as_str().unwrap().contains("run out"));
    assert_eq!(state["outlook"].as_array().unwrap().len(), 0);
    assert_eq!(state["problems"][0], "config.toml: unknown setting bogus");
    wind.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn with_nothing_cached_there_is_no_forecast_yet() {
    let dir = scratch("empty");
    std::fs::remove_dir_all(dir.join("cache")).unwrap();
    let wind = tokio::spawn(engine::run(config(&dir, four_utc)));
    let mut app = App::connect(&dir.join("wind.sock")).await;
    let state = app.next_of("state").await;
    assert_eq!(state["forecast"]["status"], "none");
    assert_eq!(state["forecast"]["region"]["south"], 36.8);
    assert_eq!(state["here"]["note"], "no forecast yet");
    app.send(r#"{"type":"field","south":37,"west":-123,"north":38,"east":-122}"#)
        .await;
    assert_eq!(
        app.next_of("error").await["message"],
        "field: no forecast yet"
    );
    wind.abort();
    let _ = std::fs::remove_dir_all(&dir);
}
