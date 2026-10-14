#!/usr/bin/env bash
if [[ -n ${BASH_ENV:-} ]]; then
  unset BASH_ENV
  exec bash --noprofile --norc "$0" "$@"
fi

seamed=0
if [[ -n ${OMAWIND_VERIFY_ENGINE:-} ]]; then
  seamed=1
fi
unset OMAWIND_VERIFY_ENGINE

set -euo pipefail
set -m

repo=$(cd "$(dirname "$0")/../../.." && pwd)
cd "$repo"

run=
evidence=
engine_pid=
window_pid=
window_pgid=
grim_pid=
grim_pgid=
launch_n=0
copy_failed=0
kept_run=
all_pgids=
self_pgid=$(ps -o pgid= -p $$ | tr -d ' ')
started_pid=

if [[ -n ${EVIDENCE_DIR:-} ]]; then
  mkdir -p "$EVIDENCE_DIR"
  evidence=$(mktemp -d "$EVIDENCE_DIR/drive.XXXXXX")
else
  evidence=$(mktemp -d /tmp/omawind-verify-evidence.XXXXXX)
fi

add_pgid() {
  local pgid=${1:-}
  [[ -n $pgid && $pgid != "$self_pgid" ]] || return 0
  case " $all_pgids " in
    *" $pgid "*) ;;
    *) all_pgids="$all_pgids $pgid" ;;
  esac
}

note_tree() {
  local pid=${1:-} child pgid
  [[ -n $pid ]] || return 0
  pgid=$(ps -o pgid= -p "$pid" 2>/dev/null | tr -d ' ' || true)
  add_pgid "$pgid"
  kill -0 "$pid" 2>/dev/null || return 0
  for child in $(ps -o pid= --ppid "$pid" 2>/dev/null || true); do
    note_tree "$child"
  done
}

start_session() {
  local out=$1 err=$2
  shift 2
  set +m
  if [[ $out == "$err" ]]; then
    setsid "$@" >"$out" 2>&1 </dev/null &
  else
    setsid "$@" >"$out" 2>"$err" </dev/null &
  fi
  started_pid=$!
  set -m
  add_pgid "$started_pid"
}

signal_pgids() {
  local sig=$1 pgid
  shift
  for pgid in "$@"; do
    [[ -n $pgid && $pgid != "$self_pgid" ]] || continue
    kill "-$sig" -- "-$pgid" 2>/dev/null || true
  done
}

sweep_run() {
  local pid cwd cmd envfile
  [[ -n ${run:-} && -d $run ]] || return 0
  for pid in /proc/[0-9]*; do
    pid=${pid#/proc/}
    [[ $pid == "$$" ]] && continue
    cwd=$(readlink -f "/proc/$pid/cwd" 2>/dev/null || true)
    cmd=$(tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null || true)
    envfile=/proc/$pid/environ
    if [[ $cwd == "$run" || $cwd == "$run"/* || $cmd == *"$run"* ]]; then
      kill -KILL "$pid" 2>/dev/null || true
      continue
    fi
    if [[ -r $envfile ]] && tr '\0' '\n' <"$envfile" | grep -F -q "XDG_RUNTIME_DIR=$run"; then
      kill -KILL "$pid" 2>/dev/null || true
    fi
  done
}

stop_all() {
  local pgid pid
  local -a ids=()
  note_tree "${engine_pid:-}"
  note_tree "${window_pid:-}"
  note_tree "${grim_pid:-}"
  for pgid in $all_pgids; do
    ids+=("$pgid")
  done
  signal_pgids TERM "${ids[@]}"
  signal_pgids KILL "${ids[@]}"
  for pgid in "${ids[@]}"; do
    for pid in $(pgrep -g "$pgid" 2>/dev/null || true); do
      kill -KILL "$pid" 2>/dev/null || true
    done
  done
  sweep_run
  engine_pid=
  window_pid=
  window_pgid=
  grim_pid=
  grim_pgid=
  all_pgids=
}

stop_groups() {
  local pgid pid
  local -a ids=()
  for pgid in "$@"; do
    [[ -n $pgid && $pgid != "$self_pgid" ]] || continue
    note_tree "$pgid"
    ids+=("$pgid")
  done
  signal_pgids TERM "${ids[@]}"
  signal_pgids KILL "${ids[@]}"
  for pgid in "${ids[@]}"; do
    for pid in $(pgrep -g "$pgid" 2>/dev/null || true); do
      kill -KILL "$pid" 2>/dev/null || true
    done
  done
}

archive_current() {
  local dest marker ok old
  [[ -n ${run:-} && -d ${run:-} ]] || return 0
  stop_all
  launch_n=$((launch_n + 1))
  dest=$evidence/launch-$launch_n
  marker=$run/artifacts/.copy-marker
  ok=0
  mkdir -p "$run/artifacts" "$dest"
  printf '%s\n' "$run" >"$marker"
  if cp -a "$run/artifacts/." "$dest/"; then
    if [[ -f $dest/.copy-marker && $(cat "$dest/.copy-marker") == "$run" ]]; then
      ok=1
    fi
  fi
  if [[ $ok -eq 1 ]]; then
    old=$run
    run=
    rm -rf "$old"
  else
    copy_failed=1
    kept_run=$run
    printf 'copy failed, run dir kept: %s\n' "$run" >&2
    run=
  fi
}

finish() {
  local status=$?
  trap '' INT TERM
  trap - EXIT
  set +e
  archive_current
  if [[ $seamed -eq 1 ]]; then
    printf 'OMAWIND_VERIFY_ENGINE was set\n' >&2
    if [[ $status -eq 0 ]]; then
      status=1
    fi
  fi
  if [[ $copy_failed -eq 1 && $status -eq 0 ]]; then
    status=1
  fi
  printf 'evidence: %s\n' "$evidence"
  exit "$status"
}
trap finish EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

oma() {
  env -u OMAWIND_CONFIG \
    HOME="$run/home" \
    XDG_CACHE_HOME="$run/cache" \
    XDG_CONFIG_HOME="$run/config" \
    XDG_RUNTIME_DIR="$run" \
    "$@"
}

record() {
  local id=$1 dir code
  shift
  dir=$run/artifacts/verify/$id
  mkdir -p "$dir"
  printf '%q ' "$@" >"$dir/command.txt"
  printf '\n' >>"$dir/command.txt"
  set +e
  "$@" >"$dir/stdout.txt" 2>"$dir/stderr.txt"
  code=$?
  set -e
  printf '%s\n' "$code" >"$dir/exit-code.txt"
  [[ -s $dir/stdout.txt ]] || rm -f "$dir/stdout.txt"
  [[ -s $dir/stderr.txt ]] || rm -f "$dir/stderr.txt"
  return 0
}

launch() {
  local dest old ready _
  [[ -f Cargo.toml && -d tests/fixtures/hrrr ]] || exit 1
  if [[ -n ${run:-} ]]; then
    old=$run
    archive_current
    [[ ! -d $old ]] || exit 1
  fi
  export PATH="${HOME}/.local/bin:${PATH}"
  mise install
  mise exec -- cargo build --locked
  run=$(mktemp -d /tmp/omawind-verify.XXXXXX)
  [[ $run == /* ]] || exit 1
  mkdir -p "$run/home" "$run/cache" "$run/config" "$run/artifacts/verify"
  sock=$run/omawind/wind.sock
  dest=$run/cache/omawind/hrrr/36.8000_-123.8000_38.8000_-121.6000/2026091403
  mkdir -p "$dest"
  cp tests/fixtures/hrrr/2026091403/f00.grib2 \
    tests/fixtures/hrrr/2026091403/f01.grib2 \
    tests/fixtures/hrrr/2026091403/f02.grib2 \
    "$dest/"
  pgrep -x omawind | sort >"$run/pids-before" || true
  start_session "$run/engine.log" "$run/engine.log" \
    env -u OMAWIND_CONFIG \
    HOME="$run/home" \
    XDG_CACHE_HOME="$run/cache" \
    XDG_CONFIG_HOME="$run/config" \
    XDG_RUNTIME_DIR="$run" \
    ./target/debug/omawind run --offline --socket "$sock"
  engine_pid=$started_pid
  ready=0
  for _ in $(seq 50); do
    if [[ -S $sock ]]; then
      ready=1
      break
    fi
    sleep 0.1
  done
  [[ $ready -eq 1 ]] || exit 1
  kill -0 "$engine_pid" || exit 1
}

drive_doctor() {
  local pid
  record doctor oma ./target/debug/omawind --version
  [[ $(cat "$run/artifacts/verify/doctor/exit-code.txt") -eq 0 ]]
  cat "$run/artifacts/verify/doctor/stdout.txt"
  [[ -S $sock ]]
  printf 'socket exists\n'
  pid=$engine_pid
  kill -0 "$pid"
  printf 'pid %s alive\n' "$pid"
}

drive_at() {
  record at-berkeley oma ./target/debug/omawind at 37.8663,-122.3148
  [[ $(cat "$run/artifacts/verify/at-berkeley/exit-code.txt") -eq 0 ]]
  diff -u tests/golden/at-berkeley.txt "$run/artifacts/verify/at-berkeley/stdout.txt"
  [[ ! -e $run/artifacts/verify/at-berkeley/stderr.txt ]]
  record at-home oma ./target/debug/omawind at
  [[ $(cat "$run/artifacts/verify/at-home/exit-code.txt") -eq 0 ]]
  diff -u tests/golden/at-berkeley.txt "$run/artifacts/verify/at-home/stdout.txt"
  [[ ! -e $run/artifacts/verify/at-home/stderr.txt ]]
  mkdir -p "$run/empty-cache/home" "$run/empty-cache/cache" "$run/empty-cache/config"
  record at-empty env -u OMAWIND_CONFIG \
    HOME="$run/empty-cache/home" \
    XDG_CACHE_HOME="$run/empty-cache/cache" \
    XDG_CONFIG_HOME="$run/empty-cache/config" \
    XDG_RUNTIME_DIR="$run" \
    ./target/debug/omawind at 37.8663,-122.3148
  [[ $(cat "$run/artifacts/verify/at-empty/exit-code.txt") -ne 0 ]]
  grep -F 'nothing cached for the region yet' "$run/artifacts/verify/at-empty/stderr.txt" >/dev/null
  record at-outside oma ./target/debug/omawind at 10,10
  [[ $(cat "$run/artifacts/verify/at-outside/exit-code.txt") -ne 0 ]]
  grep -F 'outside the forecast area' "$run/artifacts/verify/at-outside/stderr.txt" >/dev/null
}

drive_decode() {
  local got want
  record decode-f01 oma ./target/debug/omawind decode tests/fixtures/hrrr/2026091403/f01.grib2
  [[ $(cat "$run/artifacts/verify/decode-f01/exit-code.txt") -eq 0 ]]
  [[ ! -e $run/artifacts/verify/decode-f01/stderr.txt ]]
  got=$(awk '{print $1}' "$run/artifacts/verify/decode-f01/stdout.txt")
  want=$(printf '%s\n' GUST MSLMA UGRD VGRD)
  [[ $got == "$want" ]]
  grep -F 'grid 82×88' "$run/artifacts/verify/decode-f01/stdout.txt" >/dev/null
  grep -F 'valid 2026-09-14T04:00:00Z' "$run/artifacts/verify/decode-f01/stdout.txt" >/dev/null
  record decode-missing oma ./target/debug/omawind decode "$run/missing.grib2"
  [[ $(cat "$run/artifacts/verify/decode-missing/exit-code.txt") -ne 0 ]]
  grep -F 'missing.grib2' "$run/artifacts/verify/decode-missing/stderr.txt" >/dev/null
}

drive_serve() {
  [[ -S $sock ]]
  record serve-forecast python3 - "$sock" <<'EOF'
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
  [[ $(cat "$run/artifacts/verify/serve-forecast/exit-code.txt") -eq 0 ]]
  python3 - "$run/artifacts/verify/serve-forecast/stdout.txt" <<'EOF'
import json, sys
lines = [json.loads(line) for line in open(sys.argv[1]) if line.strip()]
if len(lines) != 3:
    raise SystemExit("expected 3 lines, got %s" % len(lines))
hello, state, stations = lines
if hello.get("type") != "hello" or hello.get("v") != 1:
    raise SystemExit("hello")
fetch = state.get("fetch") or {}
forecast = state.get("forecast") or {}
if state.get("type") != "state" or fetch.get("status") != "off":
    raise SystemExit("fetch status")
if forecast.get("run") != "2026-09-14T03:00:00Z" or forecast.get("hours") != 3:
    raise SystemExit("forecast")
if state.get("keel") != "lost":
    raise SystemExit("keel")
if stations.get("type") != "stations" or stations.get("status") != "off":
    raise SystemExit("stations status")
if stations.get("stations") != []:
    raise SystemExit("stations list")
EOF
}

window_miss() {
  local id=$1 name=$2 dir
  dir=$run/artifacts/verify/$id
  mkdir -p "$dir"
  printf '%s\n' "$name" >"$dir/command.txt"
  printf 'missing command: %s\n' "$name" >"$dir/stdout.txt"
  printf '%s\n' '0' >"$dir/exit-code.txt"
  printf 'missing command: %s\n' "$name" >"$dir/miss.txt"
}

window_dead() {
  local dir=$run/artifacts/verify/window-painted
  mkdir -p "$dir"
  printf '%s\n' 'grim -' >"$dir/command.txt"
  printf '%s\n' 'process was not alive' >"$dir/stdout.txt"
  printf '%s\n' '1' >"$dir/exit-code.txt"
  printf '%s\n' 'process was not alive' >"$dir/miss.txt"
}

window_up() {
  [[ -n ${window_pid:-} && -n ${window_pgid:-} ]] || return 1
  kill -0 "$window_pid" 2>/dev/null || return 1
  kill -0 -- "-$window_pgid" 2>/dev/null
}

transcript_has_skip() {
  local dir=$1 f
  for f in "$dir/stdout.txt" "$dir/stderr.txt"; do
    [[ -f $f ]] || continue
    if grep -F -q 'Qt6 QML dev packages not installed. Skipped.' "$f"; then
      return 0
    fi
  done
  return 1
}

drive_window() {
  local wayland runtime code dir painted_ok _
  local -a cmd
  [[ -S ${sock:-} && -d ${run:-} ]]
  wayland=${WAYLAND_DISPLAY-}
  if [[ -n $wayland && $wayland != /* ]]; then
    runtime=${XDG_RUNTIME_DIR-}
    [[ -n $runtime && $runtime == /* ]]
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
  painted_ok=1
  if ! command -v quickshell >/dev/null 2>&1; then
    record window-launch "${cmd[@]}"
    code=$(cat "$run/artifacts/verify/window-launch/exit-code.txt")
    [[ $code -eq 127 ]]
    grep -F 'quickshell' "$run/artifacts/verify/window-launch/stderr.txt" >/dev/null
    window_miss window-painted quickshell
    painted_ok=0
  else
    dir=$run/artifacts/verify/window-launch
    mkdir -p "$dir"
    printf '%q ' "${cmd[@]}" >"$dir/command.txt"
    printf '\n' >>"$dir/command.txt"
    start_session "$dir/stdout.txt" "$dir/stderr.txt" "${cmd[@]}"
    window_pid=$started_pid
    window_pgid=$started_pid
    code=0
    for _ in $(seq 20); do
      if window_up; then
        break
      fi
      sleep 0.1
    done
    if ! window_up; then
      set +e
      wait "$window_pid" 2>/dev/null
      code=$?
      set -e
      printf '%s\n' "$code" >"$dir/exit-code.txt"
      [[ -s $dir/stdout.txt ]] || rm -f "$dir/stdout.txt"
      [[ -s $dir/stderr.txt ]] || rm -f "$dir/stderr.txt"
      if [[ ! -e $dir/stdout.txt && ! -e $dir/stderr.txt ]]; then
        printf 'pid %s exited\n' "$window_pid" >"$dir/stdout.txt"
      fi
      window_dead
      stop_groups "$window_pgid"
      window_pid=
      window_pgid=
      painted_ok=0
    elif ! command -v grim >/dev/null 2>&1; then
      window_miss window-painted grim
      painted_ok=0
      stop_groups "$window_pgid"
      set +e
      wait "$window_pid" 2>/dev/null
      code=$?
      set -e
      printf '%s\n' "$code" >"$dir/exit-code.txt"
      window_pid=
      window_pgid=
    else
      if ! window_up; then
        window_dead
        stop_groups "$window_pgid"
        window_pid=
        window_pgid=
        painted_ok=0
      else
        dir=$run/artifacts/verify/window-painted
        mkdir -p "$dir"
        printf '%s\n' 'timeout -k 5 10 grim -' >"$dir/command.txt"
        set +m
        setsid timeout -k 5 10 grim - >"$dir/stdout.txt" 2>"$dir/stderr.txt" </dev/null &
        grim_pid=$!
        grim_pgid=$grim_pid
        set -m
        add_pgid "$grim_pgid"
        for _ in $(seq 10); do
          note_tree "$grim_pid"
          pgrep -P "$grim_pid" >/dev/null 2>&1 && break
          sleep 0.02
        done
        set +e
        wait "$grim_pid"
        code=$?
        set -e
        grim_pid=
        printf '%s\n' "$code" >"$dir/exit-code.txt"
        [[ -s $dir/stdout.txt ]] || rm -f "$dir/stdout.txt"
        [[ -s $dir/stderr.txt ]] || rm -f "$dir/stderr.txt"
        if ! window_up; then
          painted_ok=0
          printf '%s\n' 'process was not alive' >"$dir/miss.txt"
          set +e
          wait "$window_pid" 2>/dev/null
          code=$?
          set -e
          printf '%s\n' "$code" >"$run/artifacts/verify/window-launch/exit-code.txt"
        elif [[ $code -eq 0 && -s $dir/stdout.txt ]]; then
          painted_ok=1
        else
          painted_ok=0
          if [[ ! -e $dir/stdout.txt && ! -e $dir/stderr.txt ]]; then
            printf 'exit %s\n' "$code" >"$dir/stderr.txt"
          fi
        fi
        local still_up=0
        if window_up; then
          still_up=1
        fi
        stop_groups "$window_pgid" "$grim_pgid"
        if [[ $still_up -eq 1 ]]; then
          set +e
          wait "$window_pid" 2>/dev/null
          code=$?
          set -e
          printf '%s\n' "$code" >"$run/artifacts/verify/window-launch/exit-code.txt"
          [[ -s $run/artifacts/verify/window-launch/stdout.txt ]] || rm -f "$run/artifacts/verify/window-launch/stdout.txt"
          [[ -s $run/artifacts/verify/window-launch/stderr.txt ]] || rm -f "$run/artifacts/verify/window-launch/stderr.txt"
          if [[ ! -e $run/artifacts/verify/window-launch/stdout.txt && ! -e $run/artifacts/verify/window-launch/stderr.txt ]]; then
            printf 'pid %s was alive\n' "$window_pid" >"$run/artifacts/verify/window-launch/stdout.txt"
          fi
        fi
        window_pid=
        window_pgid=
        grim_pgid=
      fi
    fi
  fi
  runtime=$run/engine-probe-runtime
  mkdir -p "$runtime"
  chmod 700 "$runtime"
  [[ $runtime == "$run"/* ]]
  record window-engine env -u OMAWIND_CONFIG \
    HOME="$run/home" \
    XDG_CACHE_HOME="$run/cache" \
    XDG_CONFIG_HOME="$run/config" \
    XDG_RUNTIME_DIR="$runtime" \
    scripts/verify.sh engine
  dir=$run/artifacts/verify/window-engine
  if transcript_has_skip "$dir"; then
    printf '%s\n' 'Qt6 QML dev packages not installed. Skipped.' >"$dir/miss.txt"
  fi
  if [[ -f $run/artifacts/verify/window-painted/miss.txt || -f $dir/miss.txt ]]; then
    return 1
  fi
  [[ $(cat "$dir/exit-code.txt") -eq 0 ]]
  [[ $painted_ok -eq 1 ]]
  local saw_pass=1
  if [[ -f $dir/stdout.txt ]] && grep -F -q 'All engine-path checks passed.' "$dir/stdout.txt"; then
    saw_pass=0
  fi
  if [[ -f $dir/stderr.txt ]] && grep -F -q 'All engine-path checks passed.' "$dir/stderr.txt"; then
    saw_pass=0
  fi
  [[ $saw_pass -eq 0 ]]
}

plant_detached() {
  local dest=$1
  python3 - "$run" "$dest" <<'PY'
import os, sys

run, dest = sys.argv[1], sys.argv[2]
big = "B" * 130000
read_fd, write_fd = os.pipe()
if os.fork() > 0:
    os.close(write_fd)
    data = b""
    while True:
        chunk = os.read(read_fd, 256)
        if not chunk:
            break
        data += chunk
    os.close(read_fd)
    with open(dest, "w", encoding="utf-8") as handle:
        handle.write(data.decode())
    os._exit(0)
os.close(read_fd)
os.setsid()
os.chdir("/tmp")
leader = os.getpid()
sibling = os.fork()
if sibling == 0:
    os.setpgid(0, 0)
    os.chdir("/tmp")
    os.execvpe("sleep", ["sleep", "600"], {"PATH": "/usr/bin:/bin", "HOME": "/tmp"})
os.setpgid(sibling, sibling)
os.write(write_fd, f"leader {leader}\nsibling {sibling}\n".encode())
os.close(write_fd)
os.execvpe(
    "sleep",
    ["sleep", "600"],
    {
        "PATH": "/usr/bin:/bin",
        "HOME": "/tmp",
        "XDG_RUNTIME_DIR": run,
        "BIG": big,
    },
)
PY
}

drive_hold() {
  local hold_phase
  run=$(mktemp -d /tmp/omawind-verify.XXXXXX)
  [[ $run == /* ]] || exit 1
  mkdir -p "$run/home" "$run/artifacts/verify/hold"
  printf 'hold\n' >"$run/artifacts/verify/hold/stdout.txt"
  printf '0\n' >"$run/artifacts/verify/hold/exit-code.txt"
  printf 'run %s\n' "$run"

  hold_phase=groups
  trap 'hold_phase=go' USR1
  start_session "$run/window.log" "$run/window.log" sleep 600
  window_pid=$started_pid
  window_pgid=$started_pid
  plant_detached "$run/plant-a"
  printf 'observe-groups\n'
  printf 'window %s\n' "$window_pid"
  cat "$run/plant-a"
  while [[ $hold_phase == groups ]]; do
    sleep 0.05 || true
  done
  trap - USR1
  stop_groups "$window_pgid"
  window_pid=
  window_pgid=
  printf 'groups-stopped\n'

  start_session "$run/engine.log" "$run/engine.log" sleep 600
  engine_pid=$started_pid
  plant_detached "$run/plant-b"
  printf 'ready\n'
  printf 'engine %s\n' "$engine_pid"
  cat "$run/plant-b"
  wait "$engine_pid"
}

drive_twice() {
  local prior
  launch
  mkdir -p "$run/artifacts/verify/marker"
  printf 'first\n' >"$run/artifacts/verify/marker/stdout.txt"
  printf '0\n' >"$run/artifacts/verify/marker/exit-code.txt"
  prior=$run
  printf 'prior-run: %s\n' "$prior"
  launch
  [[ ! -d $prior ]]
  grep -F 'first' "$evidence/launch-1/verify/marker/stdout.txt" >/dev/null
  kill -0 "$engine_pid"
}

if [[ $# -eq 0 ]]; then
  set -- doctor at decode serve window
fi

for feature in "$@"; do
  case $feature in
    doctor | at | decode | serve | window | twice | hold) ;;
    *)
      printf 'unknown feature: %s\n' "$feature" >&2
      exit 2
      ;;
  esac
done

if [[ $# -eq 1 && $1 == twice ]]; then
  drive_twice
  exit 0
fi

if [[ $# -eq 1 && $1 == hold ]]; then
  drive_hold
  exit 0
fi

launch
for feature in "$@"; do
  "drive_$feature"
done
