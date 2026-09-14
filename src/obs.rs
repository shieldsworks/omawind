//! Measured wind, from NOAA's National Data Buoy Center. One text file holds
//! every station's latest report: the buoys offshore, and around the Bay the
//! piers and tide gauges of NOAA's PORTS program. Another names them. Both
//! are read here from scratch, and fetched through `curl` like the forecast.

use crate::config::Region;
use crate::fetch;
use crate::time;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Where the reports and the names come from: NDBC, or files in tests.
#[derive(Clone, Debug)]
pub struct Source {
    pub latest: String,
    pub names: String,
}

impl Source {
    pub fn ndbc() -> Source {
        Source {
            latest: "https://www.ndbc.noaa.gov/data/latest_obs/latest_obs.txt".into(),
            names: "https://www.ndbc.noaa.gov/data/stations/station_table.txt".into(),
        }
    }
}

/// A report older than this is left out.
pub const MAX_AGE: i64 = 2 * time::HOUR;
/// Clocks drift; a report further ahead than this is wrong.
const AHEAD: i64 = 15 * 60;
/// Names change rarely, so the table is kept for a week.
const NAMES_KEPT: Duration = Duration::from_secs(7 * 24 * 3600);
/// The reports weigh about 100 KB, the names 360 KB.
const LIMIT: u64 = 8 << 20;
/// Knots in a meter a second.
const KNOTS: f64 = 3600.0 / 1852.0;
const COLUMNS: [&str; 11] = [
    "STN", "LAT", "LON", "YYYY", "MM", "DD", "hh", "mm", "WDIR", "WSPD", "GST",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Station {
    /// NDBC's id, like `AAMC1` or `46026`.
    pub id: String,
    /// Like `Alameda`, when NDBC's table names it.
    pub name: Option<String>,
    pub lat: f64,
    pub lon: f64,
    /// When the report was taken, Unix seconds.
    pub time: i64,
    /// Averaged over 2 minutes ashore, 8 on a buoy.
    pub speed_kn: f64,
    /// Where it blows from, degrees true. None only in a calm.
    pub from_deg: Option<f64>,
    pub gust_kn: Option<f64>,
}

/// Each station's newest report in `latest_obs.txt`, by id, with a wind to
/// draw or not. Columns are found by the header's names. A row that doesn't
/// read is left out, and so is one dated ahead of `now`, before the newest
/// is chosen, so it can't hide a true one. A file with no row that reads is
/// an error, not an empty Bay, so the last good reports stay.
pub fn parse_latest(text: &str, now: i64) -> Result<Vec<Report>, String> {
    let header = text
        .lines()
        .find(|l| l.starts_with("#STN"))
        .ok_or("latest_obs.txt: no header")?;
    let heads: Vec<&str> = header[1..].split_whitespace().collect();
    let mut cols = [0; COLUMNS.len()];
    for (col, name) in cols.iter_mut().zip(COLUMNS) {
        *col = heads
            .iter()
            .position(|&h| h == name)
            .ok_or_else(|| format!("latest_obs.txt: no {name} column"))?;
    }
    let mut newest: HashMap<String, Report> = HashMap::new();
    let mut read = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // A whole row: every column the header names, each after the
        // station's id a number or MM, whether it's read here or not.
        let whole = fields.len() == heads.len()
            && fields.iter().enumerate().all(|(i, t)| {
                i == cols[0] || *t == "MM" || t.parse::<f64>().is_ok_and(f64::is_finite)
            });
        if !whole {
            continue;
        }
        let Some(r) = report(&fields, &cols).filter(|r| r.time <= now + AHEAD) else {
            continue;
        };
        read += 1;
        // The newest report counts, wind or not: an older one's wind is
        // out of date.
        if newest.get(&r.id).is_none_or(|old| old.time < r.time) {
            newest.insert(r.id.clone(), r);
        }
    }
    if read == 0 {
        return Err("latest_obs.txt: no reports in it".into());
    }
    let mut reports: Vec<Report> = newest.into_values().collect();
    reports.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(reports)
}

/// A station's report. Without a speed, or with a speed and no direction,
/// it has no wind to draw, but it's still the station's newest word.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub id: String,
    pub time: i64,
    pub wind: Option<Station>,
}

/// None for a row that doesn't read. The row has been checked to be whole,
/// each value a number or NDBC's missing value, `MM`.
fn report(f: &[&str], cols: &[usize; COLUMNS.len()]) -> Option<Report> {
    let [
        stn,
        lat,
        lon,
        year,
        month,
        day,
        hour,
        minute,
        wdir,
        wspd,
        gst,
    ] = *cols;
    // Some(None) for `MM`; None for anything else that isn't a number.
    let value = |i: usize| match f[i] {
        "MM" => Some(None),
        t => t.parse::<f64>().ok().filter(|v| v.is_finite()).map(Some),
    };
    let id = f[stn];
    if id.len() > 12 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let (lat, lon) = (value(lat)??, value(lon)??);
    if !((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)) {
        return None;
    }
    let time = time::parse_iso(&format!(
        "{}-{}-{}T{}:{}Z",
        f[year], f[month], f[day], f[hour], f[minute]
    ))?;
    let (wspd, wdir, gst) = (value(wspd)?, value(wdir)?, value(gst)?);
    // A speed or direction out of range is a broken row, not a missing
    // value like MM, so it mustn't stand in for a good report.
    let in_range = |v: Option<f64>, top: f64| v.is_none_or(|v| (0.0..=top).contains(&v));
    if !in_range(wspd, 100.0) || !in_range(wdir, 360.0) {
        return None;
    }
    let id = id.to_ascii_uppercase();
    // No speed, or a speed without a direction: no barb to draw.
    let speed = wspd;
    let from_deg = wdir.map(|v| v % 360.0);
    let wind = match speed {
        Some(s) if from_deg.is_some() || s == 0.0 => Some(Station {
            id: id.clone(),
            name: None,
            lat,
            lon,
            time,
            speed_kn: s * KNOTS,
            from_deg,
            gust_kn: gst.filter(|v| (0.0..=150.0).contains(v)).map(|g| g * KNOTS),
        }),
        _ => None,
    };
    Some(Report { id, time, wind })
}

/// NDBC's names by id, trimmed for a chart: `9414750 - Alameda, CA` is
/// `Alameda`.
pub fn parse_names(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|line| {
            let mut f = line.split('|');
            let id = f.next()?.trim().to_ascii_uppercase();
            let name = clean_name(f.nth(3)?)?;
            (!id.is_empty()).then_some((id, name))
        })
        .collect()
}

fn clean_name(raw: &str) -> Option<String> {
    // The names come off the network and go to terminals and the chart:
    // no control characters.
    let raw: String = raw.chars().filter(|c| !c.is_control()).collect();
    let mut s = raw.trim().replace("&amp;", "&");
    // A tide gauge's number comes first.
    if let Some((number, rest)) = s.split_once(" - ")
        && !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit())
    {
        s = rest.to_string();
    }
    // And the state last.
    if let Some((rest, state)) = s.rsplit_once(", ")
        && state.len() == 2
        && state.bytes().all(|b| b.is_ascii_uppercase())
    {
        s = rest.to_string();
    }
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// `<cache>/ndbc/station_table.txt`.
pub fn names_file(cache: &Path) -> PathBuf {
    cache.join("ndbc").join("station_table.txt")
}

/// The station names: the copy on disk while it's under a week old, else
/// NDBC's again. An older copy stands in when NDBC can't be reached.
pub fn names(cache: &Path, source: &Source) -> Result<HashMap<String, String>, String> {
    let file = names_file(cache);
    let kept = || {
        fs::read_to_string(&file)
            .ok()
            .map(|t| parse_names(&t))
            .filter(|n| !n.is_empty())
    };
    let fresh = fs::metadata(&file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < NAMES_KEPT);
    if fresh && let Some(n) = kept() {
        return Ok(n);
    }
    let fetched = fetch::get(&source.names, LIMIT).and_then(|bytes| {
        let n = parse_names(&String::from_utf8_lossy(&bytes));
        if n.is_empty() {
            return Err(format!("{}: no station names in it", source.names));
        }
        Ok(n)
    });
    match fetched {
        // Folded into what's kept, so a table cut short can't forget the
        // names it lost; a station renamed takes its new name.
        Ok(n) => {
            let mut all = kept().unwrap_or_default();
            all.extend(n);
            save(&file, table(&all).as_bytes());
            Ok(all)
        }
        Err(e) => kept().ok_or(e),
    }
}

/// Names written back the way `parse_names` reads them, in id order.
fn table(names: &HashMap<String, String>) -> String {
    let mut ids: Vec<&String> = names.keys().collect();
    ids.sort();
    ids.into_iter()
        .map(|id| format!("{id}||||{}\n", names[id]))
        .collect()
}

/// Written to a file of its own beside it and renamed over, so a reader
/// never sees half a file, and two writers never share one. A cache that
/// can't be written only costs a fetch.
fn save(file: &Path, bytes: &[u8]) {
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let Some(dir) = file.parent() else { return };
    let tmp = dir.join(format!(
        ".station_table.{}.{}.tmp",
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut f| f.write_all(bytes))
        .and_then(|()| fs::rename(&tmp, file));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

/// Every station's latest report, named where the table names it.
pub fn latest(
    source: &Source,
    names: &HashMap<String, String>,
    now: i64,
) -> Result<Vec<Report>, String> {
    let bytes = fetch::get(&source.latest, LIMIT)?;
    let mut reports = parse_latest(&String::from_utf8_lossy(&bytes), now)?;
    for s in reports.iter_mut().filter_map(|r| r.wind.as_mut()) {
        s.name = names.get(&s.id).cloned();
    }
    Ok(reports)
}

/// Folds a fetch into the reports kept: each station's newest wins, and a
/// station the fetch left out keeps its last until that's too old to show,
/// so a file cut short can't empty the Bay. A report dated ahead of the
/// clock is never kept, or it would outrank every true one after it.
pub fn merge(kept: &mut BTreeMap<String, Report>, fetched: Vec<Report>, now: i64) {
    for r in fetched.into_iter().filter(|r| r.time <= now + AHEAD) {
        if kept.get(&r.id).is_none_or(|old| old.time <= r.time) {
            kept.insert(r.id.clone(), r);
        }
    }
    kept.retain(|_, r| r.time > now - MAX_AGE && r.time <= now + AHEAD);
}

/// The winds worth showing for a region now: inside it, and taken in the
/// last `MAX_AGE`.
pub fn current<'a>(
    reports: impl IntoIterator<Item = &'a Report>,
    region: Region,
    now: i64,
) -> impl Iterator<Item = &'a Station> {
    reports
        .into_iter()
        .filter_map(|r| r.wind.as_ref())
        .filter(move |s| {
            region.contains(s.lat, s.lon) && s.time > now - MAX_AGE && s.time <= now + AHEAD
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    /// 2026-09-14 17:30 UTC, the clock for every test here.
    const NOW: i64 = 1_789_407_000;

    // Only the columns read, so a row of 11 is whole.
    const HEADER: &str = "\
#STN       LAT      LON  YYYY MM DD hh mm WDIR WSPD   GST
#text      deg      deg   yr mo day hr mn degT  m/s   m/s
";

    /// The winds of each station's newest report.
    fn winds(reports: Vec<Report>) -> Vec<Station> {
        reports.into_iter().filter_map(|r| r.wind).collect()
    }

    fn parse(rows: &str) -> Vec<Station> {
        winds(parse_latest(&format!("{HEADER}{rows}"), NOW).unwrap())
    }

    fn ids(stations: &[Station]) -> Vec<&str> {
        stations.iter().map(|s| s.id.as_str()).collect()
    }

    #[test]
    fn reads_a_report_in_knots() {
        let s = parse("AAMC1    37.772 -122.300 2026 09 14 17 00 120   1.5   2.1\n");
        assert_eq!(ids(&s), ["AAMC1"]);
        let a = &s[0];
        assert_eq!((a.lat, a.lon), (37.772, -122.3));
        assert_eq!(a.time, time::unix(2026, 9, 14, 17, 0, 0));
        assert!((a.speed_kn - 2.9158).abs() < 1e-3, "{}", a.speed_kn);
        assert_eq!(a.from_deg, Some(120.0));
        assert!((a.gust_kn.unwrap() - 4.0821).abs() < 1e-3);
        assert_eq!(a.name, None);
    }

    #[test]
    fn leaves_out_what_cant_be_drawn_or_read() {
        let s = parse(
            "\
46214 37.944 -123.466 2026 09 14 16 56  MM   MM  MM
MLSC1 36.802 -121.791 2026 09 14 17 16  MM  1.5  MM
CALM1 37.000 -122.000 2026 09 14 17 00  MM  0.0  MM
BADLA 97.000 -122.000 2026 09 14 17 00 120  1.5  MM
BADTM 37.000 -122.000 2026 09 31 17 00 120  1.5  MM
BADDR 37.000 -122.000 2026 09 14 17 00 999  1.5  MM
FAST1 37.000 -122.000 2026 09 14 17 00 120  101  MM
SHORT 37.000 -122.000 2026 09 14
<html>not a report</html>
",
        );
        assert_eq!(ids(&s), ["CALM1"]);
        assert_eq!((s[0].speed_kn, s[0].from_deg), (0.0, None));
    }

    #[test]
    fn a_file_with_no_report_that_reads_is_an_error() {
        // Truncated, or a page that isn't the reports: the last good ones
        // should stay, so this mustn't read as an empty Bay.
        for body in [
            "",
            "AAMC1 37.772 -122.300 2026 09\n",
            "AAMC1 37.772 -122.300 2026 09 14 17 00\n",
            "AAMC1 37.772 -122.300 2026 09 14 17 00 120 fast MM\n",
            "AAMC1 MM -122.300 2026 09 14 17 00 120 1.5 MM\n",
            "AAMC1 37.772 -122.300 2026 09 14 17 00 999 1.5 MM\n",
            "AAMC1 37.772 -122.300 2026 09 14 17 00 120 -1 MM\n",
            "<html>\nnot a report\n</html>\n",
        ] {
            let e = parse_latest(&format!("{HEADER}{body}"), NOW).unwrap_err();
            assert!(e.contains("no reports"), "{body:?}: {e}");
        }
        // A file whose stations all report no wind is still a good file.
        assert_eq!(
            parse("46214 37.944 -123.466 2026 09 14 16 56 MM MM MM\n"),
            []
        );
    }

    #[test]
    fn a_row_needs_every_column_of_the_header() {
        // NDBC's whole header: a row cut off after GST, or with junk in a
        // column not read, isn't a report.
        let header = "\
#STN       LAT      LON  YYYY MM DD hh mm WDIR WSPD   GST WVHT  DPD APD MWD   PRES  PTDY  ATMP  WTMP  DEWP  VIS   TIDE
#text      deg      deg   yr mo day hr mn degT  m/s   m/s   m   sec sec degT   hPa   hPa  degC  degC  degC  nmi     ft
";
        for body in [
            "46214    37.944 -123.466 2026 09 14 16 56  MM    MM    MM\n",
            "AAMC1    37.772 -122.300 2026 09 14 17 00 120   1.5   2.1   MM  MM   MM  MM 1014.5  +0.5  17.4  19.6    MM   MM     MM MM\n",
            "AAMC1    37.772 -122.300 2026 09 14 17 00 120   1.5   2.1   MM  MM   MM  MM 1014.5  +0.5  17.4  junk    MM   MM     MM\n",
        ] {
            let e = parse_latest(&format!("{header}{body}"), NOW).unwrap_err();
            assert!(e.contains("no reports"), "{body:?}: {e}");
        }
        let whole = "AAMC1    37.772 -122.300 2026 09 14 17 00 120   1.5   2.1   MM  MM   MM  MM 1014.5  +0.5  17.4  19.6    MM   MM     MM\n";
        assert_eq!(
            ids(&winds(
                parse_latest(&format!("{header}{whole}"), NOW).unwrap()
            )),
            ["AAMC1"]
        );
    }

    #[test]
    fn keeps_a_stations_newest_report() {
        let s = parse(
            "\
AAMC1 37.772 -122.300 2026 09 14 16 00 100 1.0 MM
aamc1 37.772 -122.300 2026 09 14 17 00 120 1.5 MM
AAMC1 37.772 -122.300 2026 09 14 15 00 140 2.0 MM
RCMC1 37.923 -122.410 2026 09 14 16 00 120 1.5 MM
RCMC1 37.923 -122.410 2026 09 14 17 00  MM  MM MM
PXOC1 37.798 -122.393 2026 09 14 16 00 110 3.1 MM
PXOC1 37.798 -122.393 2026 09 14 17 00  MM 3.1 MM
",
        );
        // A newer report without a wind to draw hides an older one's.
        assert_eq!(ids(&s), ["AAMC1"]);
        assert_eq!(s[0].from_deg, Some(120.0));
    }

    #[test]
    fn finds_columns_by_the_header() {
        let moved = "#STN LAT LON WSPD GST WDIR YYYY MM DD hh mm\nAAMC1 37.772 -122.300 1.5 MM 360 2026 09 14 17 00\n";
        let s = winds(parse_latest(moved, NOW).unwrap());
        assert_eq!(s[0].from_deg, Some(0.0));
        assert!(
            parse_latest("AAMC1 37.772 -122.300 2026 09 14 17 00 120 1.5 2.1\n", NOW)
                .unwrap_err()
                .contains("no header")
        );
        assert!(
            parse_latest("#STN LAT LON YYYY MM DD hh mm WDIR GST\n", NOW)
                .unwrap_err()
                .contains("no WSPD column")
        );
    }

    #[test]
    fn shows_the_regions_recent_reports() {
        let r = parse_latest(
            &format!(
                "{HEADER}\
AAMC1 37.772 -122.300 2026 09 14 17 00 120 1.5 MM
OLD01 37.772 -122.300 2026 09 14 15 00 120 1.5 MM
AHEAD 37.772 -122.300 2026 09 14 18 00 120 1.5 MM
51201 21.673 -158.116 2026 09 14 17 00 120 1.5 MM
"
            ),
            NOW,
        )
        .unwrap();
        let now = time::unix(2026, 9, 14, 17, 30, 0);
        let shown: Vec<&str> = current(&r, Region::BAY, now)
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(shown, ["AAMC1"]);
    }

    #[test]
    fn a_fetch_cut_short_keeps_the_stations_it_missed() {
        let now = time::unix(2026, 9, 14, 17, 30, 0);
        let fetch = |rows: &str| parse_latest(&format!("{HEADER}{rows}"), NOW).unwrap();
        let shown = |kept: &BTreeMap<String, Report>| -> Vec<(String, Option<f64>)> {
            current(kept.values(), Region::BAY, now)
                .map(|s| (s.id.clone(), s.from_deg))
                .collect()
        };
        let mut kept = BTreeMap::new();
        merge(
            &mut kept,
            fetch(
                "\
AAMC1 37.772 -122.300 2026 09 14 17 00 120 1.5 MM
RCMC1 37.923 -122.410 2026 09 14 17 00 160 2.1 MM
",
            ),
            now,
        );
        // The next file stops after Alameda: Richmond keeps its report.
        merge(
            &mut kept,
            fetch("AAMC1 37.772 -122.300 2026 09 14 17 06 130 2.0 MM\n"),
            now,
        );
        assert_eq!(
            shown(&kept),
            [("AAMC1".into(), Some(130.0)), ("RCMC1".into(), Some(160.0))]
        );
        // A newer report without a wind hides the old one's, fetch to fetch.
        merge(
            &mut kept,
            fetch("RCMC1 37.923 -122.410 2026 09 14 17 12 MM MM MM\n"),
            now,
        );
        // And an older report doesn't replace a newer one.
        merge(
            &mut kept,
            fetch("AAMC1 37.772 -122.300 2026 09 14 16 00 100 1.0 MM\n"),
            now,
        );
        assert_eq!(shown(&kept), [("AAMC1".into(), Some(130.0))]);
        // Two hours on, what hasn't reported since is let go.
        merge(&mut kept, Vec::new(), now + 2 * time::HOUR);
        assert!(kept.is_empty());
    }

    #[test]
    fn broken_or_future_reports_dont_displace_a_good_one() {
        let now = time::unix(2026, 9, 14, 17, 30, 0);
        let fetch = |rows: &str| parse_latest(&format!("{HEADER}{rows}"), NOW).unwrap_or_default();
        let from = |kept: &BTreeMap<String, Report>| -> Vec<Option<f64>> {
            current(kept.values(), Region::BAY, now)
                .map(|s| s.from_deg)
                .collect()
        };
        let mut kept = BTreeMap::new();
        merge(
            &mut kept,
            fetch("AAMC1 37.772 -122.300 2026 09 14 17 00 120 1.5 MM\n"),
            now,
        );
        // Newer, but with a direction or speed that can't be: not a report.
        for broken in [
            "AAMC1 37.772 -122.300 2026 09 14 17 06 999 1.5 MM\n",
            "AAMC1 37.772 -122.300 2026 09 14 17 06 120 -1 MM\n",
        ] {
            merge(&mut kept, fetch(broken), now);
            assert_eq!(from(&kept), [Some(120.0)], "{broken:?}");
        }
        // Dated in 2099: never kept, so the next true report still counts.
        merge(
            &mut kept,
            fetch("AAMC1 37.772 -122.300 2099 09 14 17 00 200 1.5 MM\n"),
            now,
        );
        assert_eq!(kept["AAMC1"].time, time::unix(2026, 9, 14, 17, 0, 0));
        merge(
            &mut kept,
            fetch("AAMC1 37.772 -122.300 2026 09 14 17 12 140 1.5 MM\n"),
            now,
        );
        assert_eq!(from(&kept), [Some(140.0)]);
    }

    #[test]
    fn a_future_row_doesnt_hide_todays_in_the_same_file() {
        assert_eq!(NOW, time::unix(2026, 9, 14, 17, 30, 0));
        let today = "AAMC1 37.772 -122.300 2026 09 14 17 00 120 1.5 MM\n";
        let future = "AAMC1 37.772 -122.300 2099 09 14 17 00 200 1.5 MM\n";
        for rows in [format!("{today}{future}"), format!("{future}{today}")] {
            assert_eq!(parse(&rows)[0].from_deg, Some(120.0), "{rows:?}");
        }
        // A file of nothing but future rows has no reports in it.
        assert!(parse_latest(&format!("{HEADER}{future}"), NOW).is_err());
    }

    #[test]
    fn trims_names_for_a_chart() {
        let n = parse_names(
            "\
# STATION_ID | OWNER | TTYPE | HULL | NAME | PAYLOAD | LOCATION | TIMEZONE | FORECAST | NOTE
#
aamc1|O|Water Level Observation Network||9414750 - Alameda, CA||37.772 N 122.300 W|P| |
46026|N|3-meter foam buoy|3DV40|SAN FRANCISCO - 18NM West of San Francisco, CA|SCOOP payload|37.750 N|P| |
x1|N|||Boats &amp; Buoys||||
x2|N|||Clear\x1b[2J\x1b[H Screen||||
blank|N|||   ||||
short|N
",
        );
        assert_eq!(n["AAMC1"], "Alameda");
        assert_eq!(n["46026"], "SAN FRANCISCO - 18NM West of San Francisco");
        assert_eq!(n["X1"], "Boats & Buoys");
        assert_eq!(n["X2"], "Clear[2J[H Screen");
        assert_eq!(n.len(), 4);
    }

    #[test]
    fn writers_at_once_leave_one_whole_table() {
        let dir = std::env::temp_dir().join(format!("omawind-save-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let file = dir.join("ndbc").join("station_table.txt");
        let tables: Vec<String> = (0..8)
            .map(|i| format!("id{i}|N|||Station {i}||\n").repeat(20_000))
            .collect();
        std::thread::scope(|s| {
            for t in &tables {
                let file = &file;
                s.spawn(move || save(file, t.as_bytes()));
            }
        });
        let kept = fs::read_to_string(&file).unwrap();
        assert!(tables.contains(&kept), "a mixed or partial table");
        let left: Vec<_> = fs::read_dir(file.parent().unwrap()).unwrap().collect();
        assert_eq!(left.len(), 1, "temporary files left behind");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn keeps_names_a_week_and_falls_back_to_them() {
        let dir = std::env::temp_dir().join(format!("omawind-names-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let table = dir.join("table.txt");
        let gone = dir.join("gone.txt");
        let from = |file: &Path| Source {
            latest: String::new(),
            names: format!("file://{}", file.display()),
        };
        let age = |cache: &Path| {
            let old = SystemTime::now() - Duration::from_secs(8 * 24 * 3600);
            fs::File::options()
                .write(true)
                .open(names_file(cache))
                .unwrap()
                .set_modified(old)
                .unwrap();
        };
        let cache = dir.join("cache");

        fs::write(&table, "aamc1|O|||9414750 - Alameda, CA||\n").unwrap();
        assert_eq!(names(&cache, &from(&table)).unwrap()["AAMC1"], "Alameda");
        assert!(names_file(&cache).exists());
        // Within the week NDBC isn't asked.
        assert_eq!(names(&cache, &from(&gone)).unwrap()["AAMC1"], "Alameda");
        // After it, it is.
        age(&cache);
        // A table cut short to one station adds its name and forgets none.
        fs::write(&table, "rcmc1|PT|||9414863 - Richmond, CA||\n").unwrap();
        let n = names(&cache, &from(&table)).unwrap();
        assert_eq!(
            (n["RCMC1"].as_str(), n["AAMC1"].as_str()),
            ("Richmond", "Alameda")
        );
        let kept = parse_names(&fs::read_to_string(names_file(&cache)).unwrap());
        assert_eq!(kept, n);
        // And when it can't answer, the old copy stands in.
        age(&cache);
        assert_eq!(names(&cache, &from(&gone)).unwrap()["RCMC1"], "Richmond");
        // With nothing kept, the failure is said.
        assert!(names(&dir.join("empty"), &from(&gone)).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
