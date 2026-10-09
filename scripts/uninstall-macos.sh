#!/usr/bin/env bash
set -euo pipefail

app="${FASTYT_APP_DIR:-$HOME/Applications}/fast.yt.app"
rm -rf "$app"
echo "Removed $app"
