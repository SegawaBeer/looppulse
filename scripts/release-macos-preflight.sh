#!/usr/bin/env bash
set -u

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

errors=0
warnings=0

ok() {
  printf "ok: %s\n" "$1"
}

warn() {
  warnings=$((warnings + 1))
  printf "warn: %s\n" "$1"
}

fail() {
  errors=$((errors + 1))
  printf "fail: %s\n" "$1"
}

has_command() {
  command -v "$1" >/dev/null 2>&1
}

require_command() {
  if has_command "$1"; then
    ok "$1 is available"
  else
    fail "$1 is missing"
  fi
}

printf "LoopPulse macOS release preflight\n"
printf "repo: %s\n\n" "$ROOT_DIR"

require_command node
require_command pnpm
require_command cargo
require_command rustc
require_command xcrun
require_command security
require_command codesign
require_command spctl

if pnpm tauri --version >/dev/null 2>&1; then
  ok "Tauri CLI is available through pnpm"
else
  fail "Tauri CLI is not available through pnpm"
fi

if xcrun notarytool --help >/dev/null 2>&1; then
  ok "notarytool is available"
else
  fail "notarytool is unavailable"
fi

if xcrun stapler 2>&1 | grep -q "Usage: stapler"; then
  ok "stapler is available"
else
  fail "stapler is unavailable"
fi

identity_count="$(security find-identity -v -p codesigning 2>/dev/null | awk '/valid identities found/ {print $1}' | tail -1)"
identity_count="${identity_count:-0}"
if [[ "$identity_count" =~ ^[0-9]+$ ]] && [ "$identity_count" -gt 0 ]; then
  ok "$identity_count code-signing identity found"
else
  warn "no valid code-signing identity found; signed release build will fail"
fi

if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  ok "APPLE_SIGNING_IDENTITY is set"
else
  warn "APPLE_SIGNING_IDENTITY is not set"
fi

if [ -n "${APPLE_API_ISSUER:-}" ] && [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_KEY_PATH:-}" ]; then
  ok "App Store Connect API notarization variables are set"
  if [ -f "$APPLE_API_KEY_PATH" ]; then
    ok "APPLE_API_KEY_PATH exists"
  else
    fail "APPLE_API_KEY_PATH does not exist: $APPLE_API_KEY_PATH"
  fi
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  ok "Apple ID notarization variables are set"
else
  warn "notarization variables are not set"
fi

if [ -f "src-tauri/tauri.conf.json" ]; then
  ok "src-tauri/tauri.conf.json exists"
else
  fail "src-tauri/tauri.conf.json is missing"
fi

node <<'NODE'
const fs = require("fs");
const config = JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8"));
const failures = [];
if (config.identifier !== "com.looppulse.menubar") failures.push(`unexpected identifier: ${config.identifier}`);
if (!config.bundle?.active) failures.push("bundle.active is not true");
if (!config.bundle?.macOS?.minimumSystemVersion) failures.push("bundle.macOS.minimumSystemVersion is missing");
if (!config.app?.macOSPrivateApi) failures.push("app.macOSPrivateApi is not enabled");
if (failures.length) {
  for (const failure of failures) console.log(`config-fail: ${failure}`);
  process.exit(1);
}
console.log(`ok: tauri config identifier=${config.identifier} minimumSystemVersion=${config.bundle.macOS.minimumSystemVersion}`);
NODE
node_status=$?
if [ "$node_status" -ne 0 ]; then
  fail "tauri config validation failed"
fi

printf "\nsummary: %s error(s), %s warning(s)\n" "$errors" "$warnings"
if [ "$errors" -gt 0 ]; then
  exit 1
fi

if [ "$warnings" -gt 0 ]; then
  printf "preflight completed with warnings; release signing may still need setup.\n"
else
  printf "preflight completed successfully.\n"
fi
