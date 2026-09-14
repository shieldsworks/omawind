# omawind protocol

Version 1. The wind engine (`omawind run`) is the server. The omawind bar
widget and omahelm's wind layer are clients.

## Transport

- A Unix stream socket, `$XDG_RUNTIME_DIR/omawind/wind.sock` by default
  (`--socket` sets another). The directory is created with mode 0700.
- A lock file beside the socket (`wind.sock.lock`) keeps a second engine
  from starting on the same socket. A socket left behind by a crashed engine
  is replaced.
- Newline-delimited JSON, UTF-8, one object per line.
- Every engine message has `"type"` and `"v": 1`. A client that sees another
  `v` shows an error and stops using the data.
- Key order is not significant. Keys that don't apply are left out, never
  sent as `null`. Clients ignore message types and keys they don't know.
- Any number of clients may connect. `state` and `stations` go to every
  client; `field` and `error` go only to the client that asked.
- A client that can't take a message within 2 seconds, or falls 64 messages
  behind, is disconnected. It can reconnect.
- A line longer than 64 KiB from a client, or one that isn't a JSON object,
  is answered with an `error` and otherwise ignored.

## Engine messages

`hello` is sent once on connect, followed by `state` and `stations`.

```json
{"type":"hello","v":1,"wind":"0.1.0"}
```

`state` is the complete current state, re-sent whenever it changes, which is
at least once a minute while there's a forecast. Clients replace their copy.

```json
{"type":"state","v":1,
 "forecast":{"status":"ok","model":"HRRR","run":"2026-09-14T03:00:00Z","ageMinutes":95,
             "first":"2026-09-14T03:00:00Z","last":"2026-09-15T03:00:00Z","hours":25,
             "region":{"south":36.8,"west":-123.8,"north":38.8,"east":-121.6}},
 "fetch":{"status":"idle","checked":"2026-09-14T04:35:00Z"},
 "keel":"connected",
 "here":{"at":"boat","lat":37.86471,"lon":-122.32073,"time":"2026-09-14T04:35:00Z",
         "speedKn":12.4,"dirDeg":262,"gustKn":17.9,"pressureHpa":1014.6},
 "outlook":[{"time":"2026-09-14T04:00:00Z","speedKn":11.8,"dirDeg":259,"gustKn":17.1,"pressureHpa":1014.7},
            {"time":"2026-09-14T05:00:00Z","speedKn":13.0,"dirDeg":264,"gustKn":18.6,"pressureHpa":1014.5}],
 "problems":["config.toml: unknown setting bogus"]}
```

### `forecast`

- `status` is one of:
  - `none`: nothing cached for the region yet.
  - `ok`: a forecast that covers now.
  - `old`: it covers now, but its run is 4 hours old or more, so newer runs
    couldn't be fetched. HRRR runs every hour and a run is usually about
    1½ hours old when it's used.
  - `expired`: now is past its last hour. The rest of `forecast` is still
    sent, so a client can say which run it was.
- `model` is `HRRR`, NOAA's High-Resolution Rapid Refresh: a 3 km grid over
  the lower 48 and its coastal waters, run hourly. `run` is the run's time,
  `first` and `last` its first and last forecast hours, and `hours` how many
  there are. The 00, 06, 12 and 18 UTC runs reach 48 hours; the others 18.
- `ageMinutes` is minutes since the run's time.
- `region` is the area forecast, from the settings, in degrees.

### `fetch`

- `status` is one of `off` (started with `--offline`), `checking`,
  `downloading` (with `done` and `total` hours), `idle` and `error`.
- `message` says what went wrong, with `error`.
- `checked` is when NOMADS was last checked successfully.
- The engine checks every 10 minutes, and at once when the region changes.
  A run is used once its first 18 hours are out; the 00, 06, 12 and 18 UTC
  runs' later hours are fetched as they appear. Until a new run is complete
  the one before it stays in use.

### `keel`

omakeel's connection: `off` (never asked), `lost` (not connected),
`connected`, or `incompatible` (it speaks another protocol version).

### `here`

The wind where the boat is: at omakeel's position when it has one, else at
home from the settings.

- `at` is `boat` or `home`. `stale` is true when omakeel's fix is stale and
  this is where the boat was last seen.
- `lat`, `lon`: degrees, west negative. `time` is now, to the minute.
- `speedKn` is the wind 10 m above the water, knots. `dirDeg` is where it
  blows from, degrees true, 0 to 359. `gustKn` is the surface gust, knots.
  `pressureHpa` is the pressure at sea level, hectopascals.
- Between hours the forecast is linear in time; between grid points it's
  bilinear. Wind is interpolated as east and north components.
- Without a forecast for the place and time, the numbers are left out and
  `note` says why.

### `outlook`

The forecast at `here`, one entry per hour, from the hour under way to the
run's last. Empty without a forecast for the place.

`stations` is the wind measured at weather stations in the region: NOAA's
buoys, and the piers and tide gauges of its PORTS program around harbors. It
is sent on connect after `state`, and again whenever it changes. Clients
replace their copy.

```json
{"type":"stations","v":1,"source":"NDBC","status":"ok","checked":"2026-09-14T17:50:00Z",
 "stations":[{"id":"AAMC1","name":"Alameda","lat":37.772,"lon":-122.3,
              "time":"2026-09-14T17:00:00Z","speedKn":2.9,"dirDeg":120,"gustKn":4.1}]}
```

- `source` is `NDBC`, NOAA's National Data Buoy Center. The engine reads its
  latest reports every 10 minutes, about as often as NDBC updates them.
- `status` is `off` (started with `--offline`), `waiting` (no answer yet),
  `ok`, or `error` with `message`. After an error the last reports stay
  until they're too old to show, and so does the last report of a station
  a fetch left out, so a file cut short can't empty the region.
- `checked` is when NDBC last answered.
- Each station: `id` is NDBC's, and `name` is sent when NDBC's table has
  one. `time` is when the report was taken; a report over 2 hours old is left
  out. `speedKn` is the wind averaged over 2 minutes ashore or 8 on a buoy,
  knots, and `dirDeg` where it blows from, degrees true, 0 to 359, left out
  only in a calm. `gustKn` is the gust, when reported. Stations that report no
  wind, or a speed without a direction, aren't listed.
- A station's anemometer may not be 10 m up, and a pier's may be sheltered:
  its wind is what it measured, not what `field` would say there.

## Requests

`field` asks for the wind across an area, at the model's own points.

```json
{"type":"field","id":7,"south":37.6,"west":-122.6,"north":37.9,"east":-122.2,
 "time":"2026-09-14T05:00:00Z","max":400}
```

- `south` < `north` and `west` < `east`, degrees.
- `time` is optional: UTC, `YYYY-MM-DDTHH:MM:SSZ` or without seconds.
  Without it the answer is for now. It must fall within the forecast.
- `max`, 1 to 2000, 400 without it, is the most points to send. Every
  `step`th grid point is sent in each direction, counted from the grid's
  first point, so points stay put as the area moves.
- `id` is any JSON value, and is copied to the answer.

```json
{"type":"field","v":1,"id":7,"time":"2026-09-14T05:00:00Z","run":"2026-09-14T03:00:00Z","step":2,
 "points":[{"lat":37.6012,"lon":-122.5981,"speedKn":14.2,"dirDeg":281,"gustKn":19.9,"pressureHpa":1014.4}]}
```

`point` asks for the forecast at one position, worked out as `here` is:
bilinear between grid points, linear between hours.

```json
{"type":"point","id":"p1","lat":37.8123,"lon":-122.4012,"time":"2026-09-14T05:00:00Z"}
```

- `lat` and `lon` are degrees, west negative. `time` and `id` are as for
  `field`; the time must fall within the forecast.
- The answer has `speedKn`, `dirDeg`, `gustKn` and `pressureHpa` as `here`
  does. Off the forecast's grid they're left out and `note` says why.

```json
{"type":"point","v":1,"id":"p1","lat":37.8123,"lon":-122.4012,"time":"2026-09-14T05:00:00Z",
 "run":"2026-09-14T03:00:00Z","speedKn":13.1,"dirDeg":262,"gustKn":18.4,"pressureHpa":1014.5}
```

`error` answers a bad request, with the request's `id` when it had one.

```json
{"type":"error","v":1,"id":7,"message":"field: no forecast yet"}
```

## Settings

`$XDG_CONFIG_HOME/omawind/config.toml` (or `$OMAWIND_CONFIG`), re-read when
it changes:

```toml
# South, west, north, east, degrees. At most 8° by 10°, inside HRRR.
region = 36.8, -123.8, 38.8, -121.6
# Where the wind is reported without a GPS: latitude, longitude.
home = 37.8663, -122.3148
```

Both default to the values above: San Francisco Bay, and the Berkeley
Marina. Problems are reported in `state.problems` and the defaults kept.

## Files

- Runs: `$XDG_CACHE_HOME/omawind/hrrr/<south>_<west>_<north>_<east>/<YYYYMMDDHH>/f00.grib2`
  and on: one GRIB2 file per hour, as NOAA's NOMADS filter cut them, in a
  folder for the region. The newest run with its first 18 hours good is
  used, else the newest with any. An hour that doesn't check out ends the
  forecast there and is fetched again. Runs older than the one in use are
  deleted.
