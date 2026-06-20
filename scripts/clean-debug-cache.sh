#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEBUG_DIR="$ROOT_DIR/src-tauri/target/debug"
LATEST_APP="$DEBUG_DIR/bundle/macos/LoopPulse.app"

if [[ ! -d "$DEBUG_DIR" ]]; then
  echo "No debug target found."
  exit 0
fi

if [[ ! -d "$LATEST_APP" ]]; then
  echo "Latest LoopPulse.app was not found at:"
  echo "$LATEST_APP"
  echo "Run a debug build before cleaning, or keep the full target directory."
  exit 1
fi

before="$(du -sh "$DEBUG_DIR" | awk '{print $1}')"

rm -rf \
  "$DEBUG_DIR/deps" \
  "$DEBUG_DIR/incremental" \
  "$DEBUG_DIR/build" \
  "$DEBUG_DIR/examples"

rm -f \
  "$DEBUG_DIR/libobserver_lib.a" \
  "$DEBUG_DIR/libobserver_lib.d" \
  "$DEBUG_DIR/libobserver_lib.dylib" \
  "$DEBUG_DIR/libobserver_lib.rlib" \
  "$DEBUG_DIR/observer.d"

if [[ -d "$DEBUG_DIR/bundle/macos" ]]; then
  find "$DEBUG_DIR/bundle/macos" -mindepth 1 -maxdepth 1 ! -name "LoopPulse.app" -exec rm -rf {} +
fi

rm -rf \
  "$DEBUG_DIR/bundle/dmg" \
  "$DEBUG_DIR/bundle/share"

after="$(du -sh "$DEBUG_DIR" | awk '{print $1}')"

echo "Debug target cleaned: $before -> $after"
echo "Kept latest app: $LATEST_APP"
