---
name: verify-omawind
description: "Drive omawind the way a sailor does (the CLI, the engine socket, and the Quickshell window when it can run) and keep proof. Use before calling any behavior change done, when reviewing someone else's change, or when asked to verify omawind."
---

# Verify omawind

`scripts/verify.sh` proves the code is sound. This skill proves the app works. It runs the binary from this checkout, drives a feature from the user's side, and keeps evidence. Run both. The agent that built a change does not sign off on this run.

## Launch

Run the drive in a bash you start for that purpose. Do not source `drive.sh` and do not paste it into the shell you are already using.

```sh
bash --noprofile --norc .cursor/skills/verify-omawind/drive.sh
```

That process is the whole drive. It sets `set -euo pipefail` and `set -m`. On EXIT, INT, and TERM it signals the engine and the window process group with TERM, then KILL, copies the artifacts out, and only then deletes the run directory. The shell that started it is not the shell those options and traps belong to.

With no arguments the drive runs doctor, `at`, decode, serve, and window. Name the features you want:

```sh
bash --noprofile --norc .cursor/skills/verify-omawind/drive.sh doctor at decode serve
bash --noprofile --norc .cursor/skills/verify-omawind/drive.sh window
bash --noprofile --norc .cursor/skills/verify-omawind/drive.sh twice
```

`twice` launches a second engine in the same process. It copies the first run's artifacts into the evidence directory before it deletes that run directory.

The drive prints `evidence: /tmp/omawind-verify-evidence.XXXXXX` as it exits. That directory remains. The run directory does not.

`cargo` on `PATH` may be older than `Cargo.toml` `rust-version`. The drive prepends `$HOME/.local/bin` and runs `mise exec -- cargo build --locked`. `bash --noprofile --norc` still inherits the environment of the shell that started it, including `PATH`.

Do not `export` `HOME`, `XDG_CACHE_HOME`, `XDG_CONFIG_HOME`, or `XDG_RUNTIME_DIR` from the shell that starts the drive. `config.rs` reads `OMAWIND_CONFIG` before those variables, so every omawind command inside the drive uses `env -u OMAWIND_CONFIG` and passes `HOME`, `XDG_CACHE_HOME`, `XDG_CONFIG_HOME`, and `XDG_RUNTIME_DIR` on that command. The engine probe uses `XDG_RUNTIME_DIR=$run/engine-probe-runtime`, not the runtime directory of the shell that started the drive.

`ui/Wind.qml` opens `$XDG_RUNTIME_DIR/omawind/wind.sock`. The offline engine binds `$run/omawind/wind.sock`, and `./run.sh` gets `XDG_RUNTIME_DIR=$run`.

## Doctor

Doctor runs before the other features inside the drive. A missing socket or a dead pid exits non-zero.

The version line is this checkout's binary. The pid is the process this drive started. The socket is `$run/omawind/wind.sock`. The drive does not read `~/.config/omawind` or `~/.cache/omawind`.

Observed on 2026-10-14.

```
omawind 0.1.0
socket exists
pid 4782 alive
```

The pid differs every launch.

## Drive

Follow `features/`. Start at `features/README.md`.

- A sailor's terminal is `./target/debug/omawind at` and `./target/debug/omawind decode`.
- An app reads the socket, one JSON object per line, as `docs/protocol.md` describes. The server pushes three lines on connect. The reader is in `features/serve-forecast.md`.
- The window is `./run.sh`, which execs `quickshell -p ui/shell.qml`. The drive always writes `window-painted`. A `miss.txt` there or under `window-engine` makes the window feature exit non-zero.

`omawind fetch` and `omawind stations` call NOAA. They are not in this map. Drive them only when the task is that network path, and say that you did.

## Evidence

The evidence directory is created before the engine starts. The EXIT trap copies `$run/artifacts` there before it deletes `$run`. A second launch in the same process does the same copy before it deletes the first directory.

- `record` saves the command, stdout, stderr, and the exit code under `verify/<id>/`. An empty stdout or stderr file is removed.
- For the socket, the three lines the server pushes on connect are in `verify/serve-forecast/`. The reader sends no request.
- The recorded HRRR files stand in for NOMADS. `--offline` must not open a connection to NOMADS. The proof is the state line `"fetch":{"status":"off"}` and the stations line `"status":"off"`.
- `window-painted` is always written. On a Hyprland session with Quickshell, it is the `grim` image. When `quickshell` or `grim` is missing, the directory names that command and `miss.txt` records the miss. Do not call that file a screenshot. `window-engine` passes only when stdout or stderr contains `All engine-path checks passed.` and neither stream contains `Qt6 QML dev packages not installed. Skipped.` The drive searches both streams. `scripts/verify.sh` still exits 0 on that skip.
- The engine transcript is produced with `env -u OMAWIND_CONFIG` and `XDG_RUNTIME_DIR` set to `$run/engine-probe-runtime`.

## Cleanup

The trap is the cleanup. It runs on a normal exit, on Ctrl-C, and on TERM. It signals each process group the drive started (`kill -- -pgid`), waits, then sends KILL. It copies the artifacts, then deletes the run directory. It does not `pkill` by name.

When omawind's commands or the lines it prints change, run `/maintain-verification-skill` over this directory.
