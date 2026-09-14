//! UTC times as Unix seconds, and the ISO 8601 text the protocol uses.
//! Civil dates from Howard Hinnant's days-from-civil algorithm.

use std::time::{SystemTime, UNIX_EPOCH};

pub const HOUR: i64 = 3600;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn unix(y: i64, m: u32, d: u32, h: u32, min: u32, s: u32) -> i64 {
    days_from_civil(y, m, d) * 86_400 + i64::from(h) * HOUR + i64::from(min) * 60 + i64::from(s)
}

/// `2026-09-14T03:00:00Z`.
pub fn iso(t: i64) -> String {
    let (y, m, d) = civil_from_days(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        s / 3600,
        s % 3600 / 60,
        s % 60
    )
}

/// `YYYYMMDD` for the UTC day containing `t`, as NOAA names its folders.
pub fn day_name(t: i64) -> String {
    let (y, m, d) = civil_from_days(t.div_euclid(86_400));
    format!("{y:04}{m:02}{d:02}")
}

/// Reads `2026-09-14T03:00:00Z` or `2026-09-14T03:00Z`: UTC only.
pub fn parse_iso(s: &str) -> Option<i64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day) = (d.next()?, d.next()?, d.next()?);
    if d.next().is_some() || y.len() != 4 || m.len() != 2 || day.len() != 2 {
        return None;
    }
    let mut t = time.split(':');
    let (h, min) = (t.next()?, t.next()?);
    let sec = t.next().unwrap_or("00");
    if t.next().is_some() || h.len() != 2 || min.len() != 2 || sec.len() != 2 {
        return None;
    }
    let num = |s: &str| {
        s.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| s.parse::<u32>().ok())?
    };
    let (y, m, day, h, min, sec) = (num(y)?, num(m)?, num(day)?, num(h)?, num(min)?, num(sec)?);
    let days_in = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) => 29,
        2 => 28,
        _ => return None,
    };
    if day == 0 || day > days_in || h > 23 || min > 59 || sec > 59 {
        return None;
    }
    Some(unix(i64::from(y), m, day, h, min, sec))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_iso() {
        let t = unix(2026, 9, 14, 3, 0, 0);
        assert_eq!(t, 1_789_354_800);
        assert_eq!(iso(t), "2026-09-14T03:00:00Z");
        assert_eq!(parse_iso("2026-09-14T03:00:00Z"), Some(t));
        assert_eq!(parse_iso("2026-09-14T03:00Z"), Some(t));
        assert_eq!(day_name(t), "20260914");
        for t in [0, 951_782_400, 4_107_542_399, -86_400] {
            assert_eq!(parse_iso(&iso(t)), Some(t), "{}", iso(t));
        }
    }

    #[test]
    fn refuses_what_isnt_a_utc_time() {
        for s in [
            "2026-09-14T03:00:00",
            "2026-09-14 03:00:00Z",
            "2026-02-29T00:00Z",
            "2026-13-01T00:00Z",
            "2026-09-14T24:00Z",
            "2026-9-14T03:00Z",
            "+026-09-14T03:00Z",
            "2026-09-14T03:00:00+00:00",
        ] {
            assert_eq!(parse_iso(s), None, "{s}");
        }
        assert!(parse_iso("2028-02-29T00:00Z").is_some());
    }
}
