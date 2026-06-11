# macOS Release Runbook

This document tracks the release path for Observer macOS builds. It keeps signing, notarization, and local verification in one place without storing private Apple account data in the repo.

## Current State

- App identifier: `com.observer.menubar`
- Product name: `观察者`
- Bundle config: `src-tauri/tauri.conf.json`
- Minimum macOS version: `12.0`
- Current bundle targets: app and DMG via `pnpm tauri build`
- Current signing status on this machine: no valid code-signing identity detected

## Required Apple Setup

Install Xcode command line tools and make sure these commands are available:

```bash
xcrun notarytool --help
xcrun stapler --help
security find-identity -v -p codesigning
```

For public distribution outside the Mac App Store, use a `Developer ID Application` certificate.

Recommended environment variables:

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export APPLE_API_ISSUER="00000000-0000-0000-0000-000000000000"
export APPLE_API_KEY="ABC123DEFG"
export APPLE_API_KEY_PATH="$HOME/private_keys/AuthKey_ABC123DEFG.p8"
```

Alternative Apple ID notarization variables:

```bash
export APPLE_ID="name@example.com"
export APPLE_PASSWORD="app-specific-password"
export APPLE_TEAM_ID="TEAMID"
```

Do not commit certificate names, API keys, `.p8` files, app-specific passwords, or team-specific secrets.

## Preflight

Run:

```bash
pnpm release:macos:preflight
```

The preflight checks:

- Node, pnpm, Rust, and Tauri CLI availability.
- Xcode command line tools, `notarytool`, and `stapler`.
- Whether at least one code-signing identity is installed.
- Whether signing and notarization environment variables are present.
- Whether the Tauri bundle identifier and macOS config are present.

The script is non-destructive. It does not sign, submit, upload, or modify Keychain entries.

## Local Unsigned Debug Build

Use this for product testing:

```bash
pnpm build
pnpm tauri build --debug
```

Outputs:

```text
src-tauri/target/debug/bundle/macos/观察者.app
src-tauri/target/debug/bundle/dmg/观察者_0.1.0_aarch64.dmg
```

## Signed Release Build

After certificate and notarization credentials are ready:

```bash
pnpm release:macos:preflight
pnpm build
APPLE_SIGNING_IDENTITY="$APPLE_SIGNING_IDENTITY" pnpm tauri build
```

If using App Store Connect API credentials, keep these variables exported before `pnpm tauri build`:

```bash
APPLE_API_ISSUER
APPLE_API_KEY
APPLE_API_KEY_PATH
```

If using Apple ID credentials, keep these variables exported instead:

```bash
APPLE_ID
APPLE_PASSWORD
APPLE_TEAM_ID
```

## Manual Notarization Fallback

If the Tauri build is signed but notarization is handled manually:

```bash
xcrun notarytool submit "src-tauri/target/release/bundle/dmg/观察者_0.1.0_aarch64.dmg" \
  --key "$APPLE_API_KEY_PATH" \
  --key-id "$APPLE_API_KEY" \
  --issuer "$APPLE_API_ISSUER" \
  --wait

xcrun stapler staple "src-tauri/target/release/bundle/dmg/观察者_0.1.0_aarch64.dmg"
xcrun stapler validate "src-tauri/target/release/bundle/dmg/观察者_0.1.0_aarch64.dmg"
```

## Verification Checklist

Before sharing a release candidate:

- `pnpm build`
- `cargo fmt --check`
- `cargo test`
- `git diff --check`
- `pnpm tauri build`
- `codesign --verify --deep --strict --verbose=2 path/to/观察者.app`
- `spctl --assess --type execute --verbose=4 path/to/观察者.app`
- `xcrun stapler validate path/to/观察者_*.dmg`

Manual smoke test:

- Launch from DMG-installed app.
- Menu bar icon appears.
- Panel opens and closes from the status icon.
- Main display and secondary display panel placement are correct.
- Notification test works after permission is granted.
- Session list, detail, focus, and copy diagnostic actions work.

## Known Follow-Ups

- Add real `Developer ID Application` identity on the release machine.
- Decide whether to store notary credentials in Keychain profiles or CI secrets.
- Decide whether release builds need a custom entitlements file beyond Tauri defaults.
- Add CI release automation only after manual signing and notarization succeed once.
