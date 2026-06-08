# Nirvana — Future Plans

A living roadmap of what we could add. Ordered by priority. Nothing here is
committed; it's the backlog we pull from. Everything must respect the core
constraints: **offline by default, no telemetry, no file deletion, launch only
via official mechanisms.**

---

## Priority 1 — More launchers & stores (color-themed)

Expand discovery beyond Steam / Epic / local. Each store reuses the existing
`Scanner` + OS-seam pattern, and gets its **own brand color** applied to its
source badge / tile accent (a per-source theme token), so the library reads at a
glance.

> ✅ **Shipped:** the extensible multi-store framework (a `STORES` descriptor
> registry), per-source **color theming** (badges + filter pills via
> `list_sources`), and **GOG Galaxy** (registry + `galaxy-2.0.db`, hybrid
> `goggalaxy://`/exe launch). Each remaining store below is now a small
> increment: enum variant + one descriptor row + one scanner.

| Store | Discovery approach (offline) | Launch | Brand accent |
|---|---|---|---|
| ~~**GOG Galaxy**~~ ✅ | ~~`galaxy-2.0.db` (SQLite) under ProgramData, or registry install paths~~ | ~~`goggalaxy://` or game exe~~ | ~~Purple `#a23fff`~~ |
| **Xbox / Microsoft Store** | `Get-AppxPackage` / PackageManager (gaming apps), or `XboxGames` install dirs | `shell:AppsFolder\<AUMID>` | Green `#107c10` |
| **EA app / Origin** | registry + `%PROGRAMDATA%\EA …` manifests | `origin2://` / `eadm://` | Red/Orange `#ff4747` |
| **Ubisoft Connect** | registry `Uplay\Installs` + install dirs | `uplay://launch/<id>` | Blue `#0070ff` |
| **Battle.net** | `.battle.net` product DB / registry | `battlenet://<game>` | Blue `#00aeff` |
| **itch.io** | `butler`/app DB under appdata | app protocol or exe | Pink `#fa5c5c` |
| **Riot** | registry / RiotClientInstalls.json | RiotClient args | Red `#d13639` |

Work items:
- A per-`Source` color token + a small store-icon set; badge + tile-accent themed.
- Source filter pills colored to match (extends the current filter row).
- Settings toggle per store (enable/disable discovery).
- Extend the `Source` enum + dedup ranking + DB CHECK constraint (migration).

## Priority 2 — Library UX

- **Recently played / Continue** row + sort (we already store `last_played` /
  `launch_count`).
- **Right-click context menu** per tile (Launch, Open folder, Favorite, Set
  cover, Tags, Uninstall) — tidier than overlay buttons.
- **First-run onboarding** — prompt to add watch folders + run the first scan so
  an empty library isn't confusing.
- **Keyboard navigation** — `/` focuses search, arrows move, Enter launches.
- **Collections / shelves** — user-defined groups beyond tags.

## Priority 3 — Art & polish

- **Splash screen** (landscape) on launch while the DB opens / first scan runs.
- **Hero/banner art** + better exe-icon fallback; "regenerate art" action.
- **Bulk cover tools** — set covers for a selection; clear custom cover.
- App **icon** wired from `public/icon.png` via `tauri icon`.

## Priority 4 — Monitor & system

- **Per-GPU split** (iGPU vs dGPU) instead of one summed GPU% figure.
- **Per-process** GPU/VRAM top list.
- Temperatures / fan / power (NVML on NVIDIA, behind a feature flag).
- Pin the monitor as a compact always-on-top overlay.

## Priority 5 — Data & playtime

- **Playtime tracking** — poll the launched process to turn `last_played` into
  hours played.
- **Backup / restore** the SQLite library; **CSV / JSON export**.
- Optional **import** of a custom games list.

## Packaging & trust

- **Code signing** — sign the installers + portable exes so SmartScreen stops
  warning on first run. Needs an OV or EV cert (or a cloud signer like Azure
  Trusted Signing) wired into the release workflow. See `RELEASING.md → Code
  signing`. Until then, builds ship unsigned ("More info → Run anyway").
- **Auto-update** — Tauri's updater (signed update artifacts) so installs can
  self-update instead of re-downloading. Depends on signing.

## Nice-to-haves

- Light/dark **accent customization** (pick the primary color).
- Localization scaffolding.
- Steam/Epic **wishlist-free** "installed size trends" over time.

---

*Have an idea? Open an issue. Priority 1 (more stores) is the next focus.*
