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

- `$HOME`, `$XDG_CACHE_HOME`, and `$XDG_CONFIG_HOME` point inside `$run`, and the Bay fixture is copied there as `../SKILL.md` describes.
- Do not pass `--offline`. `at` has no such flag. It only reads the cache.
- `at-empty` uses a second directory with an empty cache. Do not point it at the fixture cache.

- **Berkeley.** The user asks for the wind at the marina. Run `./target/debug/omawind at 37.8663,-122.3148`. Exit code 0. Stdout matches `tests/golden/at-berkeley.txt` byte for byte.
- **Home.** The user asks for the wind at home. Run `./target/debug/omawind at`. Exit code 0. Stdout matches the same golden.
- **Empty cache.** The user asks before any fetch. Run `empty=$(mktemp -d /tmp/omawind-verify-empty.XXXXXX) && mkdir -p "$empty/home" "$empty/cache" "$empty/config" && HOME="$empty/home" XDG_CACHE_HOME="$empty/cache" XDG_CONFIG_HOME="$empty/config" ./target/debug/omawind at 37.8663,-122.3148`. Exit code is not 0. Stderr contains `nothing cached for the region yet`. Remove `$empty` only after the proof is copied out.
- **Outside.** The user asks for a point far from the grid. Run `./target/debug/omawind at 10,10`. Exit code is not 0. Stderr contains `outside the forecast area`.
- **Proof.** Save stdout, stderr, and the exit code to `$run/artifacts/verify/at-berkeley/`, `$run/artifacts/verify/at-home/`, `$run/artifacts/verify/at-empty/`, and `$run/artifacts/verify/at-outside/`.

## Gotchas

- A real `HOME`, `XDG_CACHE_HOME`, or `XDG_CONFIG_HOME` makes this run print the user's forecast. Set all three inside `$run` before the command.
- The committed golden does not depend on the wall clock. `at` prints every cached hour.
- The byte compare was checked on x86_64. The Lambert math uses libm. If aarch64 rounds a displayed tenth differently, CI prints the diff. Do not edit the golden to hide that until you have both texts.
- `scripts/check-goldens.sh` is the `at-berkeley` drive. It builds first, copies the fixture into a temp cache, and diffs stdout. It does not cover `at-home`, `at-empty`, or `at-outside`.
