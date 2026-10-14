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
- Quickshell and Hyprland are required for a painted window. When `quickshell` is absent, `./run.sh` exits 127 and `window-painted` names the missing command. That file is not a screenshot.
- `scripts/verify.sh engine` substitutes a stub for the real binary. Do not let `ui/Wind.qml` start `omawind run` with no `--offline`. That start fetches from NOMADS.

- **Open the window.** Pin `WAYLAND_DISPLAY` to an absolute path first, when it is set, so Quickshell still reaches the compositor after `XDG_RUNTIME_DIR` moves to `$run`. A relative `WAYLAND_DISPLAY` is resolved against the shell's current `XDG_RUNTIME_DIR`, not against `$run`. Run `env -u OMAWIND_CONFIG HOME="$run/home" XDG_CACHE_HOME="$run/cache" XDG_CONFIG_HOME="$run/config" XDG_RUNTIME_DIR="$run" OMAWIND_BIN="$PWD/target/debug/omawind" WAYLAND_DISPLAY="$wayland" ./run.sh`, omitting `WAYLAND_DISPLAY` when the shell has none. `OMAWIND_BIN` is absolute because `ui/Wind.qml` starts the engine with cwd set to `HOME`. A relative `target/debug/omawind` is not executable from `$run/home`, and the start script then runs `omawind` from `PATH` with no `--offline`. When `quickshell` is missing, the script exits 127 and stderr names `quickshell`. Observed on 2026-10-14. Exit 127, stdout empty, and stderr `./run.sh: line 7: exec: quickshell: not found`.
- **Engine path.** Run `scripts/verify.sh engine`. A pass prints `All engine-path checks passed.` When Qt6 is missing it prints `Qt6 QML dev packages not installed. Skipped.` and exits 0. That exit code is not a pass. The fence writes the skip line to `window-engine/miss.txt`. `scripts/verify.sh` lines 68 to 70 `mkdir` `XDG_RUNTIME_DIR`, or `/tmp/runtime-omawind-verify` when the variable is unset. Observed on 2026-10-14. Exit 0. The transcript was `Qt6 QML dev packages not installed. Skipped.` and then `All verification steps passed.` The second line is the script's own summary after that skip. It is not `All engine-path checks passed.`
- **Painted window.** The fence always writes `$run/artifacts/verify/window-painted/`. When `quickshell` is missing, `command.txt` names `quickshell` and `miss.txt` says `missing command: quickshell`. When `quickshell` is present and `grim` is missing, the same files name `grim`. When both are present, the fence starts `./run.sh` in the background with stdin from `/dev/null`, checks that the pid is alive, runs `timeout 10 grim -`, and then stops the process. A non-zero `grim` exit or an empty image fails the fence. The `timeout` is there so a compositor that never answers cannot hold the shell. Do not call a missing-command file a screenshot.
- **Proof.** Save the `./run.sh` stderr and the `scripts/verify.sh engine` transcript under `$run/artifacts/verify/window-launch/` and `$run/artifacts/verify/window-engine/`. Do not label them as a window screenshot.

```sh
omawind_verify_window() {
  local opts wayland runtime code dir launch_pid alive painted_ok _
  local -a cmd
  opts=$(set +o)
  trap 'eval "$opts"; trap - RETURN' RETURN
  set +e
  set -o pipefail
  [[ -S ${sock:-} && -d ${run:-} ]] || return 1
  wayland=${WAYLAND_DISPLAY-}
  if [[ -n $wayland && $wayland != /* ]]; then
    runtime=${XDG_RUNTIME_DIR-}
    if [[ -z $runtime || $runtime != /* ]]; then
      printf 'WAYLAND_DISPLAY is relative and XDG_RUNTIME_DIR is not absolute\n' >&2
      return 1
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
  omawind_verify_window_miss() {
    local id=$1 name=$2 dir
    dir=$run/artifacts/verify/$id
    mkdir -p "$dir" || return 1
    printf '%s\n' "$name" >"$dir/command.txt" || return 1
    printf 'missing command: %s\n' "$name" >"$dir/stdout.txt" || return 1
    printf '%s\n' '0' >"$dir/exit-code.txt" || return 1
    printf 'missing command: %s\n' "$name" >"$dir/miss.txt" || return 1
  }
  omawind_verify_window_stop() {
    local pid=$1 _
    kill -- "-$pid" 2>/dev/null || kill "$pid" 2>/dev/null || true
    for _ in $(seq 20); do
      kill -0 "$pid" 2>/dev/null || return 0
      sleep 0.1
    done
    kill -9 -- "-$pid" 2>/dev/null || kill -9 "$pid" 2>/dev/null || true
    for _ in $(seq 20); do
      kill -0 "$pid" 2>/dev/null || return 0
      sleep 0.1
    done
    return 1
  }
  if ! command -v quickshell >/dev/null 2>&1; then
    record window-launch "${cmd[@]}" || return 1
    code=$(cat "$run/artifacts/verify/window-launch/exit-code.txt") || return 1
    [[ $code -eq 127 ]] || return 1
    grep -F 'quickshell' "$run/artifacts/verify/window-launch/stderr.txt" >/dev/null || return 1
    omawind_verify_window_miss window-painted quickshell || return 1
  else
    dir=$run/artifacts/verify/window-launch
    mkdir -p "$dir" || return 1
    printf '%q ' "${cmd[@]}" >"$dir/command.txt" || return 1
    printf '\n' >>"$dir/command.txt" || return 1
    setsid "${cmd[@]}" >"$dir/stdout.txt" 2>"$dir/stderr.txt" </dev/null &
    launch_pid=$!
    disown "$launch_pid" 2>/dev/null || true
    alive=0
    for _ in $(seq 20); do
      if kill -0 "$launch_pid" 2>/dev/null; then
        alive=1
        break
      fi
      sleep 0.1
    done
    if [[ $alive -ne 1 ]]; then
      wait "$launch_pid" 2>/dev/null
      code=$?
      printf '%s\n' "$code" >"$dir/exit-code.txt"
      return 1
    fi
    painted_ok=0
    if ! command -v grim >/dev/null 2>&1; then
      omawind_verify_window_miss window-painted grim || return 1
      painted_ok=1
    else
      record window-painted timeout 10 grim - || return 1
      code=$(cat "$run/artifacts/verify/window-painted/exit-code.txt") || return 1
      if [[ $code -eq 0 && -s $run/artifacts/verify/window-painted/stdout.txt ]]; then
        painted_ok=1
      elif [[ ! -s $run/artifacts/verify/window-painted/stdout.txt \
        && ! -s $run/artifacts/verify/window-painted/stderr.txt ]]; then
        printf 'exit %s\n' "$code" >"$run/artifacts/verify/window-painted/stderr.txt" || return 1
      fi
    fi
    omawind_verify_window_stop "$launch_pid" || return 1
    wait "$launch_pid" 2>/dev/null
    code=$?
    printf '%s\n' "$code" >"$dir/exit-code.txt" || return 1
    [[ -s $dir/stdout.txt ]] || rm -f "$dir/stdout.txt"
    [[ -s $dir/stderr.txt ]] || rm -f "$dir/stderr.txt"
    if [[ ! -e $dir/stdout.txt && ! -e $dir/stderr.txt ]]; then
      printf 'pid %s was alive\n' "$launch_pid" >"$dir/stdout.txt" || return 1
    fi
    [[ $painted_ok -eq 1 ]] || return 1
  fi
  record window-engine scripts/verify.sh engine || return 1
  code=$(cat "$run/artifacts/verify/window-engine/exit-code.txt") || return 1
  [[ $code -eq 0 ]] || return 1
  [[ -s $run/artifacts/verify/window-engine/stdout.txt ]] || return 1
  if grep -F 'Qt6 QML dev packages not installed. Skipped.' \
    "$run/artifacts/verify/window-engine/stdout.txt" >/dev/null; then
    printf '%s\n' 'Qt6 QML dev packages not installed. Skipped.' \
      >"$run/artifacts/verify/window-engine/miss.txt" || return 1
  elif grep -F 'All engine-path checks passed.' \
    "$run/artifacts/verify/window-engine/stdout.txt" >/dev/null; then
    true
  else
    return 1
  fi
}
omawind_verify_window
```

## Gotchas

- `./run.sh` fails before it looks for a binary when `quickshell` is missing. A missing release build is not what exit 127 means when stderr says `quickshell` is missing.
- `scripts/verify.sh engine` exits 0 on the Qt6 skip. The window fence writes that sentence to `window-engine/miss.txt`. The pass line is `All engine-path checks passed.`
- The start script in `ui/Wind.qml` is `mkdir -p -m 700 "$1" && b="$2"; [ -x "$b" ] || b=omawind; exec "$b" run 2>>"$3"`. It runs with cwd set to `HOME`, and it does not pass `--offline`. The offline engine has to already be listening on `$run/omawind/wind.sock`, or this start fetches from NOMADS and cleanup's pid check fails.
- `quickshell ipc` can call the window's `status` handler only while Quickshell is running. It is not a substitute for `./run.sh` on a machine where `quickshell` is absent.
