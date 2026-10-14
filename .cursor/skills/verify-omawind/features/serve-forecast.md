# Serve the forecast

`omawind run --offline` serves the cached forecast on a Unix socket. On connect the server pushes three lines. The first line is `hello`. The next line is `state`. The third line is `stations`. The reader sends no request. Fetch stays off, and NDBC is not asked.

## Sub-features

- `hello` is one JSON object with `"type":"hello"` and `"v":1`.
- `state-offline` reports the recorded run and `"fetch":{"status":"off"}`.
- `stations-offline` reports `"status":"off"` and an empty station list.
- `no-network` is those two status values. Fetch is `off` and stations are `off`.

## How to get to it (user POV)

- Run `omawind run --offline` and point a client at the socket. The bar does this when the widget loads, except the widget starts `omawind run` with no `--offline`.
- `./run.sh` opens the window, which starts the engine. That path is `window.md`.

## Driving it with the socket

Preconditions:

- Run it with the drive in `../SKILL.md`. The engine is `omawind run --offline` on `$run/omawind/wind.sock`.
- Doctor passes inside that same drive.

- **Connect.** The user connects. The drive's reader prints the three lines the server pushes. The reader sends no request. It exits 0.
- **Hello.** The first line is `{"type":"hello","v":1,"wind":"0.1.0"}`. Observed on 2026-10-14. That was the whole first line.
- **State.** The second line is one JSON object with `"type":"state"`, `"fetch":{"status":"off"}`, `"keel":"lost"`, and `forecast.run` equal to `2026-09-14T03:00:00Z` with `forecast.hours` equal to 3. `"fetch":{"status":"off"}` is how you see that this run did not ask NOAA for a forecast. Observed on 2026-10-14. The line also had `"status":"expired"` and `"note":"no forecast for now: the newest one has run out"`. `ageMinutes` and `here.time` were present and were not compared.
- **Stations.** The third line is one JSON object with `"type":"stations"`, `"source":"NDBC"`, `"status":"off"`, and `"stations":[]`. Observed on 2026-10-14. The line was `{"source":"NDBC","stations":[],"status":"off","type":"stations","v":1}`. `"status":"off"` is how you see that this run did not ask NDBC.
- **No network.** The no-network proof is `"fetch":{"status":"off"}` on the state line and `"status":"off"` on the stations line. Do not use an empty `$run/engine.log`. The log can be empty whether or not a request went out, because the engine does not write the NOMADS or NDBC URL there. Observed on 2026-10-14. Both status values were `off`. The log file was empty, and that emptiness was not the proof.
- **Proof.** Save the three lines to `$run/artifacts/verify/serve-forecast/`.

```sh
bash --noprofile --norc .cursor/skills/verify-omawind/drive.sh serve
```

## Gotchas

- The recorded run ends at `2026-09-14T05:00:00Z`. On a later wall clock, `forecast.status` is `expired` and `here.note` says the newest forecast has run out. That is the real clock, not a failed load. `at` still prints the three hours.
- `state` includes `ageMinutes` and `here.time`, which move. Do not pin those two.
- `keel` is `lost` because this run's `XDG_RUNTIME_DIR` has no omakeel socket. That keeps the drive off the user's keel. Do not read `lost` as a failed forecast load.
- Pass `--socket "$sock"`. The default path is `$XDG_RUNTIME_DIR/omawind/wind.sock`. With `XDG_RUNTIME_DIR` set to `$run` on the command, that default is `$run/omawind/wind.sock`. `$run/wind.sock` is a different path. The window opens the default path, and a miss there starts `omawind run` with no `--offline`.
- The window path needs Hyprland and Quickshell. Report it skipped when those are absent. The substitute is `window.md`, and it is not this socket transcript.
