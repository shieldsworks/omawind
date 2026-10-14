# Serve the forecast

`omawind run --offline` serves the cached forecast on a Unix socket. The first line is `hello`. The next line is `state`. The third line is `stations`. Fetch stays off, and NDBC is not asked.

## Sub-features

- `hello` is one JSON object with `"type":"hello"` and `"v":1`.
- `state-offline` reports the recorded run and `"fetch":{"status":"off"}`.
- `stations-offline` reports `"status":"off"` and an empty station list.
- `no-network` leaves the engine log with no NOMADS or NDBC request.

## How to get to it (user POV)

- Run `omawind run --offline` and point a client at the socket. The bar does this when the widget loads, except the widget starts `omawind run` with no `--offline`.
- `./run.sh` opens the window, which starts the engine. That path is `window.md`.

## Driving it with the socket

Preconditions:

- Launch from `../SKILL.md`, including `--offline` and `--socket "$run/wind.sock"`.
- Doctor passes.

- **Connect.** The user connects. Run the reader below. It prints three lines and exits 0.
- **Hello.** The first line is `{"type":"hello","v":1,"wind":"0.1.0"}`. Observed on 2026-10-14. That was the whole first line.
- **State.** The second line is one JSON object with `"type":"state"`, `"fetch":{"status":"off"}`, `"keel":"lost"`, and `forecast.run` equal to `2026-09-14T03:00:00Z` with `forecast.hours` equal to 3. `"fetch":{"status":"off"}` is how you see that this run did not ask NOAA for a forecast. Observed on 2026-10-14. The line also had `"status":"expired"` and `"note":"no forecast for now: the newest one has run out"`. `ageMinutes` and `here.time` were present and were not compared.
- **Stations.** The third line is one JSON object with `"type":"stations"`, `"source":"NDBC"`, `"status":"off"`, and `"stations":[]`. Observed on 2026-10-14. The line was `{"source":"NDBC","stations":[],"status":"off","type":"stations","v":1}`.
- **No network.** `$run/engine.log` does not contain `nomads.ncep.noaa.gov` or `ndbc.noaa.gov`. Observed on 2026-10-14. The log file was empty.
- **Proof.** Save the three lines and the log to `$run/artifacts/verify/serve-forecast/`.

```sh
python3 - "$run/wind.sock" <<'EOF'
import socket, sys
s = socket.socket(socket.AF_UNIX)
s.settimeout(5)
s.connect(sys.argv[1])
f = s.makefile()
for _ in range(3):
    line = f.readline()
    if not line:
        raise SystemExit(1)
    sys.stdout.write(line)
EOF
```

## Gotchas

- The recorded run ends at `2026-09-14T05:00:00Z`. On a later wall clock, `forecast.status` is `expired` and `here.note` says the newest forecast has run out. That is the real clock, not a failed load. `at` still prints the three hours.
- `state` includes `ageMinutes` and `here.time`, which move. Do not pin those two.
- `keel` is `lost` because this run's `XDG_RUNTIME_DIR` has no omakeel socket. That keeps the drive off the user's keel. Do not read `lost` as a failed forecast load.
- Pass `--socket`. The default path is `$XDG_RUNTIME_DIR/omawind/wind.sock`, not `$run/wind.sock`, unless you create that directory and omit the flag.
- The window path needs Hyprland and Quickshell. Report it skipped when those are absent. The substitute is `window.md`, and it is not this socket transcript.
