---
name: verify-omawind
description: "Drive omawind the way a sailor does (the CLI, the engine socket, and the Quickshell window when it can run) and keep proof. Use before calling any behavior change done, when reviewing someone else's change, or when asked to verify omawind."
---

# Verify omawind

`scripts/verify.sh` proves the code is sound. This skill proves the app works. It runs the binary from this checkout, drives a feature from the user's side, and keeps evidence. Run both. The agent that built a change does not sign off on this run.

## Launch

Run from the repo root. `at` and `decode` exit on their own. Build once, then start each of those in the environment below. `omawind run` stays up. Give every run its own directory so two drives do not share a socket or a cache.

```sh
mise install
cargo build --locked
run=$(mktemp -d /tmp/omawind-verify.XXXXXX)
mkdir -p "$run/home" "$run/cache" "$run/config" "$run/artifacts/verify"
export HOME="$run/home"
export XDG_CACHE_HOME="$run/cache"
export XDG_CONFIG_HOME="$run/config"
export XDG_RUNTIME_DIR="$run"
dest="$XDG_CACHE_HOME/omawind/hrrr/36.8000_-123.8000_38.8000_-121.6000/2026091403"
mkdir -p "$dest"
cp tests/fixtures/hrrr/2026091403/f00.grib2 \
  tests/fixtures/hrrr/2026091403/f01.grib2 \
  tests/fixtures/hrrr/2026091403/f02.grib2 \
  "$dest/"
./target/debug/omawind run --offline --socket "$run/wind.sock" \
  >"$run/engine.log" 2>&1 &
echo $! >"$run/pid"
for _ in $(seq 50); do
  test -S "$run/wind.sock" && break
  sleep 0.1
done
test -S "$run/wind.sock"
```

Ready when `$run/wind.sock` is a socket and the pid in `$run/pid` is alive. `at` and `decode` do not wait on that socket. They still need `HOME`, `XDG_CACHE_HOME`, and `XDG_CONFIG_HOME` pointing inside `$run`.

`mktemp` prints an absolute path. A relative `XDG_RUNTIME_DIR` makes `omawind run` exit even when `--socket` is set, because the default omakeel socket is still resolved.

## Doctor

Run this before the first drive, and again when anything looks off.

```sh
./target/debug/omawind --version
test -S "$run/wind.sock" && echo "socket exists"
kill -0 "$(cat "$run/pid")" && echo "pid $(cat "$run/pid") alive"
```

The version line is this checkout's binary. The pid is the process this run started. The socket is under `$run`. Connect only to that socket. Do not drive an omawind the user already has running. Do not read `~/.config/omawind` or `~/.cache/omawind`.

Observed on 2026-10-14.

```
omawind 0.1.0
socket exists
pid 4782 alive
```

The pid differs every launch.

## Drive

Follow `features/`. Start at `features/README.md`. A proof that drives one convenient entry point is incomplete when the map lists others.

- A sailor's terminal is `./target/debug/omawind at` and `./target/debug/omawind decode`.
- An app reads `$run/wind.sock`, one JSON object per line, as `docs/protocol.md` describes. The reader is in `features/serve-forecast.md`.
- The window is `./run.sh`, which execs `quickshell -p ui/shell.qml`. When `quickshell` is not on `PATH`, drive `features/window.md` and say the painted window was not driven.

`omawind fetch` and `omawind stations` call NOAA. They are not in this map. Drive them only when the task is that network path, and say that you did. The CLI has no flag that points them at `tests/fixtures/`.

## Evidence

Write under `$run/artifacts/verify/<feature-id>/`. Before cleanup, copy that tree to `${EVIDENCE_DIR:-/tmp/omawind-verify-evidence}/`.

- Save the command, stdout, stderr, and the exit code.
- For the socket, save the lines. The second look is the next line on the same connection.
- The recorded HRRR files stand in for NOMADS. The cache reader that `omawind fetch` fills is the one that loads them. `--offline` must not open a connection to NOMADS. Confirm that with `$run/engine.log`.
- A `grim` screenshot is the window's proof. `./run.sh` stderr and `scripts/verify.sh engine` are what you can capture without Quickshell. Name them as substitutes. Do not call them a screenshot.
- If a path cannot be reached, write the command and the missing precondition. Do not mark that path passed.

## Cleanup

Copy the evidence out first. Then stop only the pid this run wrote.

```sh
evidence=${EVIDENCE_DIR:-/tmp/omawind-verify-evidence}
mkdir -p "$evidence"
cp -a "$run/artifacts/verify/." "$evidence/"
kill "$(cat "$run/pid")" 2>/dev/null || true
for _ in $(seq 50); do
  kill -0 "$(cat "$run/pid")" 2>/dev/null || break
  sleep 0.1
done
rm -rf "$run"
test -d "$evidence"
```

Evidence stays under `$evidence`. `$run` is gone. Do not `pkill` by name.

The `export` lines stay in the shell that ran them. After `$run` is removed, point `HOME` back at the real home before any later command. A stale `HOME` hides `~/.gitconfig` and makes the next `git push` ask for a username.

On the 2026-10-14 run the pid was gone, the socket was gone, and `at-berkeley/stdout.txt` was still under `/tmp/omawind-verify-evidence`.

When omawind's commands or the lines it prints change, run `/maintain-verification-skill` over this directory.

## Helpers

This skill ships no script. The socket reader is the Python block in `features/serve-forecast.md`. The window's engine-path check is the repo's own probe.

```sh
scripts/verify.sh engine
```

The probe substitutes a stub for `omawind run`. It does not fetch from NOAA. When Qt6 is missing it prints `Qt6 QML dev packages not installed. Skipped.` and exits 0. That skip is not a pass of `features/window.md`.
