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

- The debug binary is built. No cache and no socket.

- **Hour f01.** The user asks what is in the file. Run `./target/debug/omawind decode tests/fixtures/hrrr/2026091403/f01.grib2`. Exit code 0. Stdout is four lines, starting with `GUST`, `MSLMA`, `UGRD`, and `VGRD`. Each line contains `grid 82×88` and `valid 2026-09-14T04:00:00Z`.
- **Missing file.** Run `./target/debug/omawind decode "$run/missing.grib2"`. Exit code is not 0. Stderr contains `missing.grib2`.
- **Proof.** Save stdout, stderr, and the exit code to `$run/artifacts/verify/decode-f01/` and `$run/artifacts/verify/decode-missing/`.

## Gotchas

- This command reads the path you pass. It does not use the cache. Don't copy the fixture just for this drive.
- The mean and min on each line are decoder output. If they change, say so in the PR. The field names and the valid time are the stable handles.
