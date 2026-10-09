#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

cargo build --locked

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
export XDG_CACHE_HOME="$root/cache"
export XDG_CONFIG_HOME="$root/config"
mkdir -p "$XDG_CACHE_HOME" "$XDG_CONFIG_HOME"

dest="$XDG_CACHE_HOME/omawind/hrrr/36.8000_-123.8000_38.8000_-121.6000/2026091403"
mkdir -p "$dest"
cp tests/fixtures/hrrr/2026091403/f00.grib2 \
  tests/fixtures/hrrr/2026091403/f01.grib2 \
  tests/fixtures/hrrr/2026091403/f02.grib2 \
  "$dest/"

got="$root/at.txt"
./target/debug/omawind at 37.8663,-122.3148 >"$got"

golden=tests/golden/at-berkeley.txt
if [[ ${1:-} == --write ]]; then
  mkdir -p tests/golden
  cp "$got" "$golden"
  exit 0
fi
if [[ ! -f $golden ]]; then
  printf 'missing %s; run scripts/check-goldens.sh --write\n' "$golden" >&2
  exit 1
fi
diff -u "$golden" "$got"
