# Changelog

All notable changes to **Nirvana** are recorded here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions use
[SemVer](https://semver.org/).

## [Unreleased]

## [0.1.0-alpha.4] — 2026-06-08

### Added
- **GOG Galaxy** as a discovered source: games are found from the Windows
  registry (works for DRM-free installs without the Galaxy client) and enriched
  by Galaxy's local `galaxy-2.0.db` when present — fully offline, deduped by GOG
  product id. Launch is hybrid: the `goggalaxy://` protocol when the Galaxy
  client is installed, otherwise a validated direct-exe spawn.
- An extensible **multi-store framework** (a `STORES` descriptor registry) so new
  stores are a localized addition; source badges and filter pills are now
  **color-themed** per store (GOG = purple), driven by a `list_sources` command.
- **Continue row** — a recently-played strip at the top of the library.
- **First-run onboarding** — an inline welcome (Scan now / Add watch folders)
  instead of a bare empty grid on first launch.
- **Right-click + keyboard context menu** per game (Launch, Open folder,
  Favorite, Set cover, Edit tags, Add to collection, Uninstall) — opened by
  right-click or Shift+F10.
- **Keyboard navigation** — arrow keys move focus across the grid, Enter
  launches, `/` focuses search, Esc clears, Shift+F10 opens the menu.
- **Collections** — user-curated, named groups of games: filter by a collection,
  add/remove via the context menu, manage them in Settings.

### Changed
- The library's source filter pills are data-driven (a new store appears,
  themed, with no UI change).
- Game tiles drop the tag/cover overlay buttons (those actions moved into the
  context menu); the favorite star stays.
- Themed, slim scrollbars; the Continue row is capped at 5 games in a
  non-scrolling responsive grid.
- Disabled the unused `withGlobalTauri` global API bridge (smaller injected
  runtime, reduced attack surface).

### Fixed
- Phantom horizontal scrollbar on every page (sub-pixel rounding) — clipped at
  the root without affecting the sticky nav or vertical scroll.

## [0.1.0-alpha.3] — 2026-06-08

### Added
- **Online edition** of each release — installer + portable built with the
  optional SteamGridDB cover-art feature (`Nirvana-online_*`), alongside the
  default offline build. Still OFF by default; key kept in the OS vault.
- `CONTRIBUTING.md` — setup, conventions, and the offline/no-deletion ground rules.

### Changed
- README "latest" version is now a live release badge (no hardcoded version).
- Debug builds use a separate `nirvana-dev.db`, so dev-only seeded dummy games
  never appear in a release build's library/disk views.

### Fixed
- **Cold-start crash** "state not managed for `get_library`" on first launch —
  the main window is now created after app state is initialized.
- **Phantom Epic games** (e.g. a long-uninstalled Fortnite) no longer appear:
  the scan skips `*.item` manifests whose install folder is gone.
- **Monitor graphs**: the memory sparkline is no longer a flat line (it now
  min-max scales to its own range), and all sparklines are smoothed.

## [0.1.0-alpha.2] — 2026-06-08

### Added
- **Custom borderless title bar** themed to the app (drag region + min/restore/
  close controls) replacing the default Windows frame.
- Tag **filtering** + per-tag **colors** (clickable colored chips, tag pill row).
- Library **layout switcher** (Comfortable grid / Compact list), persisted.
- **System** panel on the Monitor page + an **Open Task Manager** button.
- **About** section in Settings (what Nirvana is + the tech stack).
- **Custom cover thumbnails** — set a local image as a game's cover (offline).
- **Delete database** option (two-step confirm) in Settings.
- Tag-triggered **GitHub release workflow** (NSIS installer + portable exe).
- `FUTURE-PLANS.md` roadmap (Priority 1: more launchers/stores, color-themed).

### Changed
- **Compact** layout is now a single-game-per-row **list view** (the small-grid
  compact was buggy).
- **System** info redesigned as labeled stat cards (icon + label + value).
- **Settings** panels now fill the width as a responsive grid (no more narrow
  left column / blank right side).
- **Sticky header** — the nav stays pinned while the page scrolls (all views).
- Vite build tuned (modern WebView2 target, Oxc minify) and IPC diagnostics
  gated to dev builds only.

### Fixed
- GPU panel now detects adapters (WMI runs on a clean COM thread) and VRAM shows
  real system-wide usage (PDH) instead of `0 B`.
- Local discovery no longer lists every installed program — it scans only the
  user's configured **watch folders** (no app noise); stale entries are pruned
  on each scan.
- Cold-start scan error on the empty library no longer surfaces (progress
  listener hardened + one retry).

## [0.1.0-alpha.1] — 2026-06-07

First public alpha — a fully offline Windows game launcher.

### Added

- **Discovery + launch** across **Steam** (`libraryfolders.vdf` →
  `appmanifest_*.acf`), **Epic** (`*.item` manifests), and **local** installs
  (Uninstall registry + configurable watch folders). Results are deduped and
  persisted to SQLite. Launch via official protocols (Steam/Epic) or a validated
  argv spawn (local) — never a shell.
- **Library grid** with offline cover art (Steam cache → exe icon → placeholder),
  source badges, and sizes. **Filters** (source, favorites), **search** (name),
  and **sort** (name / size / last-played).
- **Favorites and tags** per game, persisted.
- **Disk view** — per-drive capacity and biggest-games, with junction-safe
  recursive sizing; "open install folder" and store-uninstall deep links (Nirvana
  never deletes files).
- **System monitor** — ~1 Hz CPU / RAM / network / disk / GPU + VRAM with
  sparklines, running only while the view is visible and focused (idle CPU ≈ 0).
  GPU model/driver via WMI.
- **Settings** — theme, monitor interval, watch folders, and an opt-in
  SteamGridDB toggle. A "Support" section with an offline UPI donation QR.
- Light/dark themes (**dark by default**) with CSS view transitions.

### Security & privacy

- **Zero network in the default build.** SteamGridDB cover art is opt-in and
  feature-gated **off** (no `reqwest` unless built with `--features steamgriddb`).
- Restrictive CSP, least-privilege capabilities, parameterized SQL, validated
  executable launches, and the SteamGridDB key kept in the OS credential vault.

### Packaging

- Per-user **NSIS installer** (no admin) and a **portable** `nirvana.exe`.
- Size-optimized release profile (~7 MB binary).

[Unreleased]: https://github.com/svssdeva/nirvana/compare/v0.1.0-alpha.4...HEAD
[0.1.0-alpha.4]: https://github.com/svssdeva/nirvana/compare/v0.1.0-alpha.3...v0.1.0-alpha.4
[0.1.0-alpha.3]: https://github.com/svssdeva/nirvana/compare/v0.1.0-alpha.2...v0.1.0-alpha.3
[0.1.0-alpha.2]: https://github.com/svssdeva/nirvana/compare/v0.1.0-alpha.1...v0.1.0-alpha.2
[0.1.0-alpha.1]: https://github.com/svssdeva/nirvana/releases/tag/v0.1.0-alpha.1
