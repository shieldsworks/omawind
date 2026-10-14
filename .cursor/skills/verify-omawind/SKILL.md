---
name: verify-omawind
description: "Drive omawind the way a sailor does (the CLI, the engine socket, and the Quickshell window when it can run) and keep proof. Use before calling any behavior change done, when reviewing someone else's change, or when asked to verify omawind."
---

# Verify omawind

`scripts/verify.sh` proves the code is sound. This skill proves the app works. It runs the binary from this checkout, drives a feature from the user's side, and keeps evidence. Run both. The agent that built a change does not sign off on this run.

## Launch

Run from the repo root, in bash. `at` and `decode` exit on their own. Build once, then start the offline engine. `omawind run` stays up. Give every run its own directory so two drives do not share a socket or a cache.

Run the fences in this order, in one shell. Launch, doctor, the fences in `features/`, then cleanup.

Each fence is one function. Sourcing the fence defines that function and calls it. The function saves `set +o` and installs a RETURN trap that restores those options, so the operator's shell options are back when the fence returns. A failed check is `return 1`. The interactive shell keeps running.

`set -e` is not used. In interactive bash, `set -e` inside a function exits the whole shell on the failing command, and the RETURN trap does not run. `local -` does not prevent that. `set -u` on an unset expansion also skips the RETURN trap and leaves nounset on. The trap has to be set in the fence function itself. A helper that sets the trap restores options when the helper returns, before the fence body runs.

Do not `export` `HOME`, `XDG_CACHE_HOME`, `XDG_CONFIG_HOME`, or `XDG_RUNTIME_DIR`. `config.rs` reads `OMAWIND_CONFIG` before those variables, so every omawind command also passes `-u OMAWIND_CONFIG`. The prefix on each command is `env -u OMAWIND_CONFIG HOME=... XDG_CACHE_HOME=... XDG_CONFIG_HOME=... XDG_RUNTIME_DIR=...`.

`cargo` on `PATH` may be older than `Cargo.toml` `rust-version`. `mise exec -- cargo build --locked` uses the toolchain in `mise.toml`.

`ui/Wind.qml` opens `$XDG_RUNTIME_DIR/omawind/wind.sock`. Bind the offline engine there, under `$run`, so the window attaches to this server.

```sh
omawind_verify_launch() {
  local opts ready dest _
  opts=$(set +o)
  trap 'eval "$opts"; trap - RETURN' RETURN
  set +e
  set -o pipefail
  omawind_verify_stop() {
    local dir=$1 pid _
    [[ -n ${dir:-} && -f $dir/pid ]] || return 0
    pid=$(cat "$dir/pid")
    if kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
    fi
    for _ in $(seq 50); do
      if ! kill -0 "$pid" 2>/dev/null; then
        wait "$pid" 2>/dev/null || true
        return 0
      fi
      sleep 0.1
    done
    return 1
  }
  [[ -f Cargo.toml && -d tests/fixtures/hrrr ]] || return 1
  mise install || return 1
  mise exec -- cargo build --locked || return 1
  if [[ -n ${_omawind_verify_run:-} ]]; then
    omawind_verify_stop "$_omawind_verify_run" || return 1
  fi
  run=$(mktemp -d /tmp/omawind-verify.XXXXXX)
  [[ $run == /* ]] || return 1
  _omawind_verify_run=$run
  trap "omawind_verify_stop $(printf '%q' "$run")" EXIT
  mkdir -p "$run/home" "$run/cache" "$run/config" "$run/artifacts/verify" || return 1
  sock=$run/omawind/wind.sock
  if [[ -n ${EVIDENCE_DIR:-} ]]; then
    mkdir -p "$EVIDENCE_DIR" || return 1
    evidence=$(mktemp -d "$EVIDENCE_DIR/drive.XXXXXX")
  else
    evidence=$(mktemp -d /tmp/omawind-verify-evidence.XXXXXX)
  fi
  dest=$run/cache/omawind/hrrr/36.8000_-123.8000_38.8000_-121.6000/2026091403
  mkdir -p "$dest" || return 1
  cp tests/fixtures/hrrr/2026091403/f00.grib2 \
    tests/fixtures/hrrr/2026091403/f01.grib2 \
    tests/fixtures/hrrr/2026091403/f02.grib2 \
    "$dest/" || return 1
  pgrep -x omawind | sort >"$run/pids-before" || true
  env -u OMAWIND_CONFIG \
    HOME="$run/home" \
    XDG_CACHE_HOME="$run/cache" \
    XDG_CONFIG_HOME="$run/config" \
    XDG_RUNTIME_DIR="$run" \
    ./target/debug/omawind run --offline --socket "$sock" \
    >"$run/engine.log" 2>&1 &
  printf '%s\n' "$!" >"$run/pid" || return 1
  disown "$!" 2>/dev/null || true
  ready=0
  for _ in $(seq 50); do
    if [[ -S $sock ]]; then
      ready=1
      break
    fi
    sleep 0.1
  done
  [[ $ready -eq 1 ]] || return 1
  record() {
    local id=$1 dir code
    shift
    dir=$run/artifacts/verify/$id
    mkdir -p "$dir" || return 1
    printf '%q ' "$@" >"$dir/command.txt" || return 1
    printf '\n' >>"$dir/command.txt" || return 1
    set +e
    "$@" >"$dir/stdout.txt" 2>"$dir/stderr.txt"
    code=$?
    printf '%s\n' "$code" >"$dir/exit-code.txt" || return 1
    [[ -s $dir/stdout.txt ]] || rm -f "$dir/stdout.txt"
    [[ -s $dir/stderr.txt ]] || rm -f "$dir/stderr.txt"
    return 0
  }
}
omawind_verify_launch
```

Ready when `$sock` is a socket and the pid in `$run/pid` is alive. `at` and `decode` do not open that socket. They still take the `env` prefix, with `HOME`, `XDG_CACHE_HOME`, and `XDG_CONFIG_HOME` inside `$run`.

`mktemp` prints an absolute path. A relative `XDG_RUNTIME_DIR` makes `omawind run` exit even when `--socket` is set, because the default omakeel socket is still resolved. `$run` has to start with `/`.

`record` writes the command, stdout, stderr, and the exit code under `$run/artifacts/verify/<id>/`. It removes an empty stdout or stderr file so a later check can require every copied file to be non-empty. A missing stdout file means the command printed nothing there. `record` does not turn `set -e` back on.

The EXIT trap calls `omawind_verify_stop` with the directory from this launch, quoted into the trap when it is installed. It does not expand `$run` when the shell exits. A second launch stops that engine before it creates a new directory, so the first process is not left running. Cleanup runs `trap - EXIT` after the engine is dead.

## Doctor

Run this before the first drive, and again when anything looks off. A missing socket or a dead pid returns 1. `test -S && echo` returns 0 when the socket is missing, so the check is an `if`.

```sh
omawind_verify_doctor() {
  local opts pid
  opts=$(set +o)
  trap 'eval "$opts"; trap - RETURN' RETURN
  set +e
  set -o pipefail
  [[ -n ${run:-} && -n ${sock:-} ]] || return 1
  record doctor env -u OMAWIND_CONFIG \
    HOME="$run/home" \
    XDG_CACHE_HOME="$run/cache" \
    XDG_CONFIG_HOME="$run/config" \
    XDG_RUNTIME_DIR="$run" \
    ./target/debug/omawind --version || return 1
  cat "$run/artifacts/verify/doctor/stdout.txt" || return 1
  [[ $(cat "$run/artifacts/verify/doctor/exit-code.txt") -eq 0 ]] || return 1
  if [[ ! -S $sock ]]; then
    printf 'socket missing: %s\n' "$sock" >&2
    return 1
  fi
  printf 'socket exists\n'
  pid=$(cat "$run/pid") || return 1
  if ! kill -0 "$pid" 2>/dev/null; then
    printf 'pid %s is dead\n' "$pid" >&2
    return 1
  fi
  printf 'pid %s alive\n' "$pid"
}
omawind_verify_doctor
```

The version line is this checkout's binary. The pid is the process this run started. The socket is `$run/omawind/wind.sock`. Connect only to that socket. Do not drive an omawind the user already has running. Do not read `~/.config/omawind` or `~/.cache/omawind`.

Observed on 2026-10-14.

```
omawind 0.1.0
socket exists
pid 4782 alive
```

The pid differs every launch.

## Drive

Follow `features/`. Start at `features/README.md`. A proof that drives one convenient entry point is incomplete when the map lists others.

- A sailor's terminal is `./target/debug/omawind at` and `./target/debug/omawind decode`, each with the `env` prefix.
- An app reads `$sock`, one JSON object per line, as `docs/protocol.md` describes. The server pushes three lines on connect. The reader is in `features/serve-forecast.md`.
- The window is `./run.sh`, which execs `quickshell -p ui/shell.qml`. The window fence always writes `window-painted`. When `quickshell` is not on `PATH`, that directory names the missing command. When it is, the fence starts `./run.sh` in the background, captures with `grim`, and stops it.

`omawind fetch` and `omawind stations` call NOAA. They are not in this map. Drive them only when the task is that network path, and say that you did. The CLI has no flag that points them at `tests/fixtures/`.

## Evidence

`$evidence` is one directory per drive, created in the launch fence. Cleanup copies `$run/artifacts/verify/` there before it deletes `$run`.

- Save the command, stdout, stderr, and the exit code. `record` does that.
- For the socket, save the three lines the server pushes on connect. The reader sends no request.
- The recorded HRRR files stand in for NOMADS. The cache reader that `omawind fetch` fills is the one that loads them. `--offline` must not open a connection to NOMADS. The proof is the state line `"fetch":{"status":"off"}` and the stations line `"status":"off"`. An empty `$run/engine.log` does not show that. The engine does not write those URLs to the log.
- `window-painted` is always written. On a Hyprland session with Quickshell, it is the `grim` image. When `quickshell` or `grim` is missing, the directory names that command and `miss.txt` records the miss. Do not call that file a screenshot. `window-engine` is a pass only when stdout contains `All engine-path checks passed.` The Qt6 line `Qt6 QML dev packages not installed. Skipped.` is a miss, written to `miss.txt`, even though `scripts/verify.sh` exits 0.
- If a path cannot be reached, write the command and the missing precondition. Do not mark that path passed.

## Cleanup

Copy the evidence out first. Then stop only the pid this run wrote. Confirm that pid is dead before deleting `$run`. Then confirm `pgrep -x omawind` shows no pid that was not already in `$run/pids-before`. A failed window connect starts `omawind run` with no `--offline`. That check is how you see the stray. Do not `pkill` by name. After the pid is dead and the evidence checks pass, clear the EXIT trap.

```sh
omawind_verify_cleanup() {
  local opts extra found f d
  opts=$(set +o)
  trap 'eval "$opts"; trap - RETURN' RETURN
  set +e
  set -o pipefail
  [[ -n ${run:-} && -n ${evidence:-} && -d $run ]] || return 1
  mkdir -p "$evidence" || return 1
  cp -a "$run/artifacts/verify/." "$evidence/" || return 1
  omawind_verify_stop "$run" || return 1
  pgrep -x omawind | sort >"$run/pids-after" || true
  extra=$(comm -13 "$run/pids-before" "$run/pids-after" || true)
  if [[ -n $extra ]]; then
    printf 'an omawind process was started outside this run:\n%s\n' "$extra" >&2
    return 1
  fi
  found=0
  while IFS= read -r -d '' f; do
    found=1
    if [[ ! -s $f ]]; then
      printf 'empty evidence: %s\n' "$f" >&2
      return 1
    fi
  done < <(find "$evidence" -type f -print0)
  [[ $found -eq 1 ]] || { printf 'evidence directory has no files\n' >&2; return 1; }
  while IFS= read -r -d '' d; do
    [[ -s $d/exit-code.txt ]] || { printf 'missing exit code: %s\n' "$d" >&2; return 1; }
    [[ -s $d/stdout.txt || -s $d/stderr.txt ]] || {
      printf 'empty evidence: %s\n' "$d" >&2
      return 1
    }
  done < <(find "$evidence" -mindepth 1 -maxdepth 1 -type d -print0)
  trap - EXIT
  unset _omawind_verify_run
  rm -rf "$run" || return 1
  [[ -d $evidence ]] || return 1
}
omawind_verify_cleanup
```

Evidence stays under `$evidence`. `$run` is gone. The EXIT trap is gone. The fences do not export `HOME` or the XDG variables, so those names in this shell are the ones you started with.

On the 2026-10-14 run the pid was gone, the socket was gone, and `at-berkeley/stdout.txt` was still under the evidence directory.

When omawind's commands or the lines it prints change, run `/maintain-verification-skill` over this directory.

## Helpers

This skill ships no script. The socket reader is the Python block in `features/serve-forecast.md`. The window's engine-path check is the repo's own probe.

```sh
omawind_verify_engine_probe() {
  local opts
  opts=$(set +o)
  trap 'eval "$opts"; trap - RETURN' RETURN
  set +e
  set -o pipefail
  scripts/verify.sh engine || return 1
}
omawind_verify_engine_probe
```

`scripts/verify.sh` lines 68 to 70 assign `XDG_RUNTIME_DIR` when it is unset, using `/tmp/runtime-omawind-verify`, then `mkdir` that directory and `chmod 700` it. When the variable is already set, the script creates that directory instead. The script is a subprocess, so the assignment does not stay in your shell. The directory does.

The probe substitutes a stub for `omawind run`. It does not fetch from NOAA. When Qt6 is missing it prints `Qt6 QML dev packages not installed. Skipped.` and exits 0. The window fence records that line in `miss.txt`. That skip is not a pass of `features/window.md`. A pass of the engine path is the line `All engine-path checks passed.`
