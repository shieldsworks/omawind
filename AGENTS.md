# Working on omawind

Omawind is part of Omahoy, a suite of small Omarchy apps for sailors, each in
its own repo under github.com/shieldsworks. It downloads NOAA's HRRR forecast
for the waters around the boat, decodes the GRIB files itself, and serves the
wind at the boat over a Unix socket. A Quickshell bar widget and window read
that socket. The CLI prints the same forecast for a person at a terminal.

## Toolchain

Run `mise install` first. `mise.toml` pins the Rust toolchain (with rustfmt
and clippy). `Cargo.toml`'s `rust-version` is the minimum the code promises.
A system cargo older than that fails. `mise tasks` lists every job.

`curl` is how downloads run, including the station tests that read `file://`
fixtures. Quickshell runs the window (`./run.sh`). CI does not launch it.

## Done means verified

You are not done until this passes from the repo root:

```sh
scripts/verify.sh
```

It runs, in order, `mise lint` (rustfmt check, clippy on all targets with
warnings denied), `mise test`, the comment check, the golden check, qmllint
on `ui/` when qmllint is installed, and a check that verification left no new
files in the tree. CI runs the same steps on x86_64 and aarch64. Fix what it
reports. Never weaken the check that reported it.

Commands:

- `mise install` installs the pinned Rust toolchain.
- `mise lint` is rustfmt and clippy.
- `mise test` is the decoder against `tests/fixtures/hrrr/f01.reference.txt` and the socket tests.
- `mise goldens` runs `scripts/check-goldens.sh`, which prints `omawind at 37.8663,-122.3148` from a temp copy of the Bay fixture and diffs it with `tests/golden/at-berkeley.txt`.
- `scripts/verify.sh` is the single done command.

To prove behavior end to end (the binary, driven the way a user drives it),
use the verify skill at `.cursor/skills/verify/SKILL.md` and its feature map
in `.cursor/skills/verify/features/`.

## The gates are not yours to move

These files set the rules and are changed only in a PR whose whole purpose
is changing them, reviewed by a human:

- `[lints]` in `Cargo.toml` and `clippy.toml`
- `.github/workflows/`, `scripts/verify.sh`, `scripts/check-comments.sh`, `scripts/check-goldens.sh`
- `mise.toml` task definitions for `lint`, `test`, and `goldens`

To silence one lint at one site, use
`#[expect(clippy::<lint>, reason = "<the fact that makes this correct>")]` on
the smallest item. `#[allow]` without a reason is a CI error, and `#[expect]`
fails once the code stops needing it. A crate-level allow is never the fix
for one call site. The integration tests under `tests/` are the exception,
because a panic is how those tests fail.

## Every behavior change has a test or a golden

- A bug fix starts with a failing test that reproduces the bug.
- New behavior lands with the test that pins it. For output a test can't
  easily assert (the `at` table, a rendered window), add or update a golden
  file and say in the PR why it changed.
- Never edit a golden by hand and never regenerate one to make a failing
  test pass without explaining the visible difference.
- Refactors change no test expectations. If one has to change, it was not
  a refactor.
- Tests write only to a temp dir (`std::env::temp_dir()` plus a unique
  name), never into `tests/fixtures/` or the checkout.

## No apologetic comments

Comments state facts about the code and the world it handles ("NOAA ships
cells as ISO 8211, so ..."). They do not apologize, defer, or hedge. These
fail CI (`scripts/check-comments.sh`): TODO, FIXME, XXX, HACK, workaround,
temporary fix, quick fix, for the time being, not ideal, should be fixed,
sorry, kludge, band-aid. If something is wrong, fix it in this change or
open an issue and leave the code honest. A constraint from outside is
written as the fact. What the outside thing does, and what this code does
about it.

## Copy the right pattern

You will copy what you see. Before copying, check that the code you copy
passes today's lints and has a test. Old code may predate the rules. In
particular:

- No `unwrap()` outside tests. Return a `Result` with context, or
  `expect("<invariant that guarantees this>")` when it truly cannot fail.
- Every `unsafe` block carries a `// SAFETY:` comment saying why it is
  sound. Keep unsafe to the existing `libc::flock` wrappers. Don't add
  new ones without need.
- Numeric cast lints are not on in this repo yet. They land with the
  pedantic follow-up. Don't rewrite existing `as` casts in an unrelated change.
- Take the shortcut only if it is also the right path. If the right path is
  hard, say so in the PR instead of shipping the shortcut.

## Review

The agent that wrote a change does not approve, merge, or mark it
verified. A different agent (fresh context, clean checkout) or Casey
reviews it and runs `scripts/verify.sh` plus the verify skill for the
features the change touches.

The PR description lists what changed, the tests or goldens that prove
it, which feature-map entries were driven, and what was not checked.
The reviewer reports what it ran and saw.

## Suite conventions

- Rust, edition 2024, written from scratch with few dependencies. Ask before
  adding a crate. The crates this repo uses are `serde_json`, `libc`,
  `tokio`, and `omakeel-protocol`.
- Always pass `--locked`. Don't change `Cargo.lock` unless the task is a
  dependency change.
- The wire format is specified in `docs/protocol.md`. Change that doc in
  the same PR as the socket code.
- Fixtures in `tests/fixtures/` are real published data. Don't edit them
  by hand. The README says where they came from.
- The QML window in `ui/` runs under Quickshell as an Omarchy plugin
  (`manifest.json`). Keep it in step with the engine's protocol.

## Rules specific to omawind

The GRIB reader and the Lambert grid are written from scratch. Don't add
GDAL, eccodes, pyproj, or any other map library. `scripts/reference.py`
uses eccodes and pyproj to write the oracle text. It is not a dependency
and CI does not run it.

Keep `docs/protocol.md` in sync with `src/engine.rs`. There is no generated
schema. The socket tests are what catch a drift.

## Layout

- `src/main.rs` is the CLI (`run`, `fetch`, `at`, `stations`, `decode`).
- `src/config.rs` is the region, home, and the config and cache paths.
- `src/time.rs` is Unix time and the protocol's ISO strings.
- `src/grid.rs` is the Lambert conformal grid.
- `src/grib.rs` is the GRIB2 decoder.
- `src/forecast.rs` is one run's hours, sampled in space and time.
- `src/fetch.rs` is the NOMADS download and the on-disk cache.
- `src/obs.rs` is NDBC's latest station reports.
- `src/keel.rs` is the client for omakeel's GPS socket.
- `src/engine.rs` is the Unix socket server in `docs/protocol.md`.
- `ui/` is the Quickshell bar widget and window.
- `tests/decode.rs` checks the decoder against the eccodes oracle.
- `tests/engine.rs` drives the socket with a temp copy of the fixtures.
- `tests/fixtures/` holds the recorded HRRR run and NDBC files.
- `tests/golden/at-berkeley.txt` is the committed `omawind at` text.
- `docs/protocol.md` is the socket contract.
- `scripts/verify.sh` is the done command.
