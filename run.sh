#!/usr/bin/env bash
# Opens the wind in a window of its own, from this checkout, in its own
# Quickshell process. The window starts the engine (OMAWIND_BIN, default
# this checkout's release build) when it isn't already running.
set -euo pipefail
cd "$(dirname "$0")"
exec quickshell -p ui/shell.qml "$@"
