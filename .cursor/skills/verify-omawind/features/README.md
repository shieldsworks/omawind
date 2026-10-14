# omawind verification map

This directory is the maintained source for verifying omawind's user-facing behavior. Read this index before driving the app, then use the matching feature file as the recipe.

## Baseline preconditions

- A build of this checkout, launched per `../SKILL.md`, with its own `$run` directory for the socket, the cache, and the logs.
- The recorded HRRR run copied into `$run/cache/omawind/hrrr/36.8000_-123.8000_38.8000_-121.6000/2026091403/` from `tests/fixtures/hrrr/2026091403/`.
- Doctor passes and names this run's process and socket, for any drive that starts `omawind run`.
- Never drive an instance this run did not start.
- The drive is `bash --noprofile --norc ../drive.sh`. It is a separate process. The shell that starts it keeps its options and traps. The drive does not export `HOME` or the XDG variables.

## Driving conventions

- Start every recipe from the baseline unless its preconditions say otherwise.
- Commands are literal. Keep quoted names and flags unchanged.
- Prefer stable handles. Socket message types from `docs/protocol.md`, CLI subcommands and flags, and file paths. Not screen coordinates.
- Do not delete proof during cleanup.

## Proof and skip reporting

- CLI proof is the command, stdout, stderr, and the exit code.
- Socket proof is the three lines the server pushes on connect, verbatim. The reader sends no request.
- Window proof on a Hyprland session is a `grim` image in `window-painted`. That directory is always written. When `quickshell` or `grim` is missing, it names the missing command and the drive exits non-zero. Do not call that file a screenshot. `scripts/verify.sh engine` passes this map only when `All engine-path checks passed.` is in the transcript and the Qt6 skip line is in neither stdout nor stderr. If the skip line appears in either stream, it is a miss.
- Record the feature ID and the entry point with every artifact.
- An unreachable path is reported with the command tried and the unmet precondition. Do not report it as verified through a different path.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the user-visible behavior, then exactly these four H2s, in order.

1. `Sub-features` lists short IDs, one line each.
2. `How to get to it (user POV)` lists every user entry point.
3. The third H2 is `Driving it with the CLI`, `Driving it with the socket`, or `Driving it with the start script`. It starts with `Preconditions:`, then labeled bullets that pair each user action with an exact command and the observable result.
4. `Gotchas` lists traps that waste or invalidate a run.

Keep implementation details out of the map. Name user paths, stable handles, required state, commands, and observable proof.

## Features

- [Forecast at a position](./forecast-at.md) covers `omawind at` on the cached Bay run, at home with no settings file, with an empty cache, and outside the grid.
- [Decode a GRIB file](./decode-grib.md) covers `omawind decode` on one fixture hour and on a missing file.
- [Serve the forecast](./serve-forecast.md) covers `omawind run --offline` and the `hello`, `state`, and `stations` lines.
- [The wind window](./window.md) covers `./run.sh` and the engine path the window starts. The painted window needs Quickshell.

`omawind fetch` and `omawind stations` call NOAA. They have no fixture flag. They are not feature files. Drive them only when the task is that network path, and say that you did.
