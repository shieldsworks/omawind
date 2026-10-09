---
name: verify-omawind
description: "Drive omawind the way a user does (the CLI and the engine socket) and capture proof. Use before calling any behavior change done, when reviewing someone else's change, or when asked to verify omawind."
---

# Verify omawind

`scripts/verify.sh` proves the code is sound (lint, tests, goldens). This
skill proves the app works. It launches the real binary, drives a feature
from the user's side, and keeps evidence. Run both. The agent that built a
change should not be the one that signs off on this run.

## Launch

Short commands (`at`, `decode`) need no server. Build once, then run each
drive as its own process.

```sh
mise install && cargo build --locked
run=$(mktemp -d /tmp/omawind-verify.XXXXXX)
export XDG_CACHE_HOME="$run/cache"
export XDG_CONFIG_HOME="$run/config"
export XDG_RUNTIME_DIR="$run"
mkdir -p "$XDG_CACHE_HOME" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"
dest="$XDG_CACHE_HOME/omawind/hrrr/36.8000_-123.8000_38.8000_-121.6000/2026091403"
mkdir -p "$dest"
cp tests/fixtures/hrrr/2026091403/f0{0,1,2}.grib2 "$dest/"
./target/debug/omawind run --offline --socket "$run/wind.sock" \
  >"$run/engine.log" 2>&1 & echo $! > "$run/pid"
```

Ready when `$run/wind.sock` exists and a client can read a `hello` line.
`at` and `decode` do not wait for that socket.

## Doctor

One read-only check before driving, and whenever anything looks off:

```sh
./target/debug/omawind --version
test -S "$run/wind.sock"
kill -0 "$(cat "$run/pid")"
```

It must show this checkout's build (`omawind 0.1.0` today), a live process
this run started, and a socket inside `$run`. Never drive a copy the user
already has running, and never read their real `~/.config/omawind` or
`~/.cache/omawind`. The env vars above keep this run in `$run`.

## Drive

Follow the feature map in `features/`. A proof that drives one convenient
entry point is incomplete when the map lists others.

- CLI subcommands with deterministic output (`omawind at`, `omawind decode`).
- The engine socket, speaking `docs/protocol.md` (one JSON object per line).
- The Quickshell window via `./run.sh` on a Hyprland session, with `grim`
  for screenshots. Only on Casey's machine. CI and a headless box can't run it.

`omawind stations` and `omawind fetch` talk to NOAA. Don't call them from
this skill unless the task is the network path, and then say that you did.

## Evidence

Put everything in `$run/artifacts/verify/<feature-id>/`, then copy it
somewhere that survives cleanup (`/tmp/omawind-verify-evidence` if you have
nowhere else).

- Capture the action and the resulting state. The command or socket
  request, stdout, stderr, exit code, and the reply or file it produced.
- Check side effects with a second, read-only look (the file on disk, the
  next socket message), not just the first reply.
- Exercise the real user path. No test-only endpoints, no internal setters.
  Recorded NOAA files are the stand-in for the network. The production
  cache reader already loads them.
- `--offline` must not open a connection to NOMADS. Confirm that with the
  engine log in `$run/engine.log`.
- Report anything you could not reach, with the command tried and the
  missing precondition. A skipped entry point is never reported as passed.

## Cleanup

```sh
kill "$(cat "$run/pid")" 2>/dev/null || true
rm -rf "$run"
```

Evidence stays. Check it still exists after cleanup. Kill only the pid
this run wrote. Never `pkill` by name.
