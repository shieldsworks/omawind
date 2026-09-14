//! Getting forecasts from NOAA's NOMADS server: its folder listing says
//! which HRRR runs are out, and its filter cuts each hour down to the
//! region and the four fields omawind reads, about 32 KB for the Bay.
//! Downloads go through `curl`, as omahelm's do.
//!
//! Cache: `<cache>/hrrr/<south>_<west>_<north>_<east>/<YYYYMMDDHH>/f00.grib2`
//! and on: a folder per region, so a run's hours are only ever cut to one
//! area, whoever fetched them.

use crate::config::Region;
use crate::forecast::{Forecast, MAX_HOURS, hour_file};
use crate::grib;
use crate::time;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const NOMADS: &str = "https://nomads.ncep.noaa.gov";
const USER_AGENT: &str = concat!(
    "omawind/",
    env!("CARGO_PKG_VERSION"),
    " (+https://omahoy.org)"
);
/// A run is used once its first 18 hours are out. The 00, 06, 12 and 18
/// UTC runs go on to 48, and those hours are fetched as they appear.
pub const READY_HOURS: u32 = 18;
/// Between one download and the next, to go easy on NOMADS.
const PAUSE: Duration = Duration::from_millis(300);
/// The most a listing or an hour may weigh. The Bay's hours are 33 KB,
/// the biggest region's under half a megabyte, a day's listing 100 KB.
const LISTING_LIMIT: u64 = 8 << 20;
const HOUR_LIMIT: u64 = 16 << 20;
/// curl gives up after 120 seconds; this is in case curl itself hangs.
const DEADLINE: Duration = Duration::from_secs(150);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// `YYYYMMDD`, UTC.
    pub day: String,
    pub hour: u32,
    /// The forecast hours listed, ascending.
    pub hours: Vec<u32>,
}

impl Run {
    /// `YYYYMMDDHH`, the run's cache folder.
    pub fn key(&self) -> String {
        format!("{}{:02}", self.day, self.hour)
    }

    /// Forecast hours listed without a gap from hour 0.
    pub fn unbroken(&self) -> Vec<u32> {
        (0..=MAX_HOURS)
            .take_while(|h| self.hours.contains(h))
            .collect()
    }

    pub fn ready(&self) -> bool {
        self.unbroken().len() > READY_HOURS as usize
    }

    pub fn time(&self) -> Option<i64> {
        time::parse_iso(&format!(
            "{}-{}-{}T{:02}:00Z",
            self.day.get(0..4)?,
            self.day.get(4..6)?,
            self.day.get(6..8)?,
            self.hour
        ))
    }
}

pub fn listing_url(day: &str) -> String {
    format!("{NOMADS}/pub/data/nccf/com/hrrr/prod/hrrr.{day}/conus/")
}

/// `hrrr.t03z.wrfsfcf01.grib2` → (3, 1).
fn hour_of(name: &str) -> Option<(u32, u32)> {
    let rest = name.strip_prefix("hrrr.t")?;
    let (run, rest) = rest.split_at_checked(2)?;
    let rest = rest.strip_prefix("z.wrfsfcf")?;
    let (hour, rest) = rest.split_at_checked(2)?;
    let digits = |s: &str| {
        s.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| s.parse().ok())?
    };
    (rest == ".grib2").then_some(())?;
    Some((digits(run)?, digits(hour)?))
}

/// The runs a day's listing shows, oldest first.
pub fn parse_listing(day: &str, html: &str) -> Vec<Run> {
    let mut runs: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for piece in html.split("href=\"").skip(1) {
        let name = piece.split('"').next().unwrap_or("");
        if let Some((run, hour)) = hour_of(name)
            && run < 24
            && hour <= MAX_HOURS
        {
            runs.entry(run).or_default().push(hour);
        }
    }
    runs.into_iter()
        .map(|(hour, mut hours)| {
            hours.sort_unstable();
            hours.dedup();
            Run {
                day: day.to_string(),
                hour,
                hours,
            }
        })
        .collect()
}

pub fn filter_url(run: &Run, hour: u32, r: &Region) -> String {
    format!(
        "{NOMADS}/cgi-bin/filter_hrrr_2d.pl?dir=%2Fhrrr.{day}%2Fconus&file=hrrr.t{run:02}z.wrfsfcf{hour:02}.grib2\
         &var_GUST=on&var_MSLMA=on&var_UGRD=on&var_VGRD=on\
         &lev_10_m_above_ground=on&lev_surface=on&lev_mean_sea_level=on\
         &subregion=&toplat={n}&leftlon={w}&rightlon={e}&bottomlat={s}",
        day = run.day,
        run = run.hour,
        n = r.north,
        w = r.west,
        e = r.east,
        s = r.south,
    )
}

/// NOMADS answers over HTTP/2 with a header curl rejects, so HTTP/1.1.
/// No more than `limit` bytes are kept, whatever the server says, and
/// curl is stopped after `DEADLINE` whatever it's doing. Both of its pipes
/// are read at once, so it never waits on one while omawind waits on the
/// other.
fn get(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut child = Command::new("curl")
        .args([
            "--http1.1",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
        ])
        .args(["--connect-timeout", "20", "--max-time", "120"])
        .args([
            "--user-agent",
            USER_AGENT,
            "--max-filesize",
            &limit.to_string(),
        ])
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Its own process group, so a timeout stops anything it started.
        .process_group(0)
        .spawn()
        .map_err(|e| format!("can't run curl: {e}"))?;
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        stop(&mut child);
        return Err("curl: no pipes".into());
    };
    let over = Arc::new(AtomicBool::new(false));
    let (body_tx, body_rx) = mpsc::channel();
    {
        let over = over.clone();
        std::thread::spawn(move || {
            let mut body = Vec::new();
            let read = stdout.take(limit + 1).read_to_end(&mut body);
            if body.len() as u64 > limit {
                over.store(true, Ordering::SeqCst);
            }
            let _ = body_tx.send(read.ok().map(|_| body));
        });
    }
    // Everything curl says is read; the first 64 KB is kept.
    let (why_tx, why_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stderr, mut kept, mut chunk) = (stderr, Vec::new(), [0u8; 4096]);
        while let Ok(n) = stderr.read(&mut chunk) {
            if n == 0 {
                break;
            }
            if kept.len() < 64 * 1024 {
                kept.extend_from_slice(&chunk[..n]);
            }
        }
        let _ = why_tx.send(String::from_utf8_lossy(&kept).trim().to_string());
    });
    let started = Instant::now();
    let left = || DEADLINE.saturating_sub(started.elapsed());
    let status = loop {
        let too_big = over.load(Ordering::SeqCst);
        if too_big || left().is_zero() {
            stop(&mut child);
            return Err(if too_big {
                format!("{url}: more than {} MB", limit >> 20)
            } else {
                format!("{url}: no answer in {} s", DEADLINE.as_secs())
            });
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                stop(&mut child);
                return Err(format!("curl: {e}"));
            }
        }
    };
    // curl has gone, but something it started may still hold its pipes:
    // the rest of the answer is waited for only until the deadline.
    let (Ok(body), Ok(why)) = (body_rx.recv_timeout(left()), why_rx.recv_timeout(left())) else {
        kill_group(child.id());
        return Err(format!("{url}: no answer in {} s", DEADLINE.as_secs()));
    };
    if !status.success() {
        return Err(if why.is_empty() {
            format!("curl failed on {url}")
        } else {
            why
        });
    }
    match body {
        Some(b) if b.len() as u64 <= limit => Ok(b),
        Some(_) => Err(format!("{url}: more than {} MB", limit >> 20)),
        None => Err(format!("{url}: couldn't read the answer")),
    }
}

/// Kills curl and anything it started, then reaps it.
fn stop(child: &mut Child) {
    kill_group(child.id());
    let _ = child.wait();
}

/// Only while the group has a member, so its id can't have been reused.
fn kill_group(leader: u32) {
    // SAFETY: a signal to the process group curl was started to lead.
    unsafe {
        libc::kill(-(leader as libc::pid_t), libc::SIGKILL);
    }
}

/// The newest run with its first 18 hours out, from today's listing, or
/// yesterday's just after midnight UTC.
pub fn newest_run(now: i64) -> Result<Option<Run>, String> {
    for day in [time::day_name(now), time::day_name(now - 86_400)] {
        let html = match get(&listing_url(&day), LISTING_LIMIT) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            // Today's folder doesn't exist until the first run lands.
            Err(e) if e.contains("404") => continue,
            Err(e) => return Err(e),
        };
        if let Some(run) = parse_listing(&day, &html)
            .into_iter()
            .rev()
            .find(Run::ready)
        {
            return Ok(Some(run));
        }
    }
    Ok(None)
}

/// The region's folder of runs.
pub fn runs_dir(cache: &Path, region: &Region) -> PathBuf {
    cache.join("hrrr").join(region.key().replace(',', "_"))
}

fn is_run_name(name: &str) -> bool {
    name.len() == 10 && name.bytes().all(|b| b.is_ascii_digit())
}

/// The runs cached for a region, newest first.
pub fn cached_runs(cache: &Path, region: &Region) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(runs_dir(cache, region)) else {
        return Vec::new();
    };
    let mut runs: Vec<(String, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            (is_run_name(&name) && e.path().is_dir()).then(|| (name, e.path()))
        })
        .collect();
    runs.sort();
    runs.into_iter().rev().map(|(_, path)| path).collect()
}

/// The forecast to show: the newest cached run with its first 18 hours
/// good, else the newest with any good hours, since an old forecast said
/// to be old beats none. With it, what was wrong with a newer run that
/// couldn't be read at all.
pub fn load_newest(cache: &Path, region: &Region) -> (Option<(PathBuf, Forecast)>, Option<String>) {
    let mut fallback = None;
    let mut problem = None;
    for dir in cached_runs(cache, region) {
        match Forecast::load(&dir) {
            Ok(f) if f.hours.len() > READY_HOURS as usize => return (Some((dir, f)), problem),
            Ok(f) => {
                if fallback.is_none() {
                    fallback = Some((dir, f));
                }
            }
            Err(e) => {
                if fallback.is_none() && problem.is_none() {
                    problem = Some(e);
                }
            }
        }
    }
    (fallback, problem)
}

/// Adds an hour to a run being checked, exactly as it will be loaded:
/// the run, the hour and the grid of the hours before it, winds turned.
fn add(run: &mut Option<Forecast>, bytes: &[u8], reference: i64) -> Result<(), String> {
    let fields = grib::parse(bytes)?;
    match run {
        Some(f) => f.push(&fields),
        None => {
            let f = Forecast::start(&fields)?;
            if f.run != reference {
                return Err("the hours are from another run".into());
            }
            *run = Some(f);
            Ok(())
        }
    }
}

/// The cache's lock, so `omawind fetch` and the engine take turns writing
/// it. Without `wait`, None when the other has it.
fn lock(cache: &Path, wait: bool) -> Result<Option<File>, String> {
    let dir = cache.join("hrrr");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(".lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let how = if wait {
        libc::LOCK_EX
    } else {
        libc::LOCK_EX | libc::LOCK_NB
    };
    // SAFETY: `flock` on a descriptor `file` owns; it's released when the
    // file closes.
    if unsafe { libc::flock(file.as_raw_fd(), how) } != 0 {
        let e = std::io::Error::last_os_error();
        if !wait && e.kind() == std::io::ErrorKind::WouldBlock {
            return Ok(None);
        }
        return Err(format!("{}: {e}", path.display()));
    }
    Ok(Some(file))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("part.{}.{n}", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Downloads the hours of `run` not cached, or cached but not fitting:
/// each is checked as the forecast will load it, against the hours before.
/// `progress` hears the hour under way and the run's length; `cancelled`
/// is asked between hours. Returns the run's folder and how many hours
/// were fetched.
pub fn download(
    cache: &Path,
    run: &Run,
    region: &Region,
    progress: &mut dyn FnMut(usize, usize),
    cancelled: &dyn Fn() -> bool,
) -> Result<(PathBuf, usize), String> {
    let _lock = lock(cache, true)?;
    let dir = runs_dir(cache, region).join(run.key());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let reference = run.time().ok_or("bad run name")?;
    let hours = run.unbroken();
    let mut checked: Option<Forecast> = None;
    let mut fetched = 0;
    for &hour in &hours {
        let path = dir.join(hour_file(hour));
        if let Ok(bytes) = std::fs::read(&path)
            && add(&mut checked, &bytes, reference).is_ok()
        {
            continue;
        }
        if cancelled() {
            return Err("stopped: the region changed".into());
        }
        progress(hour as usize, hours.len());
        if fetched > 0 {
            std::thread::sleep(PAUSE);
        }
        let bytes = get(&filter_url(run, hour, region), HOUR_LIMIT)?;
        add(&mut checked, &bytes, reference)
            .map_err(|e| format!("{} hour {hour}: {e}", run.key()))?;
        write_atomic(&path, &bytes)?;
        fetched += 1;
    }
    Ok((dir, fetched))
}

/// Deletes a region's runs older than the newest one named, except those
/// named. A newer run, perhaps just fetched by another omawind, stays.
/// Skipped while another omawind is writing the cache.
pub fn prune(cache: &Path, region: &Region, keep: &[&Path]) {
    let Some(newest) = keep.iter().filter_map(|p| p.file_name()).max() else {
        return;
    };
    let Ok(Some(_lock)) = lock(cache, false) else {
        return;
    };
    let Ok(rd) = std::fs::read_dir(runs_dir(cache, region)) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        if is_run_name(&name.to_string_lossy())
            && name.as_os_str() < newest
            && !keep.contains(&e.path().as_path())
        {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = r#"<a href="hrrr.t02z.wrfsfcf18.grib2">x</a> <a href="hrrr.t02z.wrfsfcf18.grib2.idx">
        <a href="hrrr.t03z.wrfsfcf00.grib2">x</a><a href="hrrr.t03z.wrfsfcf01.grib2">x</a>
        <a href="hrrr.t03z.wrfprsf00.grib2">x</a><a href="hrrr.t3z.wrfsfcf02.grib2">x</a>
        <a href="hrrr.t25z.wrfsfcf00.grib2">x</a><a href="hrrr.t03z.wrfsfcf03.grib2">x</a>"#;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omawind-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_runs_from_a_listing() {
        let runs = parse_listing("20260914", LISTING);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].hours, [18]);
        assert_eq!(runs[1].key(), "2026091403");
        assert_eq!(runs[1].hours, [0, 1, 3]);
        assert_eq!(runs[1].unbroken(), [0, 1]);
        assert!(!runs[1].ready());
        assert_eq!(runs[1].time(), Some(time::unix(2026, 9, 14, 3, 0, 0)));
    }

    #[test]
    fn a_run_is_ready_with_its_first_18_hours() {
        let run = |n: u32| Run {
            day: "20260914".into(),
            hour: 0,
            hours: (0..n).collect(),
        };
        assert!(!run(18).ready());
        assert!(run(19).ready());
        assert!(run(49).ready());
    }

    #[test]
    fn asks_nomads_for_the_region_and_four_fields() {
        let run = Run {
            day: "20260914".into(),
            hour: 3,
            hours: vec![],
        };
        let url = filter_url(&run, 1, &Region::BAY);
        assert!(url.starts_with(
            "https://nomads.ncep.noaa.gov/cgi-bin/filter_hrrr_2d.pl?dir=%2Fhrrr.20260914%2Fconus&file=hrrr.t03z.wrfsfcf01.grib2&"
        ));
        assert!(
            url.ends_with("&subregion=&toplat=38.8&leftlon=-123.8&rightlon=-121.6&bottomlat=36.8")
        );
        for v in ["UGRD", "VGRD", "GUST", "MSLMA"] {
            assert!(url.contains(&format!("&var_{v}=on")));
        }
    }

    #[test]
    fn keeps_regions_apart_and_prunes_only_older_runs() {
        let cache = scratch("prune");
        let other = Region {
            south: 47.0,
            west: -123.5,
            north: 48.8,
            east: -122.0,
        };
        let make = |region: &Region, name: &str| {
            let dir = runs_dir(&cache, region).join(name);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        };
        let old = make(&Region::BAY, "2026091401");
        let newer = make(&Region::BAY, "2026091404");
        let elsewhere = make(&other, "2026091402");
        make(&Region::BAY, "notarun");
        assert_eq!(
            cached_runs(&cache, &Region::BAY),
            [newer.clone(), old.clone()]
        );
        assert_eq!(
            cached_runs(&cache, &other),
            std::slice::from_ref(&elsewhere)
        );
        prune(&cache, &Region::BAY, &[&newer]);
        assert!(!old.exists() && newer.exists() && elsewhere.exists());
        let newest = make(&Region::BAY, "2026091405");
        prune(&cache, &Region::BAY, &[&newer]);
        assert!(newest.exists() && newer.exists());
        std::fs::remove_dir_all(&cache).unwrap();
    }

    #[test]
    fn loads_the_good_hours_and_passes_over_a_broken_run() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hrrr/2026091403");
        let cache = scratch("load");
        let copy = |name: &str| {
            let dir = runs_dir(&cache, &Region::BAY).join(name);
            std::fs::create_dir_all(&dir).unwrap();
            for h in 0..3 {
                std::fs::copy(fixture.join(hour_file(h)), dir.join(hour_file(h))).unwrap();
            }
            dir
        };
        let good = copy("2026091403");
        let broken = copy("2026091404");
        std::fs::write(
            broken.join("f00.grib2"),
            b"<html>Request for Future Data</html>",
        )
        .unwrap();
        let (found, problem) = load_newest(&cache, &Region::BAY);
        let (dir, f) = found.unwrap();
        assert_eq!((dir, f.hours.len()), (good.clone(), 3));
        // The broken newer run is said, though an older one is shown.
        assert!(problem.unwrap().contains("not GRIB"));
        // A bad hour after the first ends the forecast there.
        std::fs::remove_dir_all(&broken).unwrap();
        std::fs::write(good.join("f02.grib2"), b"GRIB").unwrap();
        let (found, _) = load_newest(&cache, &Region::BAY);
        assert_eq!(found.unwrap().1.hours.len(), 2);
        // Nothing good at all: say why.
        std::fs::write(good.join("f00.grib2"), b"<html>").unwrap();
        let (found, problem) = load_newest(&cache, &Region::BAY);
        assert!(found.is_none());
        assert!(problem.unwrap().contains("not GRIB"));
        std::fs::remove_dir_all(&cache).unwrap();
    }

    #[test]
    fn an_hour_counts_only_if_it_fits_the_run() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hrrr/2026091403");
        let hour = |h: u32| std::fs::read(fixture.join(hour_file(h))).unwrap();
        let three = time::unix(2026, 9, 14, 3, 0, 0);
        // Another run's first hour.
        assert!(add(&mut None, &hour(0), three + 3600).is_err());
        let mut run = None;
        add(&mut run, &hour(0), three).unwrap();
        // Hour 2 where hour 1 belongs.
        assert!(add(&mut run, &hour(2), three).is_err());
        add(&mut run, &hour(1), three).unwrap();
        add(&mut run, &hour(2), three).unwrap();
        assert_eq!(run.unwrap().hours.len(), 3);
    }

    #[test]
    fn keeps_no_more_than_the_limit() {
        let dir = scratch("get");
        let file = dir.join("answer");
        std::fs::write(&file, vec![b'x'; 5000]).unwrap();
        let url = format!("file://{}", file.display());
        assert_eq!(get(&url, 5000).unwrap().len(), 5000);
        assert!(get(&url, 4999).is_err());
        assert!(get(&format!("file://{}", dir.join("missing").display()), 5000).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
