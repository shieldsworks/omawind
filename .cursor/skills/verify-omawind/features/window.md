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
- **Engine path.** The drive runs `env -u OMAWIND_CONFIG XDG_RUNTIME_DIR=$run/engine-probe-runtime scripts/verify.sh engine`. A pass prints `All engine-path checks passed.` and does not print the Qt6 skip. When Qt6 is missing it prints `Qt6 QML dev packages not installed. Skipped.` and exits 0. That exit code is not a pass. If the skip sentence is in stdout or stderr, the drive writes `window-engine/miss.txt` and exits non-zero, including when `All engine-path checks passed.` is also present. `OMAWIND_VERIFY_ENGINE` does not replace that command. If it is set, the drive unsets it, runs `scripts/verify.sh engine`, and exits non-zero. Observed on 2026-10-14. Exit 0. The transcript was `Qt6 QML dev packages not installed. Skipped.` and then `All verification steps passed.` The second line is the script's own summary after that skip. It is not `All engine-path checks passed.`
- **Painted window.** The drive always writes `window-painted`. When `quickshell` is missing, `command.txt` names `quickshell` and `miss.txt` says `missing command: quickshell`. When `quickshell` is present and `grim` is missing, the same files name `grim`. A `miss.txt` makes the drive exit non-zero. When both are present, the drive starts `./run.sh` with `setsid` so the window is its own session, and it keeps that process group id. The window leader has to be alive when `grim` starts and still alive when `grim` returns. Otherwise `window-painted/miss.txt` says `process was not alive` and the drive exits non-zero, including when `grim` printed bytes. The capture command is `timeout -k 5 10 grim -`. `timeout -k` sends KILL five seconds after TERM. A non-zero `grim` exit, an empty image, or a `miss.txt` fails the drive. Do not call a missing-command file a screenshot. Ctrl-C ignores further INT and TERM before it exits, then signals the window session and the engine with TERM then KILL.
- **Proof.** Save the `./run.sh` stderr and the `scripts/verify.sh engine` transcript under `$run/artifacts/verify/window-launch/` and `$run/artifacts/verify/window-engine/`. Do not label them as a window screenshot.

```sh
env -u BASH_ENV bash --noprofile --norc .cursor/skills/verify-omawind/drive.sh window
```

## Gotchas

- `./run.sh` fails before it looks for a binary when `quickshell` is missing. A missing release build is not what exit 127 means when stderr says `quickshell` is missing.
- `scripts/verify.sh engine` exits 0 on the Qt6 skip. The drive writes that sentence to `window-engine/miss.txt` and exits non-zero when the sentence is in stdout or stderr, including beside `All engine-path checks passed.`
- The drive starts `./run.sh` with `setsid` while monitor mode is briefly off, so `setsid` execs and `$!` is the session leader. The drive stores that session id and signals it after the leader has exited. A setsid child, including the engine `Wind.qml` starts with `env -C $HOME`, is its own session. The window stop and the final stop both signal that session. A child that called `setpgid` stays in the session until TERM and then KILL.
- `timeout -k 5 10 grim -` sends KILL five seconds after TERM when `grim` ignores TERM.
- The start script in `ui/Wind.qml` is `mkdir -p -m 700 "$1" && b="$2"; [ -x "$b" ] || b=omawind; exec "$b" run 2>>"$3"`. It runs with cwd set to `HOME`, and it does not pass `--offline`. The offline engine has to already be listening on `$run/omawind/wind.sock`, or this start fetches from NOMADS and cleanup's pid check fails.
- `quickshell ipc` can call the window's `status` handler only while Quickshell is running. It is not a substitute for `./run.sh` on a machine where `quickshell` is absent.
