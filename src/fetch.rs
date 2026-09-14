//! Getting forecasts from NOAA's NOMADS server: its folder listing says
//! which HRRR runs are out, and its filter cuts each hour down to the
//! region and the four fields omawind reads, about 32 KB for the Bay.
//! Downloads go through `curl`, as omahelm's do.
//!
//! Cache: `<cache>/hrrr/<YYYYMMDDHH>/f00.grib2`… with a `region` file
//! naming the area the hours were cut to.

use crate::config::Region;
use crate::forecast::{MAX_HOURS, hour_file};
use crate::grib;
use crate::time;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

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
fn get(url: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("curl")
        .args([
            "--http1.1",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
        ])
        .args([
            "--connect-timeout",
            "20",
            "--max-time",
            "120",
            "--user-agent",
            USER_AGENT,
        ])
        .arg(url)
        .output()
        .map_err(|e| format!("can't run curl: {e}"))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if why.is_empty() {
            format!("curl failed on {url}")
        } else {
            why
        });
    }
    Ok(out.stdout)
}

/// The newest run with its first 18 hours out, from today's listing, or
/// yesterday's just after midnight UTC.
pub fn newest_run(now: i64) -> Result<Option<Run>, String> {
    for day in [time::day_name(now), time::day_name(now - 86_400)] {
        let html = match get(&listing_url(&day)) {
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

pub fn runs_dir(cache: &Path) -> PathBuf {
    cache.join("hrrr")
}

fn is_run_name(name: &str) -> bool {
    name.len() == 10 && name.bytes().all(|b| b.is_ascii_digit())
}

/// The run to show: the newest whose first 18 hours are cached for this
/// region, else the newest with any hours at all, since an old forecast
/// said to be old beats none.
pub fn newest_cached(cache: &Path, region: &Region) -> Option<PathBuf> {
    let mut runs: Vec<(String, PathBuf, usize)> = std::fs::read_dir(runs_dir(cache))
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let path = e.path();
            let region_ok =
                std::fs::read_to_string(path.join("region")).ok()?.trim() == region.key();
            let hours = (0..=MAX_HOURS)
                .take_while(|&h| path.join(hour_file(h)).is_file())
                .count();
            (is_run_name(&name) && region_ok && hours > 0).then_some((name, path, hours))
        })
        .collect();
    runs.sort();
    let ready = runs.iter().rev().find(|r| r.2 > READY_HOURS as usize);
    ready.or(runs.last()).map(|r| r.1.clone())
}

/// Checks a downloaded hour is what was asked for, not an error page.
fn check(bytes: &[u8], run: &Run, hour: u32) -> Result<(), String> {
    let fields = grib::parse(bytes)?;
    let reference = run.time().ok_or("bad run name")?;
    let wind = |number: u8| {
        fields.iter().any(|f| {
            (f.discipline, f.category, f.number, f.surface) == (0, 2, number, 103)
                && f.level == Some(10.0)
                && f.reference == reference
                && f.lead == i64::from(hour) * time::HOUR
        })
    };
    if wind(2) && wind(3) {
        Ok(())
    } else {
        Err("the download has no 10 m wind for that hour".into())
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Downloads the hours of `run` not yet cached. Returns its folder and how
/// many hours were new.
pub fn download(
    cache: &Path,
    run: &Run,
    region: &Region,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<(PathBuf, usize), String> {
    let dir = runs_dir(cache).join(run.key());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let region_file = dir.join("region");
    if std::fs::read_to_string(&region_file)
        .ok()
        .as_deref()
        .map(str::trim)
        != Some(&region.key())
    {
        // Hours cut to another region are no use: start the run again.
        for h in 0..=MAX_HOURS {
            let _ = std::fs::remove_file(dir.join(hour_file(h)));
        }
        write_atomic(&region_file, format!("{}\n", region.key()).as_bytes())?;
    }
    let missing: Vec<u32> = run
        .unbroken()
        .into_iter()
        .filter(|&h| !dir.join(hour_file(h)).is_file())
        .collect();
    for (done, &hour) in missing.iter().enumerate() {
        progress(done, missing.len());
        if done > 0 {
            std::thread::sleep(PAUSE);
        }
        let bytes = get(&filter_url(run, hour, region))?;
        check(&bytes, run, hour).map_err(|e| format!("{} hour {hour}: {e}", run.key()))?;
        write_atomic(&dir.join(hour_file(hour)), &bytes)?;
    }
    Ok((dir, missing.len()))
}

/// Deletes cached runs other than the ones named.
pub fn prune(cache: &Path, keep: &[&Path]) {
    let Ok(rd) = std::fs::read_dir(runs_dir(cache)) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        if is_run_name(&name.to_string_lossy()) && !keep.contains(&e.path().as_path()) {
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
    fn picks_a_complete_run_over_a_newer_partial_one() {
        let cache = std::env::temp_dir().join(format!("omawind-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache);
        let make = |name: &str, hours: u32, region: &Region| {
            let dir = runs_dir(&cache).join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("region"), region.key()).unwrap();
            for h in 0..hours {
                std::fs::write(dir.join(hour_file(h)), b"").unwrap();
            }
            dir
        };
        let other = Region {
            south: 47.0,
            west: -123.5,
            north: 48.8,
            east: -122.0,
        };
        let old = make("2026091401", 19, &Region::BAY);
        make("2026091402", 5, &Region::BAY);
        make("2026091403", 19, &other);
        assert_eq!(newest_cached(&cache, &Region::BAY), Some(old.clone()));
        let newer = make("2026091404", 19, &Region::BAY);
        assert_eq!(newest_cached(&cache, &Region::BAY), Some(newer.clone()));
        prune(&cache, &[&newer]);
        assert!(!old.exists() && newer.exists());
        std::fs::remove_dir_all(&cache).unwrap();
    }
}
