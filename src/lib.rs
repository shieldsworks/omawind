//! omawind: NOAA's HRRR wind forecast for the waters around the boat,
//! downloaded, decoded from GRIB from scratch, and served to Omahoy apps.

pub mod config;
pub mod engine;
pub mod fetch;
pub mod forecast;
pub mod grib;
pub mod grid;
pub mod keel;
pub mod time;
