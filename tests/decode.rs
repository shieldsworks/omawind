//! The decoder against eccodes, and the winds against pyproj: the numbers
//! in tests/fixtures/hrrr/f01.reference.txt come from scripts/reference.py,
//! never from omawind.

use omawind::{forecast::Forecast, grib, time};
use std::collections::HashMap;
use std::path::Path;

const RUN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/hrrr/2026091403"
);
const REFERENCE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/hrrr/f01.reference.txt"
);

/// `key=value` pairs from a reference line.
fn pairs(line: &str) -> HashMap<&str, &str> {
    line.split_whitespace()
        .filter_map(|w| w.split_once('='))
        .collect()
}

fn num(s: &str) -> f64 {
    s.parse().unwrap()
}

/// eccodes' short names for the fields omawind reads.
fn short_name(f: &grib::Field) -> &'static str {
    match f.name().as_str() {
        "UGRD" => "10u",
        "VGRD" => "10v",
        "GUST" => "gust",
        "MSLMA" => "mslma",
        _ => "?",
    }
}

#[test]
fn every_value_matches_eccodes() {
    let reference = std::fs::read_to_string(REFERENCE).unwrap();
    let fields = grib::parse(&std::fs::read(Path::new(RUN).join("f01.grib2")).unwrap()).unwrap();
    assert_eq!(fields.len(), 4);
    let mut checked = 0;
    for line in reference.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.first() {
            Some(&"field") => {
                let p = pairs(line);
                let f = fields
                    .iter()
                    .find(|f| short_name(f) == words[1])
                    .expect(words[1]);
                assert_eq!(
                    (f.discipline, f.category, f.number),
                    (
                        num(p["discipline"]) as u8,
                        num(p["category"]) as u8,
                        num(p["number"]) as u8
                    )
                );
                assert_eq!((f.grid.nx, f.grid.ny), (82, 88));
                assert_eq!(f.lead, 3600 * num(p["hour"]) as i64);
                assert_eq!(f.reference, time::unix(2026, 9, 14, 3, 0, 0));
                let values: Vec<f64> = f.values.iter().map(|&v| f64::from(v)).collect();
                let biggest = values.iter().fold(0f64, |m, v| m.max(v.abs()));
                // f32 rounding, at most half an ulp per value.
                let slack = values.len() as f64 * biggest * f64::from(f32::EPSILON);
                let sum: f64 = values.iter().sum();
                assert!(
                    (sum - num(p["sum"])).abs() <= slack,
                    "{}: sum {sum} vs {}",
                    words[1],
                    p["sum"]
                );
                let min = values.iter().copied().fold(f64::MAX, f64::min);
                let max = values.iter().copied().fold(f64::MIN, f64::max);
                for (ours, theirs) in [(min, num(p["min"])), (max, num(p["max"]))] {
                    assert!(
                        (ours - theirs).abs() <= 1e-6 * theirs.abs().max(1.0),
                        "{}: {ours} vs {theirs}",
                        words[1]
                    );
                }
                checked += 1;
            }
            Some(&"value") => {
                let f = fields.iter().find(|f| short_name(f) == words[1]).unwrap();
                let (i, theirs) = (num(words[2]) as usize, num(words[3]));
                let ours = f64::from(f.values[i]);
                assert!(
                    (ours - theirs).abs() <= 1e-5 * theirs.abs().max(1.0),
                    "{} {i}: {ours} vs {theirs}",
                    words[1]
                );
                checked += 1;
            }
            Some(&"latlon") => {
                let (i, lat, lon) = (num(words[1]) as usize, num(words[2]), num(words[3]));
                let (la, lo) = fields[0].grid.position(i);
                assert!(
                    (la - lat).abs() < 2e-6 && (lo - lon).abs() < 2e-6,
                    "{i}: {la},{lo} vs {lat},{lon}"
                );
                checked += 1;
            }
            _ => {}
        }
    }
    assert_eq!(checked, 4 + 4 * 7 + 7);
}

#[test]
fn winds_turned_to_true_north_match_pyproj() {
    let reference = std::fs::read_to_string(REFERENCE).unwrap();
    let f = Forecast::load(Path::new(RUN)).unwrap();
    assert_eq!(f.hours.len(), 3);
    let valid = time::unix(2026, 9, 14, 4, 0, 0);
    let positions: HashMap<usize, (f64, f64)> = reference
        .lines()
        .filter(|l| l.starts_with("latlon "))
        .map(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            (num(w[1]) as usize, (num(w[2]), num(w[3])))
        })
        .collect();
    let mut checked = 0;
    for line in reference.lines().filter(|l| l.starts_with("wind ")) {
        let i: usize = line.split_whitespace().nth(1).unwrap().parse().unwrap();
        let p = pairs(line);
        let (lat, lon) = positions[&i];
        let s = f
            .sample(lat, lon, valid)
            .unwrap_or_else(|| panic!("no sample at {i}"));
        let knots = num(p["speed_ms"]) * 3600.0 / 1852.0;
        assert!(
            (s.speed_kn - knots).abs() < 1e-3,
            "{i}: {} kn vs {knots}",
            s.speed_kn
        );
        let off = (s.from_deg - num(p["from_deg"]) + 540.0).rem_euclid(360.0) - 180.0;
        assert!(
            off.abs() < 1e-3,
            "{i}: from {} vs {}",
            s.from_deg,
            p["from_deg"]
        );
        checked += 1;
    }
    assert_eq!(checked, 7);
}

#[test]
fn samples_between_hours_and_points() {
    let f = Forecast::load(Path::new(RUN)).unwrap();
    let t0 = time::unix(2026, 9, 14, 3, 0, 0);
    assert_eq!((f.first(), f.last()), (t0, t0 + 2 * 3600));
    // Before the run and after its last hour there's nothing to say.
    assert!(f.sample(37.8, -122.5, t0 - 1).is_none());
    assert!(f.sample(37.8, -122.5, t0 + 2 * 3600 + 1).is_none());
    // Outside the grid.
    assert!(f.sample(40.5, -122.5, t0).is_none());
    // Halfway between two hours, a scalar is halfway between them.
    let (a, b, mid) = (
        f.sample(37.8, -122.5, t0).unwrap(),
        f.sample(37.8, -122.5, t0 + 3600).unwrap(),
        f.sample(37.8, -122.5, t0 + 1800).unwrap(),
    );
    let (pa, pb, pm) = (
        a.pressure_hpa.unwrap(),
        b.pressure_hpa.unwrap(),
        mid.pressure_hpa.unwrap(),
    );
    assert!((pm - (pa + pb) / 2.0).abs() < 1e-9);
    assert!(mid.gust_kn.is_some());
}

#[test]
fn a_field_is_thinned_to_the_most_points_asked_for() {
    let f = Forecast::load(Path::new(RUN)).unwrap();
    let t = time::unix(2026, 9, 14, 4, 0, 0);
    let bay = omawind::config::Region::BAY;
    let (step1, all) = f.field(&bay, t, 100_000).unwrap();
    assert_eq!(step1, 1);
    assert!(all.iter().all(|p| bay.contains(p.lat, p.lon)));
    // 3 km apart, the Bay's 2° by 2.2° holds a few thousand points.
    assert!((4000..7216).contains(&all.len()), "{}", all.len());
    let (step, few) = f.field(&bay, t, 300).unwrap();
    assert!(
        few.len() <= 300 && few.len() > 100 && step > 1,
        "{step} {}",
        few.len()
    );
    assert!(f.field(&bay, t + 86_400, 300).is_none());
}
