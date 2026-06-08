# Bevel UI Implementation Plan

Date: 2026-06-08
Scope: UI refresh planning only. Backend collectors, notifications, panel positioning, and icon behavior stay unchanged.

## Phase 0: Current State Protection

Done:

- Created branch `backup/pre-bevel-ui` at `4dc8553`.
- Created tag `pre-bevel-ui-20260608` at `4dc8553`.
- Created working branch `codex/bevel-ui-refresh`.
- Added Bevel reference board in `docs/design/bevel-reference/`.
- Added Observer UX map in `docs/design/observer-ux-map.md`.

Open note:

- `assets/logo-concepts/` is currently untracked and unrelated to this UI planning pass.

## Phase 1: Design Tokens

Goal: make the current UI easier to restyle before moving markup.

Edit target:

- `src/App.svelte` CSS only.

Tasks:

1. Add root-level tokens for surfaces, text, status colors, spacing, radius, and motion.
2. Replace hard-coded repeated colors in the panel CSS first.
3. Keep visual output close to current UI during this phase.

Verification:

- `pnpm build`
- `git diff --check`

## Phase 2: Panel Home Refresh

Goal: make the default menu panel feel like a Bevel-style health dashboard.

Edit target:

- `src/App.svelte` markup and CSS for the non-detail panel view.

Tasks:

1. Replace the current summary strip with a compact `HealthHero` and `MetricCard` group.
2. Add an `ActivityHeatmap` built from current session risk/activity data.
3. Restyle card list without changing session selection behavior.
4. Preserve compact row mode for multi-session density.

Do not change:

- Panel size.
- Panel window positioning.
- `panel-shown` animation trigger.
- Notifications.

Verification:

- `pnpm build`
- Open the app and test menu bar click on main display and secondary display.
- Confirm compact mode still fits at least four sessions when available.

## Phase 3: Panel Detail Refresh

Goal: turn the session detail view into stacked metric cards.

Edit target:

- `src/App.svelte` detail view markup and CSS.

Tasks:

1. Convert the health hero to a selected-session health card.
2. Convert context/token/process signals into chart-like cards.
3. Keep risk reasons readable and actionable.
4. Keep all current actions reachable: open project, terminal, focus, copy path, copy diagnostic.

Verification:

- `pnpm build`
- Click into a session and return.
- Trigger settings overlay from detail state.

## Phase 4: Settings And Dashboard

Goal: unify the rest of the app after the panel direction is approved.

Settings:

- Keep the existing four tabs.
- Restyle controls with Bevel surfaces and calmer section cards.
- Do not redesign the settings information architecture yet.

Dashboard:

- Convert KPIs to health cards.
- Convert the session table into a dense desktop version of session metric rows.
- Convert inspector detail into the same card system as panel detail.
- Map timeline to the Bevel journal/list pattern.

Verification:

- `pnpm build`
- `cargo fmt --check`
- `cargo test`
- `git diff --check`
- `pnpm tauri build --debug`

## Commit Slices

Recommended:

1. `docs: add bevel reference board and ux map`
2. `refactor(ui): add bevel design tokens`
3. `feat(ui): refresh panel overview`
4. `feat(ui): refresh panel session detail`
5. `feat(ui): refresh dashboard health layout`

## Review Checklist

- The app still feels like a native macOS menu utility, not a copied iOS app.
- The first screen answers: are my agents healthy, active, risky, or done?
- The panel remains dense enough for real monitoring.
- Risk colors are functional and consistent.
- Light-theme references inform tokens, but dark theme ships first.
- No backend behavior changes are bundled with UI refresh commits.

