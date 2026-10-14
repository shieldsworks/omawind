# Forecast at a position

`omawind at` prints the cached HRRR wind at a latitude and longitude. With no position it uses home from the settings, which is the Berkeley marina when no settings file exists. With nothing cached, it exits non-zero and says to run `omawind fetch`.

## Sub-features

- `at-berkeley` prints the three hours of the recorded Bay run at 37.8663,-122.3148.
- `at-home` prints that same table when the user passes no position and no settings file.
- `at-empty` exits non-zero when the cache has no run.
- `at-outside` exits non-zero when the position is outside the forecast area.

## How to get to it (user POV)

- Run `omawind at 37.8663,-122.3148` in a terminal.
- Run `omawind at` with no position. With no settings file, home is the same Berkeley point.
- The bar widget shows the same wind. That path needs Quickshell. See `window.md`.

## Driving it with the CLI

Preconditions:

- Same bash shell as the launch fence in `../SKILL.md`. `$run` and `record` are set. Do not export `HOME` or the XDG variables.
- Do not pass `--offline`. `at` has no such flag. It only reads the cache.
- `at-empty` uses `$run/empty-cache`, an empty cache inside `$run`. Do not `mktemp` a second directory outside `$run`. Cleanup deletes `$run` and that cache goes with it.

- **Berkeley.** The user asks for the wind at the marina. Run `env -u OMAWIND_CONFIG HOME="$run/home" XDG_CACHE_HOME="$run/cache" XDG_CONFIG_HOME="$run/config" XDG_RUNTIME_DIR="$run" ./target/debug/omawind at 37.8663,-122.3148`. Exit code 0. Stdout matches `tests/golden/at-berkeley.txt` byte for byte. Observed on 2026-10-14. Exit 0, stderr empty, and `diff -u` against the golden printed nothing.
- **Home.** The user asks for the wind at home. Run `env -u OMAWIND_CONFIG HOME="$run/home" XDG_CACHE_HOME="$run/cache" XDG_CONFIG_HOME="$run/config" XDG_RUNTIME_DIR="$run" ./target/debug/omawind at`. Exit code 0. Stdout matches the same golden. Observed on 2026-10-14. Exit 0, stderr empty, and the same `diff -u` printed nothing.
- **Empty cache.** The user asks before any fetch. Run `env -u OMAWIND_CONFIG HOME="$run/empty-cache/home" XDG_CACHE_HOME="$run/empty-cache/cache" XDG_CONFIG_HOME="$run/empty-cache/config" XDG_RUNTIME_DIR="$run" ./target/debug/omawind at 37.8663,-122.3148` after creating those three directories. Exit code is not 0. Stderr contains `nothing cached for the region yet`. Observed on 2026-10-14. Exit 1 and stderr `omawind: nothing cached for the region yet: run omawind fetch`.
- **Outside.** The user asks for a point far from the grid. Run `env -u OMAWIND_CONFIG HOME="$run/home" XDG_CACHE_HOME="$run/cache" XDG_CONFIG_HOME="$run/config" XDG_RUNTIME_DIR="$run" ./target/debug/omawind at 10,10`. Exit code is not 0. Stderr contains `outside the forecast area`. Observed on 2026-10-14. Exit 1 and stderr `omawind: that position is outside the forecast area`.
- **Proof.** `record` saves the command, stdout, stderr, and the exit code to `$run/artifacts/verify/at-berkeley/`, `$run/artifacts/verify/at-home/`, `$run/artifacts/verify/at-empty/`, and `$run/artifacts/verify/at-outside/`.

```sh
omawind_verify_at() {
  local opts
  opts=$(set +o)
  trap 'eval "$opts"; trap - RETURN' RETURN
  set +e
  set -o pipefail
  [[ -n ${run:-} && -d $run ]] || return 1
  record at-berkeley env -u OMAWIND_CONFIG \
    HOME="$run/home" \
    XDG_CACHE_HOME="$run/cache" \
    XDG_CONFIG_HOME="$run/config" \
    XDG_RUNTIME_DIR="$run" \
    ./target/debug/omawind at 37.8663,-122.3148 || return 1
  [[ $(cat "$run/artifacts/verify/at-berkeley/exit-code.txt") -eq 0 ]] || return 1
  diff -u tests/golden/at-berkeley.txt "$run/artifacts/verify/at-berkeley/stdout.txt" || return 1
  [[ ! -e $run/artifacts/verify/at-berkeley/stderr.txt ]] || return 1
  record at-home env -u OMAWIND_CONFIG \
    HOME="$run/home" \
    XDG_CACHE_HOME="$run/cache" \
    XDG_CONFIG_HOME="$run/config" \
    XDG_RUNTIME_DIR="$run" \
    ./target/debug/omawind at || return 1
  [[ $(cat "$run/artifacts/verify/at-home/exit-code.txt") -eq 0 ]] || return 1
  diff -u tests/golden/at-berkeley.txt "$run/artifacts/verify/at-home/stdout.txt" || return 1
  [[ ! -e $run/artifacts/verify/at-home/stderr.txt ]] || return 1
  mkdir -p "$run/empty-cache/home" "$run/empty-cache/cache" "$run/empty-cache/config" || return 1
  record at-empty env -u OMAWIND_CONFIG \
    HOME="$run/empty-cache/home" \
    XDG_CACHE_HOME="$run/empty-cache/cache" \
    XDG_CONFIG_HOME="$run/empty-cache/config" \
    XDG_RUNTIME_DIR="$run" \
    ./target/debug/omawind at 37.8663,-122.3148 || return 1
  [[ $(cat "$run/artifacts/verify/at-empty/exit-code.txt") -ne 0 ]] || return 1
  grep -F 'nothing cached for the region yet' "$run/artifacts/verify/at-empty/stderr.txt" || return 1
  record at-outside env -u OMAWIND_CONFIG \
    HOME="$run/home" \
    XDG_CACHE_HOME="$run/cache" \
    XDG_CONFIG_HOME="$run/config" \
    XDG_RUNTIME_DIR="$run" \
    ./target/debug/omawind at 10,10 || return 1
  [[ $(cat "$run/artifacts/verify/at-outside/exit-code.txt") -ne 0 ]] || return 1
  grep -F 'outside the forecast area' "$run/artifacts/verify/at-outside/stderr.txt" || return 1
}
omawind_verify_at
```

## Gotchas

- `config.rs` reads `OMAWIND_CONFIG` first. Without `-u OMAWIND_CONFIG`, a value in your shell points `at` at the user's settings. Pass `HOME`, `XDG_CACHE_HOME`, `XDG_CONFIG_HOME`, and `XDG_RUNTIME_DIR` on the command. Do not export them.
- The committed golden does not depend on the wall clock. `at` prints every cached hour.
- The byte compare was checked on x86_64. The Lambert math uses libm. If aarch64 rounds a displayed tenth differently, CI prints the diff. Do not edit the golden to hide that until you have both texts.
- `scripts/check-goldens.sh` is the `at-berkeley` drive. It builds first, copies the fixture into a temp cache, and diffs stdout. It does not cover `at-home`, `at-empty`, or `at-outside`.
