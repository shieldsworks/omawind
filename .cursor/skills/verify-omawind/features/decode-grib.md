# Decode a GRIB file

`omawind decode` lists the fields in one GRIB2 file. The names, the grid
size, and the valid time are what a person checks.

## Sub-features

- `decode-f01` lists the four fields in the recorded hour `f01.grib2`.
- `decode-missing` exits non-zero when the file is not there.

## How to get to it (user POV)

- Run `omawind decode tests/fixtures/hrrr/2026091403/f01.grib2` in a terminal.

## Driving it with the CLI

Preconditions:

- Same bash shell as the launch fence in `../SKILL.md`, so `$run` and `record` exist.
- `decode` reads the path you pass. It does not read the cache, and it does not open the socket. Leave the fixture in the repo. The copy under `$run/cache` is not for this drive.

- **Hour f01.** The user asks what is in the file. Run `env -u OMAWIND_CONFIG HOME="$run/home" XDG_CACHE_HOME="$run/cache" XDG_CONFIG_HOME="$run/config" XDG_RUNTIME_DIR="$run" ./target/debug/omawind decode tests/fixtures/hrrr/2026091403/f01.grib2`. Exit code 0. Stdout is four lines, in order `GUST`, `MSLMA`, `UGRD`, `VGRD`. Each line contains `grid 82×88` and `valid 2026-09-14T04:00:00Z`. Observed on 2026-10-14. Exit 0, stderr empty, and those four prefixes and tokens were present.
- **Missing file.** Run `env -u OMAWIND_CONFIG HOME="$run/home" XDG_CACHE_HOME="$run/cache" XDG_CONFIG_HOME="$run/config" XDG_RUNTIME_DIR="$run" ./target/debug/omawind decode "$run/missing.grib2"`. Exit code is not 0. Stderr contains `missing.grib2`. Observed on 2026-10-14. Exit 1 and stderr contained `missing.grib2` and `No such file or directory`.
- **Proof.** `record` saves the command, stdout, stderr, and the exit code to `$run/artifacts/verify/decode-f01/` and `$run/artifacts/verify/decode-missing/`.

```sh
omawind_verify_decode() {
  local opts got want
  opts=$(set +o)
  trap 'eval "$opts"; trap - RETURN' RETURN
  set +e
  set -o pipefail
  [[ -n ${run:-} && -d $run ]] || return 1
  record decode-f01 env -u OMAWIND_CONFIG \
    HOME="$run/home" \
    XDG_CACHE_HOME="$run/cache" \
    XDG_CONFIG_HOME="$run/config" \
    XDG_RUNTIME_DIR="$run" \
    ./target/debug/omawind decode tests/fixtures/hrrr/2026091403/f01.grib2 || return 1
  [[ $(cat "$run/artifacts/verify/decode-f01/exit-code.txt") -eq 0 ]] || return 1
  [[ ! -e $run/artifacts/verify/decode-f01/stderr.txt ]] || return 1
  got=$(awk '{print $1}' "$run/artifacts/verify/decode-f01/stdout.txt") || return 1
  want=$(printf '%s\n' GUST MSLMA UGRD VGRD)
  [[ $got == "$want" ]] || return 1
  grep -F 'grid 82×88' "$run/artifacts/verify/decode-f01/stdout.txt" >/dev/null || return 1
  grep -F 'valid 2026-09-14T04:00:00Z' "$run/artifacts/verify/decode-f01/stdout.txt" >/dev/null || return 1
  record decode-missing env -u OMAWIND_CONFIG \
    HOME="$run/home" \
    XDG_CACHE_HOME="$run/cache" \
    XDG_CONFIG_HOME="$run/config" \
    XDG_RUNTIME_DIR="$run" \
    ./target/debug/omawind decode "$run/missing.grib2" || return 1
  [[ $(cat "$run/artifacts/verify/decode-missing/exit-code.txt") -ne 0 ]] || return 1
  grep -F 'missing.grib2' "$run/artifacts/verify/decode-missing/stderr.txt" >/dev/null || return 1
}
omawind_verify_decode
```

## Gotchas

- This command reads the path you pass. It does not use the cache. The launch still copies the Bay fixture for `at` and for the offline engine. That copy is unused here.
- The field names, the grid size, and the valid time are the stable handles. The min, the max, and the mean on each line are decoder output. If those numbers change, say so in the PR.
