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

- The offline engine from `../SKILL.md` is listening on `$sock`, which is `$run/omawind/wind.sock`. `ui/Wind.qml` opens `$XDG_RUNTIME_DIR/omawind/wind.sock`. With `XDG_RUNTIME_DIR` set to `$run` on `./run.sh`, that is the same socket.
- Quickshell and Hyprland are required for the painted window. When they are absent, `window-launch` and `window-painted` are unreachable. Say so. Do not call another transcript a screenshot.
- `scripts/verify.sh engine` substitutes a stub for the real binary. Do not let `ui/Wind.qml` start `omawind run` with no `--offline`. That start fetches from NOMADS.

- **Open the window.** Pin `WAYLAND_DISPLAY` to an absolute path first, when it is set, so Quickshell still reaches the compositor after `XDG_RUNTIME_DIR` moves to `$run`. A relative `WAYLAND_DISPLAY` is resolved against the shell's current `XDG_RUNTIME_DIR`, not against `$run`. Run `env -u OMAWIND_CONFIG HOME="$run/home" XDG_CACHE_HOME="$run/cache" XDG_CONFIG_HOME="$run/config" XDG_RUNTIME_DIR="$run" OMAWIND_BIN="$PWD/target/debug/omawind" WAYLAND_DISPLAY="$wayland" ./run.sh`, omitting `WAYLAND_DISPLAY` when the shell has none. `OMAWIND_BIN` is absolute because `ui/Wind.qml` starts the engine with cwd set to `HOME`. A relative `target/debug/omawind` is not executable from `$run/home`, and the start script then runs `omawind` from `PATH` with no `--offline`. When `quickshell` is missing, the script exits 127 and stderr names `quickshell`. The painted window was not driven. Observed on 2026-10-14. Exit 127, stdout empty, and stderr `./run.sh: line 7: exec: quickshell: not found`.
- **Engine path.** Run `scripts/verify.sh engine`. A pass prints `All engine-path checks passed.` When Qt6 is missing it prints `Qt6 QML dev packages not installed. Skipped.` and exits 0. Record that line. The skip is not a pass of this feature. `scripts/verify.sh` lines 68 to 70 `mkdir` `XDG_RUNTIME_DIR`, or `/tmp/runtime-omawind-verify` when the variable is unset. Observed on 2026-10-14. Exit 0. The transcript was `Qt6 QML dev packages not installed. Skipped.` and then `All verification steps passed.` The second line is the script's own summary after that skip.
- **Painted window.** On a Hyprland session, capture the window with `grim`. Save the image under `$run/artifacts/verify/window-painted/`. When Hyprland or Quickshell is absent, record the missing command and leave this step unreached. Observed on 2026-10-14. `quickshell` and `grim` were not on `PATH`. This step was not driven.
- **Proof.** Save the `./run.sh` stderr and the `scripts/verify.sh engine` transcript under `$run/artifacts/verify/window-launch/` and `$run/artifacts/verify/window-engine/`. Do not label them as a window screenshot.

```sh
set -euo pipefail
[[ -S ${sock:-} && -d $run ]]
wayland=${WAYLAND_DISPLAY-}
if [[ -n $wayland && $wayland != /* ]]; then
  runtime=${XDG_RUNTIME_DIR-}
  if [[ -z $runtime || $runtime != /* ]]; then
    printf 'WAYLAND_DISPLAY is relative and XDG_RUNTIME_DIR is not absolute\n' >&2
    exit 1
  fi
  wayland=$runtime/$wayland
fi
cmd=(env -u OMAWIND_CONFIG
  HOME="$run/home"
  XDG_CACHE_HOME="$run/cache"
  XDG_CONFIG_HOME="$run/config"
  XDG_RUNTIME_DIR="$run"
  OMAWIND_BIN="$PWD/target/debug/omawind")
if [[ -n $wayland ]]; then
  cmd+=(WAYLAND_DISPLAY="$wayland")
fi
cmd+=(./run.sh)
record window-launch "${cmd[@]}"
code=$(cat "$run/artifacts/verify/window-launch/exit-code.txt")
if ! command -v quickshell >/dev/null 2>&1; then
  [[ $code -eq 127 ]]
  grep -F 'quickshell' "$run/artifacts/verify/window-launch/stderr.txt" >/dev/null
fi
record window-engine scripts/verify.sh engine
[[ $(cat "$run/artifacts/verify/window-engine/exit-code.txt") -eq 0 ]]
[[ -s $run/artifacts/verify/window-engine/stdout.txt ]]
```

## Gotchas

- `./run.sh` fails before it looks for a binary when `quickshell` is missing. A missing release build is not what exit 127 means when stderr says `quickshell` is missing.
- `scripts/verify.sh engine` exits 0 on the Qt6 skip. That exit code does not mean the engine path was checked.
- The start script in `ui/Wind.qml` is `mkdir -p -m 700 "$1" && b="$2"; [ -x "$b" ] || b=omawind; exec "$b" run 2>>"$3"`. It runs with cwd set to `HOME`, and it does not pass `--offline`. The offline engine has to already be listening on `$run/omawind/wind.sock`, or this start fetches from NOMADS and cleanup's pid check fails.
- `quickshell ipc` can call the window's `status` handler only while Quickshell is running. It is not a substitute for `./run.sh` on a machine where `quickshell` is absent.
