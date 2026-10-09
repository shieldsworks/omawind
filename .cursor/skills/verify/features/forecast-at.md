# Forecast at a position

`omawind at` prints the cached HRRR wind at a latitude and longitude. With
nothing cached, it exits non-zero and says to run `omawind fetch`.

## Sub-features

- `at-berkeley` prints the three hours of the recorded Bay run at 37.8663,-122.3148.
- `at-outside` exits non-zero when the position is outside the forecast area.

## How to get to it (user POV)

- Run `omawind at 37.8663,-122.3148` in a terminal.
- Run `omawind at` with no position to use home from the settings. With no settings file, home is the same Berkeley point.
- The bar widget shows the same wind. That path needs Quickshell on Casey's machine.

## Driving it with the CLI

Preconditions:

- `$XDG_CACHE_HOME` and `$XDG_CONFIG_HOME` point inside `$run`, and the Bay fixture is copied there as `../SKILL.md` describes.
- Do not pass `--offline`. `at` has no such flag. It only reads the cache.

- **Berkeley.** The user asks for the wind at the marina. Run `./target/debug/omawind at 37.8663,-122.3148`. Exit code 0. Stdout matches `tests/golden/at-berkeley.txt` byte for byte.
- **Outside.** The user asks for a point far from the grid. Run `./target/debug/omawind at 10,10`. Exit code is not 0. Stderr contains `outside the forecast area`.
- **Proof.** Save stdout, stderr, and the exit code to `$run/artifacts/verify/at-berkeley/` and `$run/artifacts/verify/at-outside/`.

## Gotchas

- A real `XDG_CACHE_HOME` or `XDG_CONFIG_HOME` makes this run print the user's forecast. Set both to `$run` before the command.
- The committed golden does not depend on the wall clock. `at` prints every cached hour.
- `scripts/check-goldens.sh` is this drive for `at-berkeley`. It already uses a temp cache.
