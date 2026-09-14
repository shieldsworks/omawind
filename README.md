# Omawind

Wind forecasts for [Omahoy](https://github.com/shieldsworks/omahoy), from GRIB.

Omawind is a Rust engine and an Omarchy bar widget. It downloads NOAA's HRRR
forecast for the waters around the boat, decodes the GRIB files itself, and
puts the forecast wind at the boat in your bar.

**Status: early.** It fetches and serves HRRR over San Francisco Bay, and
the wind NOAA's stations there measured, and shows the forecast in the bar.
[omahelm](https://github.com/shieldsworks/omahelm) draws both on the chart.

## What it does now

- Finds the newest run of NOAA's HRRR, a 3 km forecast of the lower 48 and
  its coastal waters that runs every hour, and downloads just the region and
  four fields: wind 10 m above the water, gusts, and sea-level pressure.
  About 32 KB an hour for the Bay, 18 to 48 hours a run, through NOAA's
  NOMADS filter.
- Decodes GRIB2 from scratch: the Lambert conformal grid, simple packing and
  bitmaps. Every value matches ECMWF's eccodes on a real Bay run, and the
  grid's winds are turned to true north, checked against pyproj.
- Keeps the forecast on disk, so it still works with the connection down,
  and says how old it is.
- Reports the wind where the boat is, from
  [omakeel](https://github.com/shieldsworks/omakeel)'s GPS, or at home
  without one: now, and hour by hour to the end of the run.
- Reads the wind measured now at NOAA's stations in the region, every 10
  minutes: the buoys offshore, and around the Bay the piers and tide gauges
  of NOAA's PORTS program. It's the National Data Buoy Center's plain-text
  latest reports, read from scratch, named from its station table.
- Serves all of that, and wind fields for the chart, over a Unix socket as
  newline-delimited JSON ([docs/protocol.md](docs/protocol.md)).
- The bar shows it like `WSW 14G19 kn`, in the theme's urgent color at 21
  knots or more. Click for the hours ahead.

## Use

Add the widget to the Omarchy bar. It starts the engine itself.

```sh
omarchy plugin add https://github.com/shieldsworks/omawind.git --enable
cd ~/.config/omarchy/plugins/org.omahoy.wind && cargo build --release --locked
```

Or run the engine from a terminal:

```sh
omawind fetch     # download the newest run now
omawind at        # print it at home, or: omawind at 37.81,-122.48
omawind stations  # the wind the region's stations measured
omawind run       # serve it all, checking NOAA every 10 minutes
```

```
HRRR 2026-09-14T03:00:00Z at 37.8663, -122.3148
UTC                    from     kn   gust      hPa
2026-09-14T19:00:00Z   224°    4.2    6.1   1013.6
2026-09-14T20:00:00Z   225°    6.2    8.9   1012.6
2026-09-14T21:00:00Z   206°    6.0   11.5   1012.1
```

```
NDBC                                 UTC  from     kn   gust
46026   SAN FRANCISCO - 18NM West  17:30  330°    7.8   11.7
AAMC1   Alameda                    17:18  110°    2.9    4.1
FTPC1   San Francisco              17:18  250°    7.0   11.1
OMHC1   Oakland Middle Harbor Met  17:18  220°    4.1    6.0
RCMC1   Richmond                   17:18  160°    6.0    8.0
```

Settings live in `~/.config/omawind/config.toml`. Both default to the Bay:

```toml
region = 36.8, -123.8, 38.8, -121.6   # south, west, north, east
home = 37.8663, -122.3148             # the wind without a GPS
```

## Limits

- HRRR only, so US waters only. GFS, for offshore, is next. Its GRIB uses
  complex packing, which omawind doesn't decode yet.
- The region is fixed by the settings; it doesn't follow the boat yet.
- A forecast is a model, not an observation. HRRR can miss the sea breeze by
  hours and knots. Look at the water.
- Stations stand where NOAA put them, mostly on piers. An anemometer may not
  be 10 m up, and a pier's may be sheltered. A station tells you the wind
  there, not in the middle of the channel.

## Develop

Build a checkout with [mise](https://mise.jdx.dev) and Rust 1.98:

```sh
mise install
mise lint                   # rustfmt and clippy
mise test                   # the decoder against eccodes, and the socket
scripts/link-plugin.sh      # point the Omarchy bar at this checkout
```

`tests/fixtures/hrrr/2026091403` is three hours of a real HRRR run over the
Bay. `scripts/reference.py` wrote the values it's checked against, with
eccodes and pyproj in a throwaway virtualenv. Neither is a dependency.
`tests/fixtures/ndbc` is NDBC's latest reports and station table for the
Bay, as fetched on 2026-09-14.

## License

MIT
