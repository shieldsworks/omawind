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

/// The fields omawind reads from one hour's file.
pub struct Picked<'a> {
    pub u: &'a Field,
    pub v: &'a Field,
    pub gust: Option<&'a Field>,
    pub pressure: Option<&'a Field>,
}

/// Picks out an hour's fields, checked against each other, the run and
/// the hour.
pub fn pick(fields: &[Field], run: i64, hour: usize) -> Result<Picked<'_>, String> {
    let (Some(u), Some(v)) = (
        find(fields, 2, 2, 103, Some(10.0)),
        find(fields, 2, 3, 103, Some(10.0)),
    ) else {
        return Err(format!("hour {hour} has no 10 m wind"));
    };
    let gust = find(fields, 2, 22, 1, None);
    let pressure = find(fields, 3, 198, 101, None).or(find(fields, 3, 1, 101, None));
    for f in [Some(u), Some(v), gust, pressure].into_iter().flatten() {
        if f.grid != u.grid
            || f.values.len() != u.grid.len()
            || f.reference != run
            || f.lead != hour as i64 * 3600
        {
            return Err(format!(
                "hour {hour}: {} doesn't match the run, hour or grid",
                f.name()
            ));
        }
    }
    Ok(Picked {
        u,
        v,
        gust,
        pressure,
    })
}

impl Forecast {
    /// A run's hour files, `f00.grib2` onward, as far as they run unbroken
    /// and good. A bad hour ends the forecast there; a bad first hour, or
    /// none, is an error.
    pub fn load(dir: &Path) -> Result<Forecast, String> {
        let mut forecast: Option<Forecast> = None;
        for hour in 0..=MAX_HOURS {
            let path = dir.join(hour_file(hour));
            let Ok(bytes) = std::fs::read(&path) else {
                break;
            };
            let added = grib::parse(&bytes).and_then(|fields| {
                if let Some(f) = forecast.as_mut() {
                    f.push(&fields)
                } else {
                    Forecast::start(&fields).map(|f| forecast = Some(f))
                }
            });
            if let Err(e) = added {
                if forecast.is_none() {
                    return Err(format!("{}: {e}", path.display()));
                }
                break;
            }
        }
        forecast.ok_or_else(|| format!("{}: no forecast hours", dir.display()))
    }

    /// Each element holds one forecast hour's fields, hour 0 first.
    pub fn from_hours(hours: Vec<Vec<Field>>) -> Result<Forecast, String> {
        let (first, rest) = hours.split_first().ok_or("no forecast hours")?;
        let mut forecast = Forecast::start(first)?;
        for fields in rest {
            forecast.push(fields)?;
        }
        Ok(forecast)
    }

    /// A forecast of hour 0.
    fn start(fields: &[Field]) -> Result<Forecast, String> {
        let u = find(fields, 2, 2, 103, Some(10.0)).ok_or("hour 0 has no 10 m wind")?;
        let mut forecast = Forecast {
            run: u.reference,
            grid: u.grid.clone(),
            positions: (0..u.grid.len()).map(|i| u.grid.position(i)).collect(),
            hours: Vec::new(),
        };
        forecast.push(fields)?;
        Ok(forecast)
    }

    /// Adds the next hour, its winds turned to true north.
    fn push(&mut self, fields: &[Field]) -> Result<(), String> {
        let h = self.hours.len();
        let p = pick(fields, self.run, h)?;
        if p.u.grid != self.grid {
            return Err(format!("hour {h} is on another grid"));
        }
        let (mut east, mut north) = (p.u.values.clone(), p.v.values.clone());
        for (i, (e, n)) in east.iter_mut().zip(north.iter_mut()).enumerate() {
            let (a, b) = self
                .grid
                .to_earth(f64::from(*e), f64::from(*n), self.positions[i].1);
            (*e, *n) = (a as f32, b as f32);
            // A gap stays a gap; a wind beyond f32 isn't a wind.
            if e.is_infinite() || n.is_infinite() {
                return Err(format!("hour {h} has a wind too strong to be real"));
            }
        }
        self.hours.push(Hour {
            valid: p.u.valid(),
            u: east,
            v: north,
            gust: p.gust.map(|f| f.values.clone()),
            pressure: p.pressure.map(|f| f.values.clone()),
        });
        Ok(())
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
        // Within a hair of a point is on it, so a gap beside it doesn't count.
        let snap = |w: f64| {
            if w < 1e-6 {
                0.0
            } else if w > 1.0 - 1e-6 {
                1.0
            } else {
                w
            }
        };
        let (wx, wy) = (snap(fi - i0 as f64), snap(fj - j0 as f64));
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
        // Only what carries weight counts: a gap, or a field one hour
        // lacks, matters only when it's part of the answer.
        let at = |values: &[f32]| -> Option<f64> {
            let mut sum = 0.0;
            for &(i, weight) in points.iter().filter(|p| p.1 > 0.0) {
                let v = f64::from(values[i]);
                if !v.is_finite() {
                    return None;
                }
                sum += v * weight;
            }
            Some(sum)
        };
        let both = |x: Option<&Vec<f32>>, y: Option<&Vec<f32>>| -> Option<f64> {
            let before = if w < 1.0 { at(x?)? * (1.0 - w) } else { 0.0 };
            let after = if w > 0.0 { at(y?)? * w } else { 0.0 };
            Some(before + after)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Lambert;

    fn field(number: u8, surface: u8, level: Option<f64>, hour: i64, values: Vec<f32>) -> Field {
        let grid = Grid::lambert(Lambert {
            nx: 2,
            ny: 2,
            la1: 37.8,
            lo1: -122.5,
            lov: -97.5,
            lad: 38.5,
            latin1: 38.5,
            latin2: 38.5,
            dx: 3000.0,
            dy: 3000.0,
            radius: 6_371_229.0,
            winds_along_grid: false,
            rows_north: true,
        })
        .unwrap();
        Field {
            discipline: 0,
            category: 2,
            number,
            surface,
            level,
            reference: 0,
            lead: hour * 3600,
            grid,
            values,
        }
    }

    /// Wind toward the east at `u` m/s, and gusts of 5 if `gust`.
    fn hour(h: i64, gust: bool, u: Vec<f32>) -> Vec<Field> {
        let mut f = vec![
            field(2, 103, Some(10.0), h, u),
            field(3, 103, Some(10.0), h, vec![0.0; 4]),
        ];
        if gust {
            f.push(field(22, 1, None, h, vec![5.0; 4]));
        }
        f
    }

    #[test]
    fn what_carries_no_weight_doesnt_count() {
        // Gusts in the first hour only, and a gap beside the first point.
        let f = Forecast::from_hours(vec![
            hour(0, true, vec![1.0, f32::NAN, 1.0, 1.0]),
            hour(1, false, vec![1.0; 4]),
        ])
        .unwrap();
        let (lat, lon) = f.positions[0];
        let on_the_hour = f.sample(lat, lon, 0).unwrap();
        assert!((on_the_hour.speed_kn - KNOTS).abs() < 1e-9);
        assert_eq!(on_the_hour.from_deg, 270.0);
        assert!((on_the_hour.gust_kn.unwrap() - 5.0 * KNOTS).abs() < 1e-9);
        // Halfway to the next hour, which has none, the gust is unknown.
        assert_eq!(f.sample(lat, lon, 1800).unwrap().gust_kn, None);
        // Halfway to the gap, the wind is unknown.
        let (lat2, lon2) = f.positions[1];
        assert!(
            f.sample((lat + lat2) / 2.0, (lon + lon2) / 2.0, 0)
                .is_none()
        );
    }

    #[test]
    fn a_wind_too_strong_to_turn_is_refused() {
        let mut fields = hour(0, false, vec![3e38; 4]);
        fields[1].values = vec![3e38; 4];
        for f in &mut fields {
            f.grid.winds_along_grid = true;
        }
        let e = Forecast::from_hours(vec![fields]).err().unwrap();
        assert!(e.contains("too strong"), "{e}");
    }

    #[test]
    fn a_field_shorter_than_its_grid_is_refused() {
        let mut short = hour(0, false, vec![1.0; 4]);
        short[1].values.truncate(3);
        assert!(Forecast::from_hours(vec![short]).is_err());
    }
}
