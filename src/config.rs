//! `~/.config/omawind/config.toml`: the waters to forecast, and home, where
//! the wind is reported when there's no GPS.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub south: f64,
    pub west: f64,
    pub north: f64,
    pub east: f64,
}

impl Region {
    /// San Francisco Bay and its approaches, Bodega Bay to Monterey Bay.
    pub const BAY: Region = Region {
        south: 36.8,
        west: -123.8,
        north: 38.8,
        east: -121.6,
    };

    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        (self.south..=self.north).contains(&lat) && (self.west..=self.east).contains(&lon)
    }

    /// `south,west,north,east`, as a run's `region` file keeps it.
    pub fn key(&self) -> String {
        format!(
            "{:.4},{:.4},{:.4},{:.4}",
            self.south, self.west, self.north, self.east
        )
    }

    /// Four numbers: south, west, north, east.
    pub fn parse(s: &str) -> Option<Region> {
        let n: Vec<f64> = s
            .split(',')
            .map(|p| p.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
            .collect::<Option<_>>()?;
        let &[south, west, north, east] = n.as_slice() else {
            return None;
        };
        Some(Region {
            south,
            west,
            north,
            east,
        })
    }

    /// Inside HRRR's reach, and no bigger than a few hundred miles a side,
    /// so every hour stays a small download.
    pub fn check(&self) -> Result<(), String> {
        if !(self.south < self.north && self.west < self.east) {
            return Err("south must be below north and west below east".into());
        }
        if self.north - self.south > 8.0 || self.east - self.west > 10.0 {
            return Err("at most 8° of latitude by 10° of longitude".into());
        }
        if self.south < 21.2 || self.north > 52.6 || self.west < -134.0 || self.east > -61.0 {
            return Err("outside HRRR's forecast, which covers the lower 48 and its coasts".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub region: Region,
    pub home: (f64, f64),
}

impl Default for Settings {
    /// The Bay, and home at the Berkeley Marina, where omahelm also opens.
    fn default() -> Self {
        Settings {
            region: Region::BAY,
            home: (37.8663, -122.3148),
        }
    }
}

impl Settings {
    /// Unknown keys and bad values are reported and left at their defaults.
    pub fn parse(text: &str) -> (Settings, Vec<String>) {
        let mut s = Settings::default();
        let mut problems = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() || line.starts_with('[') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                problems.push(format!("line {}: expected key = value", n + 1));
                continue;
            };
            let (k, v) = (k.trim(), v.trim().trim_matches('"'));
            match k {
                "region" => match Region::parse(v)
                    .ok_or_else(|| "expected south, west, north, east in degrees".to_string())
                {
                    Ok(r) => match r.check() {
                        Ok(()) => s.region = r,
                        Err(e) => problems.push(format!("region: {e}")),
                    },
                    Err(e) => problems.push(format!("region: {e}")),
                },
                "home" => match v.split_once(',').and_then(|(a, b)| {
                    let (lat, lon) = (a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?);
                    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon))
                        .then_some((lat, lon))
                }) {
                    Some(home) => s.home = home,
                    None => problems.push("home: expected latitude, longitude in degrees".into()),
                },
                other => problems.push(format!("unknown setting {other}")),
            }
        }
        (s, problems)
    }
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

pub fn config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("OMAWIND_CONFIG") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir().join(".config"));
    base.join("omawind/config.toml")
}

pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir().join(".cache"));
    base.join("omawind")
}

/// The settings file, or the defaults when there isn't one.
pub fn load(path: &std::path::Path) -> (Settings, Vec<String>) {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let (s, p) = Settings::parse(&text);
            (
                s,
                p.into_iter().map(|p| format!("config.toml: {p}")).collect(),
            )
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Settings::default(), Vec::new()),
        Err(e) => (Settings::default(), vec![format!("config.toml: {e}")]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_region_and_home() {
        let (s, p) = Settings::parse(
            "# Puget Sound\nregion = 47.0, -123.5, 48.8, -122.0\nhome = \"47.68, -122.41\"\n",
        );
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(
            s.region,
            Region {
                south: 47.0,
                west: -123.5,
                north: 48.8,
                east: -122.0
            }
        );
        assert_eq!(s.home, (47.68, -122.41));
    }

    #[test]
    fn reports_bad_settings_and_keeps_the_defaults() {
        let (s, p) = Settings::parse(
            "region = 38, -123\nhome = north\nregion = 38.8, -121.6, 36.8, -123.8\nregion = 1, 2, 3, 4\nwind = strong\nnonsense\n",
        );
        assert_eq!(s, Settings::default());
        assert_eq!(p.len(), 6, "{p:?}");
        assert!(p[2].contains("south must be below north"));
        assert!(p[3].contains("outside HRRR"));
        assert_eq!(p[4], "unknown setting wind");
    }

    #[test]
    fn a_region_key_is_stable() {
        assert_eq!(Region::BAY.key(), "36.8000,-123.8000,38.8000,-121.6000");
        assert_eq!(Region::parse(&Region::BAY.key()), Some(Region::BAY));
        assert!(Region::BAY.check().is_ok());
    }
}
