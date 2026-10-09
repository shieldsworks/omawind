# omawind verification map

This directory is the maintained source for verifying omawind's user-facing
behavior. Read this index before driving the app, then use the matching
feature file as the recipe. Keep it current. A change that adds or alters a
user path updates its feature file in the same PR.

## Baseline preconditions

- A build of this checkout, launched per `../SKILL.md` with its own temp
  `$run` directory (socket, cache, logs).
- The recorded HRRR run copied into
  `$XDG_CACHE_HOME/omawind/hrrr/36.8000_-123.8000_38.8000_-121.6000/2026091403/`
  from `tests/fixtures/hrrr/2026091403/`.
- Doctor passes and names this run's process and socket, for any drive that
  starts `omawind run`.
- Never drive an instance this run did not start.

## Driving conventions

- Start every recipe from the baseline unless its preconditions say otherwise.
- Commands are literal. Keep quoted names and flags unchanged.
- Prefer stable handles. Socket message types from `docs/protocol.md`, CLI
  subcommands and flags, file paths. Not screen coordinates.
- Don't delete proof during cleanup.

## Proof and skip reporting

- CLI proof is the command, stdout, stderr, and exit code.
- Socket proof is the request line and the reply lines, verbatim.
- Window proof (Casey's machine only) is a `grim` screenshot with the app visible.
- Record the feature ID and entry point with every artifact.
- An unreachable path is reported with the command tried and the unmet
  precondition, never as verified through a different path.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the
user-visible behavior, then exactly these four H2s, in order:

1. `Sub-features`: short IDs, one line each.
2. `How to get to it (user POV)`: every user entry point.
3. `Driving it with <harness>`: starts with `Preconditions:`, then labeled
   bullets pairing each user action with an exact command and the
   observable result.
4. `Gotchas`: traps that waste or invalidate a run.

Keep implementation details out of the map. Name user paths, stable
handles, required state, commands, and observable proof.

## Features

- [Forecast at a position](./forecast-at.md) covers `omawind at` on the cached Bay run, including a position outside the grid.
- [Decode a GRIB file](./decode-grib.md) covers `omawind decode` on one fixture hour.
- [Serve the forecast](./serve-forecast.md) covers `omawind run --offline` and the `hello` / `state` messages.
