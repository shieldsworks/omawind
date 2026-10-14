# The wind window

`./run.sh` opens the wind in a window of its own. The window starts `omawind run` when the socket is down, from `OMAWIND_BIN` or this checkout's release binary, and if that engine never answers it shows the last log line instead of a blank window.

## Sub-features

- `window-launch` `./run.sh` opens the window under Quickshell.
- `window-engine` the window's start script runs the binary next to the QML file, and `scripts/verify.sh engine` checks that path with a stub.
- `window-painted` a Hyprland session can capture the window with `grim`.

## How to get to it (user POV)

- Run `./run.sh` from the checkout. It execs `quickshell -p ui/shell.qml`. It does not set `OMAWIND_BIN`.
- The Omarchy bar loads `ui/WindBar.qml`. The same `ui/Wind.qml` starts the engine.
- With the window focused, `j` and `k` scroll, `n` toggles Night Watch, and `q` closes.

## Driving it with the start script

Preconditions:

- The baseline from `../SKILL.md` is not required for `./run.sh`. The script does not read the cache.
- Quickshell and Hyprland are required for the painted window. When they are absent, `window-launch` and `window-painted` are unreachable. Say so. Do not call another transcript a screenshot.
- Do not exec the start script in `ui/Wind.qml` against the real binary. That script runs `omawind run` with no `--offline`, so it fetches from NOMADS. `scripts/verify.sh engine` is the check that substitutes a stub.

- **Open the window.** Run `./run.sh`. When `quickshell` is missing, the script exits 127 and stderr names `quickshell`. The painted window was not driven.
- **Engine path.** Run `scripts/verify.sh engine`. A pass prints `All engine-path checks passed.` When Qt6 is missing it prints `Qt6 QML dev packages not installed. Skipped.` and exits 0. Record that line. The skip is not a pass of this feature.
- **Painted window.** On a Hyprland session, capture the window with `grim`. Save the image under `$run/artifacts/verify/window/`. When Hyprland or Quickshell is absent, record the missing command and leave this step unreached.
- **Proof.** Save the `./run.sh` stderr and the `scripts/verify.sh engine` transcript under `$run/artifacts/verify/window/`. Do not label them as a window screenshot.

## Gotchas

- `./run.sh` fails before it looks for a binary. A missing release build is not what exit 127 means when stderr says `quickshell` is missing.
- `scripts/verify.sh engine` exits 0 on the Qt6 skip. That exit code does not mean the engine path was checked.
- The start script in `ui/Wind.qml` is `mkdir -p -m 700 "$1" && b="$2"; [ -x "$b" ] || b=omawind; exec "$b" run 2>>"$3"`. The real binary then fetches. The offline engine in `../SKILL.md` is a different command. Do not treat its socket transcript as this window.
- `quickshell ipc` can call the window's `status` handler only while Quickshell is running. It is not a substitute for `./run.sh` on a machine where `quickshell` is absent.
