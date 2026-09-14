//! A forecast: one model run's hours over the region, its winds turned to
//! true north, sampled at any position and time inside it.

use crate::config::Region;
use crate::grib::{self, Field};
use crate::grid::Grid;
use std::path::Path;

/// Metres per second to knots.
pub const KNOTS: f64 = 3600.0 / 1852.0;
/// HRRR's longest run.
pub const MAX_HOURS: u32 = 48;

pub fn hour_file(hour: u32) -> String {
    format!("f{hour:02}.grib2")
}

pub struct Hour {
    /// Unix seconds.
    pub valid: i64,
    /// Wind 10 m above ground, m/s, toward east and north.
    pub u: Vec<f32>,
    pub v: Vec<f32>,
    /// Gusts at the surface, m/s.
    pub gust: Option<Vec<f32>>,
    /// Pressure at mean sea level, pascals.
    pub pressure: Option<Vec<f32>>,
}

pub struct Forecast {
    /// The run, Unix seconds.
    pub run: i64,
    pub grid: Grid,
    /// Every point's latitude and longitude.
    pub positions: Vec<(f64, f64)>,
    /// Hourly, from the run's own hour, unbroken.
    pub hours: Vec<Hour>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub speed_kn: f64,
    /// Where the wind blows from, degrees true.
    pub from_deg: f64,
    pub gust_kn: Option<f64>,
    pub pressure_hpa: Option<f64>,
}

pub struct Point {
    pub lat: f64,
    pub lon: f64,
    pub sample: Sample,
}

fn find(
    fields: &[Field],
    category: u8,
    number: u8,
    surface: u8,
    level: Option<f64>,
) -> Option<&Field> {
    fields.iter().find(|f| {
        f.discipline == 0
            && f.category == category
            && f.number == number
            && f.surface == surface
            && (level.is_none() || f.level == level)
    })
}

impl Forecast {
    /// A run's hour files, `f00.grib2` onward, as far as they run unbroken.
    pub fn load(dir: &Path) -> Result<Forecast, String> {
        let mut hours: Vec<Vec<Field>> = Vec::new();
        for hour in 0..=MAX_HOURS {
            let path = dir.join(hour_file(hour));
            let Ok(bytes) = std::fs::read(&path) else {
                break;
            };
            let fields = grib::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            hours.push(fields);
        }
        Forecast::from_hours(hours).map_err(|e| format!("{}: {e}", dir.display()))
    }

    /// Each element holds one forecast hour's fields, hour 0 first.
    pub fn from_hours(hours: Vec<Vec<Field>>) -> Result<Forecast, String> {
        let mut out: Option<Forecast> = None;
        for (h, fields) in hours.into_iter().enumerate() {
            let (Some(u), Some(v)) = (
                find(&fields, 2, 2, 103, Some(10.0)),
                find(&fields, 2, 3, 103, Some(10.0)),
            ) else {
                return Err(format!("hour {h} has no 10 m wind"));
            };
            let gust = find(&fields, 2, 22, 1, None);
            let pressure = find(&fields, 3, 198, 101, None).or(find(&fields, 3, 1, 101, None));
            let used = [Some(u), Some(v), gust, pressure];
            for f in used.iter().flatten() {
                if f.grid != u.grid || f.reference != u.reference || f.lead != h as i64 * 3600 {
                    return Err(format!(
                        "hour {h}: {} doesn't match the run, hour or grid",
                        f.name()
                    ));
                }
            }
            let forecast = out.get_or_insert_with(|| {
                let positions = (0..u.grid.len()).map(|i| u.grid.position(i)).collect();
                Forecast {
                    run: u.reference,
                    grid: u.grid.clone(),
                    positions,
                    hours: Vec::new(),
                }
            });
            if u.grid != forecast.grid || u.reference != forecast.run {
                return Err(format!("hour {h} is from another run or grid"));
            }
            let (mut east, mut north) = (u.values.clone(), v.values.clone());
            for (i, (e, n)) in east.iter_mut().zip(north.iter_mut()).enumerate() {
                let (a, b) =
                    forecast
                        .grid
                        .to_earth(f64::from(*e), f64::from(*n), forecast.positions[i].1);
                (*e, *n) = (a as f32, b as f32);
            }
            forecast.hours.push(Hour {
                valid: u.valid(),
                u: east,
                v: north,
                gust: gust.map(|f| f.values.clone()),
                pressure: pressure.map(|f| f.values.clone()),
            });
        }
        out.ok_or_else(|| "no forecast hours".into())
    }

    pub fn first(&self) -> i64 {
        self.hours[0].valid
    }

    pub fn last(&self) -> i64 {
        self.hours[self.hours.len() - 1].valid
    }

    /// The hours either side of `t` and how far between them it is.
    fn bracket(&self, t: i64) -> Option<(&Hour, &Hour, f64)> {
        if t < self.first() || t > self.last() {
            return None;
        }
        let k = self.hours.partition_point(|h| h.valid <= t).max(1) - 1;
        let a = &self.hours[k];
        let b = self.hours.get(k + 1).unwrap_or(a);
        let w = if b.valid > a.valid {
            (t - a.valid) as f64 / (b.valid - a.valid) as f64
        } else {
            0.0
        };
        Some((a, b, w))
    }

    /// The forecast at a position and time: bilinear between the four
    /// points around it, linear between the hours either side. None outside
    /// the grid or the forecast's hours.
    pub fn sample(&self, lat: f64, lon: f64, t: i64) -> Option<Sample> {
        let (fi, fj) = self.grid.locate(lat, lon);
        let (nx, ny) = (self.grid.nx, self.grid.ny);
        let (last_i, last_j) = ((nx - 1) as f64, (ny - 1) as f64);
        // A position on the edge, rounded, may land a hair outside it: a
        // thousandth of a cell is 3 m on HRRR's grid.
        const EDGE: f64 = 1e-3;
        if !(fi >= -EDGE && fj >= -EDGE && fi <= last_i + EDGE && fj <= last_j + EDGE) {
            return None;
        }
        let (fi, fj) = (fi.clamp(0.0, last_i), fj.clamp(0.0, last_j));
        let i0 = (fi.floor() as usize).min(nx - 2);
        let j0 = (fj.floor() as usize).min(ny - 2);
        let (wx, wy) = (fi - i0 as f64, fj - j0 as f64);
        let corners = [
            (j0 * nx + i0, (1.0 - wx) * (1.0 - wy)),
            (j0 * nx + i0 + 1, wx * (1.0 - wy)),
            ((j0 + 1) * nx + i0, (1.0 - wx) * wy),
            ((j0 + 1) * nx + i0 + 1, wx * wy),
        ];
        self.combine(t, &corners)
    }

    /// Weighted points, weighted hours.
    fn combine(&self, t: i64, points: &[(usize, f64)]) -> Option<Sample> {
        let (a, b, w) = self.bracket(t)?;
        let at = |values: &[f32]| -> Option<f64> {
            let mut sum = 0.0;
            for &(i, weight) in points {
                let v = f64::from(values[i]);
                if v.is_nan() {
                    return None;
                }
                sum += v * weight;
            }
            Some(sum)
        };
        let both = |x: Option<&Vec<f32>>, y: Option<&Vec<f32>>| -> Option<f64> {
            Some(at(x?)? * (1.0 - w) + at(y?)? * w)
        };
        let u = both(Some(&a.u), Some(&b.u))?;
        let v = both(Some(&a.v), Some(&b.v))?;
        Some(Sample {
            speed_kn: u.hypot(v) * KNOTS,
            from_deg: (u.atan2(v).to_degrees() + 180.0).rem_euclid(360.0),
            gust_kn: both(a.gust.as_ref(), b.gust.as_ref()).map(|g| g * KNOTS),
            pressure_hpa: both(a.pressure.as_ref(), b.pressure.as_ref()).map(|p| p / 100.0),
        })
    }

    /// The model's own points inside `area` at time `t`, every `step`th
    /// column and row, `step` the smallest that keeps them to `max`. Steps
    /// count from the grid's first point, so the points stay put as the
    /// area moves.
    pub fn field(&self, area: &Region, t: i64, max: usize) -> Option<(usize, Vec<Point>)> {
        self.bracket(t)?;
        let (nx, ny) = (self.grid.nx, self.grid.ny);
        // The area's edges, traced, bound the columns and rows it covers.
        let (mut i_lo, mut i_hi, mut j_lo, mut j_hi) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for k in 0..=16 {
            let f = f64::from(k) / 16.0;
            let lat = area.south + (area.north - area.south) * f;
            let lon = area.west + (area.east - area.west) * f;
            for (la, lo) in [
                (lat, area.west),
                (lat, area.east),
                (area.south, lon),
                (area.north, lon),
            ] {
                let (fi, fj) = self.grid.locate(la, lo);
                (i_lo, i_hi) = (i_lo.min(fi), i_hi.max(fi));
                (j_lo, j_hi) = (j_lo.min(fj), j_hi.max(fj));
            }
        }
        let clamp = |v: f64, n: usize| v.clamp(0.0, (n - 1) as f64) as usize;
        let (i0, i1) = (clamp(i_lo.floor() - 1.0, nx), clamp(i_hi.ceil() + 1.0, nx));
        let (j0, j1) = (clamp(j_lo.floor() - 1.0, ny), clamp(j_hi.ceil() + 1.0, ny));
        let inside = |step: usize| {
            (j0..=j1)
                .filter(move |j| j % step == 0)
                .flat_map(move |j| {
                    (i0..=i1)
                        .filter(move |i| i % step == 0)
                        .map(move |i| j * nx + i)
                })
                .filter(|&k| area.contains(self.positions[k].0, self.positions[k].1))
        };
        let all = inside(1).count();
        let max = max.max(1);
        let mut step = ((all as f64 / max as f64).sqrt().ceil() as usize).max(1);
        while inside(step).count() > max {
            step += 1;
        }
        let points = inside(step)
            .filter_map(|k| {
                let (lat, lon) = self.positions[k];
                Some(Point {
                    lat,
                    lon,
                    sample: self.combine(t, &[(k, 1.0)])?,
                })
            })
            .collect();
        Some((step, points))
    }
}
