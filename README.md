# Omawind

Wind forecasts for [Omahoy](https://github.com/shieldsworks/omahoy), from GRIB.

**Status: planned.** Nothing to install yet.

## What it will do

- Download NOAA forecast models, HRRR for the coast and GFS offshore, for just
  the area and hours you need, and decode the GRIB2 files from scratch in Rust.
- Keep the last forecast on the boat, and say how old it is once the
  connection is gone.
- Show the forecast wind at the boat's position in a bar widget.
- Serve wind, gusts, pressure and waves to omahelm, which draws them over the
  chart.

## License

MIT
