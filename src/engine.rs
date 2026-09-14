//! `omawind run`: keeps the newest forecast for the region, follows the
//! boat through omakeel, and serves both over the Unix socket in
//! docs/protocol.md.

use crate::config::{self, Region, Settings};
use crate::fetch;
use crate::forecast::{Forecast, Sample};
use crate::keel::{self, Boat, Update};
use crate::time;
use serde_json::{Map, Value, json};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io,
    os::unix::{
        fs::{FileTypeExt, OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    sync::{Arc, mpsc as std_mpsc},
    time::SystemTime,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::mpsc,
    task::JoinHandle,
    time::{Duration, MissedTickBehavior, interval, sleep, timeout},
};

pub const VERSION: u32 = 1;
/// Messages queued for one app. An app this far behind is dropped.
const QUEUE: usize = 64;
/// An app that can't take one message in this long has stalled.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// The longest line an app may send.
const MAX_LINE: usize = 64 * 1024;
/// How often NOMADS is asked for a newer run.
const CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(10 * 60);
/// A forecast from a run this old means newer runs couldn't be fetched.
const OLD_AFTER: i64 = 4 * time::HOUR;
/// The most points a `field` answer may carry.
const MAX_POINTS: usize = 2000;

pub struct Config {
    pub socket: PathBuf,
    /// omakeel's socket, for the boat's position. None to never ask.
    pub keel: Option<PathBuf>,
    pub cache: PathBuf,
    /// The settings file.
    pub settings: PathBuf,
    /// Whether to download forecasts, or only use what's cached.
    pub fetch: bool,
    /// Unix seconds now. Tests pin it.
    pub clock: fn() -> i64,
}

/// `$XDG_RUNTIME_DIR/omawind/wind.sock`.
pub fn default_socket() -> io::Result<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("omawind").join("wind.sock"))
        .ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR must be set to an absolute path"))
}

enum Event {
    Checking,
    Downloading { done: usize, total: usize },
    Fetched(Result<(PathBuf, usize), String>),
    Request { client: u64, message: Value },
}

struct FetchState {
    status: &'static str,
    progress: Option<(usize, usize)>,
    message: Option<String>,
    checked: Option<i64>,
}

struct Wind {
    config: Config,
    settings: Settings,
    problems: Vec<String>,
    settings_seen: Option<SystemTime>,
    forecast: Option<Forecast>,
    run_dir: Option<PathBuf>,
    load_problem: Option<String>,
    fetch: FetchState,
    boat: Option<Boat>,
    keel: &'static str,
}

impl Wind {
    /// The newest good run cached. What's on show stays while nothing
    /// cached can be read, and the problem is said.
    fn load_newest(&mut self) {
        let (found, problem) = fetch::load_newest(&self.config.cache, &self.settings.region);
        self.load_problem = problem;
        match found {
            Some((dir, f)) => {
                self.forecast = Some(f);
                self.run_dir = Some(dir);
            }
            None if self.load_problem.is_none() => {
                self.forecast = None;
                self.run_dir = None;
            }
            None => {}
        }
    }

    /// Reads the settings file again if it changed. True if the region did.
    fn reread_settings(&mut self) -> bool {
        let seen = fs::metadata(&self.config.settings)
            .and_then(|m| m.modified())
            .ok();
        if seen == self.settings_seen {
            return false;
        }
        self.settings_seen = seen;
        let (settings, problems) = config::load(&self.config.settings);
        let moved = settings.region != self.settings.region;
        self.settings = settings;
        self.problems = problems;
        if moved {
            self.forecast = None;
            self.run_dir = None;
            self.load_newest();
        }
        moved
    }

    /// Where the wind is reported: the boat, else home.
    fn here(&self) -> (f64, f64, &'static str, bool) {
        match self.boat {
            Some(b) => (b.lat, b.lon, "boat", !b.current),
            None => (self.settings.home.0, self.settings.home.1, "home", false),
        }
    }

    fn state(&self, now: i64) -> String {
        // Minutes, so the state changes once a minute, not every second.
        let now = now - now.rem_euclid(60);
        let r = self.settings.region;
        let region = json!({"south": r.south, "west": r.west, "north": r.north, "east": r.east});
        let mut forecast = Map::new();
        match &self.forecast {
            None => {
                forecast.insert("status".into(), json!("none"));
            }
            Some(f) => {
                let status = if now > f.last() {
                    "expired"
                } else if now - f.run >= OLD_AFTER {
                    "old"
                } else {
                    "ok"
                };
                forecast.insert("status".into(), json!(status));
                forecast.insert("model".into(), json!("HRRR"));
                forecast.insert("run".into(), json!(time::iso(f.run)));
                forecast.insert("ageMinutes".into(), json!((now - f.run).max(0) / 60));
                forecast.insert("first".into(), json!(time::iso(f.first())));
                forecast.insert("last".into(), json!(time::iso(f.last())));
                forecast.insert("hours".into(), json!(f.hours.len()));
            }
        }
        forecast.insert("region".into(), region);

        let mut fetch = Map::new();
        fetch.insert("status".into(), json!(self.fetch.status));
        if let Some((done, total)) = self.fetch.progress {
            fetch.insert("done".into(), json!(done));
            fetch.insert("total".into(), json!(total));
        }
        if let Some(m) = &self.fetch.message {
            fetch.insert("message".into(), json!(m));
        }
        if let Some(t) = self.fetch.checked {
            fetch.insert("checked".into(), json!(time::iso(t)));
        }

        let (lat, lon, at, stale) = self.here();
        let mut here = Map::new();
        here.insert("at".into(), json!(at));
        if stale {
            here.insert("stale".into(), json!(true));
        }
        here.insert("lat".into(), json!(round(lat, 5)));
        here.insert("lon".into(), json!(round(lon, 5)));
        here.insert("time".into(), json!(time::iso(now)));
        let mut outlook = Vec::new();
        match &self.forecast {
            Some(f) => match f.sample(lat, lon, now) {
                Some(s) => sample_into(&mut here, &s),
                None => {
                    let why = if !self.settings.region.contains(lat, lon)
                        || f.sample(lat, lon, f.first()).is_none()
                    {
                        format!("the {at} is outside the forecast area")
                    } else {
                        "no forecast for now: the newest one has run out".into()
                    };
                    here.insert("note".into(), json!(why));
                }
            },
            None => {
                here.insert("note".into(), json!("no forecast yet"));
            }
        }
        if let Some(f) = &self.forecast {
            for hour in f.hours.iter().filter(|h| h.valid > now - time::HOUR) {
                if let Some(s) = f.sample(lat, lon, hour.valid) {
                    let mut m = Map::new();
                    m.insert("time".into(), json!(time::iso(hour.valid)));
                    sample_into(&mut m, &s);
                    outlook.push(Value::Object(m));
                }
            }
        }

        let mut problems = self.problems.clone();
        problems.extend(self.load_problem.iter().cloned());
        if self.keel == "incompatible" {
            problems.push("omakeel speaks another protocol version: update omawind".into());
        }
        let mut state = json!({
            "type": "state", "v": VERSION,
            "forecast": forecast,
            "fetch": fetch,
            "keel": self.keel,
            "here": here,
            "outlook": outlook,
        });
        if !problems.is_empty() {
            state["problems"] = json!(problems);
        }
        state.to_string()
    }

    fn answer(&self, message: &Value, now: i64) -> Value {
        let id = message.get("id").cloned();
        let reply = match message.get("type").and_then(Value::as_str) {
            Some("field") => self.field(message, now),
            Some(other) => Err(format!("unknown type {other}")),
            None => Err("missing type".into()),
        };
        let mut reply =
            reply.unwrap_or_else(|e| json!({"type": "error", "v": VERSION, "message": e}));
        if let Some(id) = id {
            reply["id"] = id;
        }
        reply
    }

    fn field(&self, m: &Value, now: i64) -> Result<Value, String> {
        let number = |k: &str| {
            m.get(k)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
                .ok_or_else(|| format!("field: {k} must be a number"))
        };
        let area = Region {
            south: number("south")?,
            west: number("west")?,
            north: number("north")?,
            east: number("east")?,
        };
        if !(area.south < area.north && area.west < area.east) {
            return Err("field: south must be below north and west below east".into());
        }
        let t = match m.get("time") {
            None => now,
            Some(Value::String(s)) => {
                time::parse_iso(s).ok_or("field: time must be like 2026-09-14T03:00:00Z")?
            }
            Some(_) => return Err("field: time must be a string".into()),
        };
        let max = match m.get("max") {
            None => 400,
            Some(v) => v
                .as_u64()
                .filter(|&n| (1..=MAX_POINTS as u64).contains(&n))
                .ok_or(format!("field: max must be 1 to {MAX_POINTS}"))?
                as usize,
        };
        let f = self.forecast.as_ref().ok_or("field: no forecast yet")?;
        let (step, points) = f.field(&area, t, max).ok_or_else(|| {
            format!(
                "field: {} is outside the forecast, {} to {}",
                time::iso(t),
                time::iso(f.first()),
                time::iso(f.last())
            )
        })?;
        let points: Vec<Value> = points
            .iter()
            .map(|p| {
                let mut m = Map::new();
                m.insert("lat".into(), json!(round(p.lat, 4)));
                m.insert("lon".into(), json!(round(p.lon, 4)));
                sample_into(&mut m, &p.sample);
                Value::Object(m)
            })
            .collect();
        Ok(
            json!({"type": "field", "v": VERSION, "time": time::iso(t), "run": time::iso(f.run),
                  "step": step, "points": points}),
        )
    }
}

fn round(v: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (v * k).round() / k
}

fn sample_into(m: &mut Map<String, Value>, s: &Sample) {
    m.insert("speedKn".into(), json!(round(s.speed_kn, 1)));
    m.insert(
        "dirDeg".into(),
        json!((s.from_deg.round() as i64).rem_euclid(360)),
    );
    if let Some(g) = s.gust_kn {
        m.insert("gustKn".into(), json!(round(g, 1)));
    }
    if let Some(p) = s.pressure_hpa {
        m.insert("pressureHpa".into(), json!(round(p, 1)));
    }
}

fn encode(v: &str) -> Arc<str> {
    let mut line = v.to_string();
    line.push('\n');
    line.into()
}

/// Runs until it fails to bind. The socket file is removed when the
/// returned future ends or is dropped.
pub async fn run(config: Config) -> io::Result<()> {
    let (listener, _socket) = bind(&config.socket)?;
    let (tx, mut events) = mpsc::channel::<Event>(256);
    let (keel_tx, mut keel_rx) = mpsc::channel::<Update>(16);
    if let Some(path) = config.keel.clone() {
        tokio::spawn(keel::follow(path, keel_tx));
    } else {
        drop(keel_tx);
    }
    let fetching = config.fetch;
    let mut wind = Wind {
        settings: Settings::default(),
        problems: Vec::new(),
        settings_seen: None,
        forecast: None,
        run_dir: None,
        load_problem: None,
        fetch: FetchState {
            status: if fetching { "idle" } else { "off" },
            progress: None,
            message: None,
            checked: None,
        },
        boat: None,
        keel: if config.keel.is_some() { "lost" } else { "off" },
        config,
    };
    // Read the settings, and the newest run cached for their region.
    wind.settings_seen = Some(SystemTime::UNIX_EPOCH);
    wind.reread_settings();
    wind.load_newest();
    let regions = fetching.then(|| {
        spawn_fetcher(
            wind.config.cache.clone(),
            wind.settings.region,
            tx.clone(),
            wind.config.clock,
        )
    });

    let mut clients: Vec<Client> = Vec::new();
    let mut next_id: u64 = 1;
    let mut last_state: Arc<str> = Arc::from("");
    let mut tick = interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut ticks: u64 = 0;
    loop {
        let now = (wind.config.clock)();
        tokio::select! {
            Some(event) = events.recv() => match event {
                Event::Checking => {
                    wind.fetch.status = "checking";
                    wind.fetch.progress = None;
                }
                Event::Downloading { done, total } => {
                    wind.fetch.status = "downloading";
                    wind.fetch.progress = Some((done, total));
                }
                Event::Fetched(Ok((dir, _))) => {
                    wind.fetch.status = "idle";
                    wind.fetch.progress = None;
                    wind.fetch.message = None;
                    wind.fetch.checked = Some(now);
                    // A new run, new hours of this one, or hours another
                    // omawind fetched: reading the cache again is cheap.
                    wind.load_newest();
                    if let Some(shown) = wind.run_dir.clone() {
                        fetch::prune(&wind.config.cache, &wind.settings.region, &[&shown, &dir]);
                    }
                }
                Event::Fetched(Err(e)) => {
                    wind.fetch.status = "error";
                    wind.fetch.progress = None;
                    wind.fetch.message = Some(e);
                    // The hours that did arrive may have made a run ready.
                    wind.load_newest();
                }
                Event::Request { client, message } => {
                    let reply = encode(&wind.answer(&message, now).to_string());
                    // An app too far behind to take its answer is let go.
                    if let Some(i) = clients.iter().position(|c| c.id == client)
                        && clients[i].tx.try_send(reply).is_err()
                    {
                        clients.remove(i);
                    }
                }
            },
            Some(update) = keel_rx.recv() => match update {
                Update::Boat(boat) => {
                    wind.boat = boat;
                    wind.keel = "connected";
                }
                Update::Lost => {
                    wind.boat = None;
                    wind.keel = "lost";
                }
                Update::Incompatible(_) => {
                    wind.boat = None;
                    wind.keel = "incompatible";
                }
            },
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let client = Client::spawn(stream, next_id, tx.clone());
                    next_id += 1;
                    let hello = encode(&json!({"type": "hello", "v": VERSION, "wind": env!("CARGO_PKG_VERSION")}).to_string());
                    if client.tx.try_send(hello).is_ok() {
                        clients.push(client);
                    }
                }
                Err(e) => {
                    eprintln!("omawind: accept: {e}");
                    sleep(Duration::from_millis(100)).await;
                }
            },
            _ = tick.tick() => {
                ticks += 1;
                if ticks.is_multiple_of(5) && wind.reread_settings()
                    && let Some(regions) = &regions
                {
                    let _ = regions.send(wind.settings.region);
                }
            }
        }

        let state = encode(&wind.state(now));
        let changed = state != last_state;
        last_state = state;
        clients.retain_mut(|client| {
            if client.tx.is_closed() {
                return false;
            }
            let fresh = std::mem::take(&mut client.fresh);
            !(changed || fresh) || client.tx.try_send(last_state.clone()).is_ok()
        });
    }
}

/// Checks NOMADS on a thread of its own, since curl blocks: at once, then
/// every 10 minutes, or as soon as the region changes.
fn spawn_fetcher(
    cache: PathBuf,
    mut region: Region,
    tx: mpsc::Sender<Event>,
    clock: fn() -> i64,
) -> std_mpsc::Sender<Region> {
    let (regions, rx) = std_mpsc::channel::<Region>();
    std::thread::spawn(move || {
        loop {
            if tx.blocking_send(Event::Checking).is_err() {
                return;
            }
            let result = fetch::newest_run(clock()).and_then(|run| {
                let run = run.ok_or("NOMADS lists no HRRR run with 18 hours out yet")?;
                fetch::download(&cache, &run, &region, &mut |done, total| {
                    let _ = tx.blocking_send(Event::Downloading { done, total });
                })
            });
            if tx.blocking_send(Event::Fetched(result)).is_err() {
                return;
            }
            match rx.recv_timeout(CHECK_EVERY) {
                Ok(r) => region = r,
                Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                Err(std_mpsc::RecvTimeoutError::Disconnected) => return,
            }
            while let Ok(r) = rx.try_recv() {
                region = r;
            }
        }
    });
    regions
}

/// One connected app: a task writes its queue and reads its requests.
/// Dropping it closes the app's socket.
struct Client {
    id: u64,
    tx: mpsc::Sender<Arc<str>>,
    /// Hasn't been sent the current state yet.
    fresh: bool,
    task: JoinHandle<()>,
}

impl Client {
    fn spawn(stream: UnixStream, id: u64, requests: mpsc::Sender<Event>) -> Client {
        let (tx, mut rx) = mpsc::channel::<Arc<str>>(QUEUE);
        let task = tokio::spawn(async move {
            let (mut reader, mut writer) = stream.into_split();
            let mut scratch = [0u8; 4096];
            let mut line: Vec<u8> = Vec::new();
            // Past MAX_LINE: the rest of the line is thrown away.
            let mut skipping = false;
            loop {
                tokio::select! {
                    out = rx.recv() => {
                        let Some(out) = out else { break };
                        let written = timeout(WRITE_TIMEOUT, writer.write_all(out.as_bytes())).await;
                        if !matches!(written, Ok(Ok(()))) {
                            break;
                        }
                    }
                    read = reader.read(&mut scratch) => {
                        let n = match read {
                            Ok(n) if n > 0 => n,
                            _ => break,
                        };
                        let mut errors = Vec::new();
                        for &b in &scratch[..n] {
                            if b != b'\n' {
                                if !skipping {
                                    line.push(b);
                                    if line.len() > MAX_LINE {
                                        line.clear();
                                        skipping = true;
                                        errors.push("line too long");
                                    }
                                }
                                continue;
                            }
                            if std::mem::take(&mut skipping) {
                                continue;
                            }
                            let text = std::mem::take(&mut line);
                            if text.iter().all(u8::is_ascii_whitespace) {
                                continue;
                            }
                            match serde_json::from_slice::<Value>(&text) {
                                Ok(message @ Value::Object(_)) => {
                                    if requests.send(Event::Request { client: id, message }).await.is_err() {
                                        return;
                                    }
                                }
                                _ => errors.push("not a JSON object"),
                            }
                        }
                        for e in errors {
                            let reply = json!({"type": "error", "v": VERSION, "message": e}).to_string() + "\n";
                            let written = timeout(WRITE_TIMEOUT, writer.write_all(reply.as_bytes())).await;
                            if !matches!(written, Ok(Ok(()))) {
                                return;
                            }
                        }
                    }
                }
            }
        });
        Client {
            id,
            tx,
            fresh: true,
            task,
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Takes the lock beside the socket, so one socket has one engine, then
/// binds, replacing a socket a crashed engine left behind. As omakeel does.
fn bind(path: &Path) -> io::Result<(UnixListener, SocketFile)> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
        && !dir.exists()
    {
        fs::create_dir_all(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    let lock = lock(path)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_socket() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} exists and isn't a socket", path.display()),
            ));
        }
        fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    Ok((
        listener,
        SocketFile {
            path: path.to_path_buf(),
            _lock: lock,
        },
    ))
}

fn lock_path(socket: &Path) -> PathBuf {
    let mut name = OsString::from(socket.as_os_str());
    name.push(".lock");
    PathBuf::from(name)
}

fn lock(socket: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(lock_path(socket))?;
    // SAFETY: `flock` on a descriptor `file` owns; the lock lasts until the
    // file is closed.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let e = io::Error::last_os_error();
        if e.kind() == io::ErrorKind::WouldBlock {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("omawind is already running on {}", socket.display()),
            ));
        }
        return Err(e);
    }
    Ok(file)
}

struct SocketFile {
    path: PathBuf,
    _lock: File,
}

impl Drop for SocketFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
