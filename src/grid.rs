//! Model grids. HRRR's is Lambert conformal (GRIB2 template 3.30) on a
//! sphere: this places its points, finds a position among them, and turns
//! its winds from the grid's axes to true north.

use std::f64::consts::FRAC_PI_4;

#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub nx: usize,
    pub ny: usize,
    /// The first point, degrees.
    pub la1: f64,
    pub lo1: f64,
    /// The meridian parallel to the grid's y axis, and the latitude where
    /// the spacing is true.
    pub lov: f64,
    pub lad: f64,
    pub latin1: f64,
    pub latin2: f64,
    /// Spacing at `lad`, metres.
    pub dx: f64,
    pub dy: f64,
    /// Earth's radius, metres.
    pub radius: f64,
    /// Winds are given along the grid's x and y, not east and north.
    pub winds_along_grid: bool,
    /// Rows run south to north (scanning mode bit 2).
    pub rows_north: bool,
    // Derived: the cone constant, R·F, and the first point and spacing in
    // projected metres.
    n: f64,
    rf: f64,
    x1: f64,
    y1: f64,
    sx: f64,
    sy: f64,
}

pub struct Lambert {
    pub nx: usize,
    pub ny: usize,
    pub la1: f64,
    pub lo1: f64,
    pub lov: f64,
    pub lad: f64,
    pub latin1: f64,
    pub latin2: f64,
    pub dx: f64,
    pub dy: f64,
    pub radius: f64,
    pub winds_along_grid: bool,
    pub rows_north: bool,
}

/// Longitude difference in (-180, 180].
fn wrap(d: f64) -> f64 {
    let d = d.rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

fn t(lat_deg: f64) -> f64 {
    (FRAC_PI_4 + lat_deg.to_radians() / 2.0).tan()
}

impl Grid {
    pub fn lambert(g: Lambert) -> Result<Grid, String> {
        let ok = g.nx >= 2
            && g.ny >= 2
            && g.dx > 0.0
            && g.dy > 0.0
            && g.radius > 0.0
            && [g.latin1, g.latin2, g.lad, g.la1]
                .iter()
                .all(|l| l.abs() < 89.0);
        // A northern cone: HRRR's, and every NOAA Lambert grid over the US.
        if !ok || g.latin1 <= 0.0 || g.latin2 <= 0.0 {
            return Err("unsupported Lambert grid: only northern cones".into());
        }
        let (p1, p2) = (g.latin1.to_radians(), g.latin2.to_radians());
        let n = if (g.latin1 - g.latin2).abs() < 1e-9 {
            p1.sin()
        } else {
            (p1.cos() / p2.cos()).ln() / (t(g.latin2) / t(g.latin1)).ln()
        };
        let rf = g.radius * p1.cos() * t(g.latin1).powf(n) / n;
        let mut grid = Grid {
            nx: g.nx,
            ny: g.ny,
            la1: g.la1,
            lo1: g.lo1,
            lov: g.lov,
            lad: g.lad,
            latin1: g.latin1,
            latin2: g.latin2,
            dx: g.dx,
            dy: g.dy,
            radius: g.radius,
            winds_along_grid: g.winds_along_grid,
            rows_north: g.rows_north,
            n,
            rf,
            x1: 0.0,
            y1: 0.0,
            sx: 0.0,
            sy: 0.0,
        };
        // The spacing is true at LaD, so on the map it's scaled by k there.
        let k = n * grid.rho(g.lad) / (g.radius * g.lad.to_radians().cos());
        grid.sx = g.dx * k;
        grid.sy = g.dy * k;
        (grid.x1, grid.y1) = grid.project(g.la1, g.lo1);
        Ok(grid)
    }

    pub fn len(&self) -> usize {
        self.nx * self.ny
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn rho(&self, lat: f64) -> f64 {
        self.rf / t(lat).powf(self.n)
    }

    /// Projected metres, from the cone's apex.
    fn project(&self, lat: f64, lon: f64) -> (f64, f64) {
        let rho = self.rho(lat);
        let theta = self.n * wrap(lon - self.lov).to_radians();
        (rho * theta.sin(), -rho * theta.cos())
    }

    fn unproject(&self, x: f64, y: f64) -> (f64, f64) {
        let rho = x.hypot(y);
        let theta = x.atan2(-y);
        let lon = wrap(self.lov + (theta / self.n).to_degrees());
        let lat = 2.0 * (self.rf / rho).powf(1.0 / self.n).atan() - std::f64::consts::FRAC_PI_2;
        (lat.to_degrees(), lon)
    }

    /// Point `index`'s latitude and longitude, degrees, west negative.
    pub fn position(&self, index: usize) -> (f64, f64) {
        let (i, j) = (index % self.nx, index / self.nx);
        let y = if self.rows_north {
            self.y1 + j as f64 * self.sy
        } else {
            self.y1 - j as f64 * self.sy
        };
        self.unproject(self.x1 + i as f64 * self.sx, y)
    }

    /// Where a position falls among the points, as fractional column and
    /// row. Outside the grid when either is below 0 or past the last.
    pub fn locate(&self, lat: f64, lon: f64) -> (f64, f64) {
        let (x, y) = self.project(lat, lon);
        let fj = (y - self.y1) / self.sy;
        (
            (x - self.x1) / self.sx,
            if self.rows_north { fj } else { -fj },
        )
    }

    /// The wind `(u, v)` at a point of longitude `lon`, turned to east and
    /// north if the grid gives it along its own axes. The grid's y axis is
    /// `n·(lon − LoV)` from true north there.
    pub fn to_earth(&self, u: f64, v: f64, lon: f64) -> (f64, f64) {
        if !self.winds_along_grid {
            return (u, v);
        }
        let a = self.n * wrap(lon - self.lov).to_radians();
        let (s, c) = a.sin_cos();
        (c * u + s * v, -s * u + c * v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HRRR's grid, cut to the Bay as NOAA's filter cuts it.
    pub fn bay() -> Grid {
        Grid::lambert(Lambert {
            nx: 82,
            ny: 88,
            la1: 36.364046,
            lo1: -123.635023,
            lov: -97.5,
            lad: 38.5,
            latin1: 38.5,
            latin2: 38.5,
            dx: 3000.0,
            dy: 3000.0,
            radius: 6_371_229.0,
            winds_along_grid: true,
            rows_north: true,
        })
        .unwrap()
    }

    #[test]
    fn places_points_where_eccodes_does() {
        // tests/fixtures/hrrr/f01.reference.txt
        let g = bay();
        for (index, lat, lon) in [
            (0, 36.364046, -123.635023),
            (81, 36.946051, -121.010803),
            (82, 36.389926, -123.644406),
            (3000, 37.652249, -122.408667),
            (7215, 39.214360, -121.776206),
        ] {
            let (la, lo) = g.position(index);
            assert!(
                (la - lat).abs() < 2e-6 && (lo - lon).abs() < 2e-6,
                "{index}: {la} {lo}"
            );
        }
    }

    #[test]
    fn locating_a_point_finds_it() {
        let g = bay();
        for index in [0, 1, 81, 82, 3000, 7215] {
            let (lat, lon) = g.position(index);
            let (fi, fj) = g.locate(lat, lon);
            assert!((fi - (index % 82) as f64).abs() < 1e-6, "{index} {fi}");
            assert!((fj - (index / 82) as f64).abs() < 1e-6, "{index} {fj}");
        }
    }

    #[test]
    fn turns_grid_winds_to_true_north() {
        // Grid north at the Bay's first point is 16.27° east of true north
        // (pyproj, by finite differences): a wind along the grid's y axis
        // blows toward 343.73° true.
        let g = bay();
        let (u, v) = g.to_earth(0.0, 1.0, -123.635023);
        let toward = u.atan2(v).to_degrees().rem_euclid(360.0);
        assert!((toward - (360.0 - 16.269434)).abs() < 1e-4, "{toward}");
        assert!((u.hypot(v) - 1.0).abs() < 1e-12);
    }
}
