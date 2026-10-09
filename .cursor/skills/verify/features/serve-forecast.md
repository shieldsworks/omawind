# Serve the forecast

`omawind run --offline` serves the cached forecast on a Unix socket. The
first line is `hello`. The next line is `state`. Fetch stays off.

## Sub-features

- `hello` is one JSON object with `"type":"hello"` and `"v":1`.
- `state-offline` reports the recorded run and `"fetch":{"status":"off"}`.
- `no-network` leaves the engine log with no NOMADS request.

## How to get to it (user POV)

- Run `omawind run --offline` and point a client at the socket. The bar does this when the widget loads.
- `./run.sh` opens the window, which starts the engine. Casey's machine only.

## Driving it with the socket

Preconditions:

- Launch from `../SKILL.md`, including `--offline` and `--socket "$run/wind.sock"`.
- Doctor passes.

- **Hello.** The user connects. Read the first line from `$run/wind.sock`. It is `{"type":"hello","v":1,"wind":"0.1.0"}`.
- **State.** Read the next line. It is one JSON object with `"type":"state"`, `"fetch":{"status":"off"}`, and `forecast.run` equal to `2026-09-14T03:00:00Z` with `forecast.hours` equal to 3. `"fetch":{"status":"off"}` is how you see that this run did not ask NOAA.
- **Proof.** Save both lines to `$run/artifacts/verify/serve-forecast/`.

## Gotchas

- The recorded run ends at `2026-09-14T05:00:00Z`. On a later wall clock, `forecast.status` is `expired` and `here.note` says the run has run out. That is the real clock, not a failed load. `at` still prints the three hours.
- `state` includes `ageMinutes` and `here.time`, which move. Don't pin those two.
- Pass `--socket`. The default path is `$XDG_RUNTIME_DIR/omawind/wind.sock`, not `$run/wind.sock`, unless you create that directory.
- The window path needs Hyprland and Quickshell. Report it skipped when those are absent.
