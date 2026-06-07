# PRD — Unified Game Launcher (Windows)

**Codename:** TBD · **Owner:** Deva · **License:** MIT (100% open source) · **Status:** Draft v0.2
**v0.2 changes:** dropped ALL online features (no login, no store browse). Added real-time system monitor. Offline cover-art strategy.

---

## 1. Summary

A lightweight, **fully offline** Tauri + Rust desktop app for Windows that discovers, organizes, and launches games across **Steam, Epic, and arbitrary local installs** from one UI. Adds a **disk manager** with per-game space analysis, **GPU/driver info**, and a **real-time minimal system monitor** (memory / disk / network / GPU).

**No network. No accounts. No store. No payments. No telemetry.** Pure local launcher + library + insight.

---

## 2. Goals / Non-goals

### Goals
- One-click discovery of all installed games (Steam + Epic + local), zero config.
- Single launch surface; <50MB binary, <1s cold start, near-zero idle CPU.
- Disk insight: per-game size, per-drive total/free, biggest-consumers ranking.
- Real-time minimal monitor: RAM, disk I/O, network throughput, GPU util/VRAM.
- Show GPU model + driver version.
- 100% open source, zero network calls in core, zero paid services, no ads/tracking/cookies.

### Non-goals
- **No login / OAuth / accounts** (Steam or Epic).
- **No store browsing, search, sales, pricing, purchase.**
- No game install/download mgmt (launch only via official protocol).
- No file deletion / uninstall by us (deep-link to store uninstaller only).
- No cloud, no CI/CD, no Linux/Mac in v1 (Rust core kept port-friendly).
- No anti-cheat interaction; never modify store/auth/network traffic.

---

## 3. Users
- **Primary:** PC gamer with games across Steam + Epic + standalone who wants one offline launcher + storage clarity + a glanceable resource monitor.
- **Secondary:** Tinkerer wanting an open-source, no-bloat, no-network alternative to heavy launchers.

---

## 4. Scope by milestone

| Milestone | Contents | Risk |
|---|---|---|
| **M1 — Discovery + launch** | One-click scan (Steam/Epic/local), library grid, launch, cover art | Low |
| **M2 — Disk** | Per-game sizing, per-drive stats, biggest-games view | Low |
| **M3 — Monitor + GPU** | Real-time RAM/disk/net/GPU view, GPU model + driver version | Low–Med |
| **M4 — Polish** | Tags, filters, playtime, last-played, theming | Low |

All offline. No milestone depends on a network call.

---

## 5. Functional requirements

### 5.1 One-click library scan (`FR-SCAN`)
Single action aggregates installed games from all sources, dedups, persists to local SQLite.

- **Steam:** `HKCU\Software\Valve\Steam\SteamPath` → `steamapps\libraryfolders.vdf` (use the copy under `Steam\config\` — `steamapps` copy gets overwritten on start) → each library's `appmanifest_*.acf` (VDF) → `appid`, `name`, `installdir`, `SizeOnDisk`, `StateFlags`.
- **Epic:** `C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests\*.item` (JSON) → `AppName`, `DisplayName`, `InstallLocation`, `LaunchExecutable`, `InstallSize`. **Installed games only** — no login, so owned-but-uninstalled titles are out of scope (by design).
- **Local/other:** registry `HKLM`/`HKCU\...\Uninstall\*` → DisplayName/InstallLocation/DisplayIcon, filtered to game-likely entries; plus user-added watch folders scanned for `.exe`.

Dedup: normalized install path + name. Each entry tagged source (steam/epic/local). Incremental + cached; full rescan on demand.

**Acceptance:** fresh launch → Scan → all installed Steam/Epic/local titles appear with source badge, path, size — no manual input, no network.

### 5.2 Launch (`FR-LAUNCH`)
- Steam: `steam://rungameid/<appid>`
- Epic: `com.epicgames.launcher://apps/<AppName>?action=launch&silent=true`
- Local: spawn resolved `.exe` (cwd = exe dir).
- Record `last_played`, `launch_count`.

**Acceptance:** Play → correct title launches via official mechanism; no store-client bypass.

### 5.3 Disk manager + per-game space (`FR-DISK`)
- Per drive: total/used/free (`sysinfo`).
- Per game: real on-disk size = parallel recursive sum (`jwalk`); manifest-reported size shown as placeholder while computing.
- Views: games by size desc; grouped by drive; bar/treemap of top N; total used by games.
- Actions: open install folder; "uninstall via store" deep-link. **We never delete files in v1.**

**Acceptance:** disk view shows accurate per-game GB + drive free + ranked biggest-games within seconds for a typical library.

**Footgun [LOW]:** Steam library moves use junctions/reparse points → skip reparse points, track visited paths to avoid double-count / loops.

### 5.4 GPU + driver info (`FR-GPU`)
- Adapters via WMI `Win32_VideoController` → `Name`, `DriverVersion`, `DriverDate`, `AdapterRAM`.
- NVIDIA: optional enrich via `nvml-wrapper` (clean driver version, VRAM, temp, power) when NVIDIA driver present.
- Display: GPU model(s), driver version, driver date, driver-age hint.
- **No driver download/install.** Optional manual link to vendor page only.

**Acceptance:** panel shows GPU model + driver version matching Device Manager, vendor-agnostic.

### 5.5 Real-time system monitor (`FR-MONITOR`) — minimal view
Glanceable live stats. Sampled in Rust, pushed to UI via Tauri events. Sampling runs **only while the monitor view is visible and the window is focused** — paused otherwise (idle CPU ≈ 0).

Metrics (system-wide aggregates; optionally the launched game's process):
- **Memory:** used / total, % (`sysinfo`).
- **Network:** up/down bytes/s = delta of interface rx/tx per interval (`sysinfo`).
- **Disk:** read/write bytes/s; per-drive busy if cheap (`sysinfo` disk usage deltas; PDH `PhysicalDisk` optional).
- **GPU:**
  - Utilization %: Windows **PDH** counter `\GPU Engine(*)\Utilization Percentage`, sum/aggregate the `engtype_3D` (and optionally Compute/Copy) instances. Cross-vendor (AMD/NVIDIA/Intel), same source as Task Manager. **Requires WDDM 2.0+.**
  - VRAM used/budget: DXGI `IDXGIAdapter3::QueryVideoMemoryInfo` (cross-vendor), or NVML on NVIDIA.
  - Temp/power: NVML (NVIDIA only); AMD via vendor lib (skip v1).
- CPU % optional (`sysinfo`) — cheap, likely include.

Design: single background sampler task @ ~1Hz (configurable), one reused `System` instance with `refresh_specifics` (refresh only what's needed — **never full process refresh every tick**), emits one small struct per tick. Sparkline + number per metric. No history persistence in v1.

**Is it doable + lightweight on Windows?** Yes. All metrics come from cheap OS APIs (`sysinfo`, PDH, DXGI, NVML). 1Hz aggregate sampling is well under 1% CPU. Cost only appears if you full-refresh all processes or sample faster than needed — both avoided above.

**Acceptance:** open monitor → RAM/disk/net/GPU update ~1s with values matching Task Manager (±sampling), CPU overhead negligible, sampling stops when view hidden.

**Footguns:**
- **[MED] NVIDIA `RmProfilingAdminOnly`:** GPU PDH counters can be admin-gated on some NVIDIA driver configs → non-admin reads return nothing. Detect empty counters → show "GPU util unavailable (driver counter access restricted)" + fall back to NVML if NVIDIA.
- **[LOW] WDDM < 2.0 / old drivers:** no GPU engine counters → degrade gracefully.
- **[LOW] Laptop hybrid GPU:** iGPU + dGPU both enumerate; label clearly, don't sum across physical adapters blindly.

### 5.6 Cover art (`FR-ART`) — offline-first
No store browse, so art comes from local sources:

1. **Steam (free, local, no network):** read from `<Steam>\appcache\librarycache\`. Look for both layouts (client version-dependent):
   - flat: `<appid>_library_600x900.jpg`, `<appid>_header.jpg`, `<appid>_logo.png`, `<appid>_icon.jpg`
   - per-appid subfolder: `librarycache\<appid>\library_600x900.jpg` etc.
   Prefer `library_600x900` (portrait cover) → fallback `header` → `icon`.
   **Caveat [LOW]:** cache only populated after Steam has rendered that game's library entry; a freshly installed, never-viewed game may lack art → fall back to icon/placeholder.
2. **Local games:** extract icon from `LaunchExecutable`/`DisplayIcon` via Windows `SHGetFileInfo`/`ExtractIconEx`, convert to PNG (`image` crate). Low-res but offline + always available.
3. **Epic:** manifests rarely carry usable art. Best-effort: check Epic launcher webcache; otherwise exe-icon fallback. Accept weaker Epic art in v1.
4. **Optional, opt-in, OFF by default — `SteamGridDB`:** community art DB with a real API (free key). This is the *only* network feature, fully optional, behind an explicit toggle; covers Epic/local titles with proper covers. **Default build stays 100% offline** (memory: zero-network core preserved). Flag clearly if enabled.

**Acceptance:** Steam titles show proper portrait covers from local cache with no network; local titles show exe icons; missing art shows a clean placeholder. Optional toggle can pull richer art if user opts in.

### 5.7 Library UX (`FR-UI`)
Grid + list, cover art, source badge, filters (source/drive/installed), sort (name/size/last-played), local text search over installed library, favorites/tags (local), light/dark theme.

---

## 6. Architecture

### Stack
- **Shell:** Tauri 2.x (Rust core + system webview). Small binary, native FS/registry/WMI/PDH access.
- **Core (Rust):** all scanning, parsing, disk, GPU, monitor, persistence. Exposed via Tauri commands + events.
- **Frontend:** **Lit 3 + Vite + TypeScript** (vanilla web components; no React — Preact only if a component truly needs it). Tiny bundle.
- **DB:** local **SQLite** via `rusqlite` (single file in app data). No server, no cloud.

### Rust crates
| Concern | Crate |
|---|---|
| Shell/IPC/events | `tauri` |
| Async | `tokio` |
| Serde | `serde`, `serde_json` |
| VDF (acf/libraryfolders) | `keyvalues-parser` (verify maintenance) |
| Registry | `winreg` |
| WMI (GPU model/driver) | `wmi` |
| Win32 PDH (GPU/disk counters) + DXGI (VRAM) + icon extraction | `windows` |
| NVIDIA enrich (optional) | `nvml-wrapper` (feature-gated) |
| Sys metrics (mem/net/disk/cpu) | `sysinfo` |
| Fast dir size | `jwalk` |
| Icon → PNG | `image` |
| DB | `rusqlite` (bundled sqlite) |
| HTTP (only if SteamGridDB opt-in) | `reqwest` (feature-gated, OFF by default) |
| Logging | `tracing` |

### Module layout (Rust)
```
src-tauri/src/
  main.rs            # tauri setup, command + event registration
  commands.rs        # #[tauri::command] surface
  scan/
    mod.rs           # orchestrate + dedup + persist
    steam.rs         # registry + libraryfolders.vdf + appmanifest acf
    epic.rs          # manifests/*.item parsing (installed only)
    local.rs         # uninstall registry + folder scan
  launch.rs          # protocol/exe launch + last_played
  disk.rs            # drive stats + jwalk per-game sizing (reparse-guarded)
  gpu.rs             # wmi model/driver + optional nvml
  monitor/
    mod.rs           # sampler task, ~1Hz, emits event; pause on hidden/unfocused
    metrics.rs       # sysinfo mem/net/disk/cpu deltas
    gpu_counters.rs  # PDH \GPU Engine util + DXGI QueryVideoMemoryInfo
  art/
    mod.rs
    steam_cache.rs   # librarycache lookup (flat + subfolder layouts)
    exe_icon.rs      # SHGetFileInfo/ExtractIconEx -> PNG
    gridindb.rs      # OPTIONAL, feature-gated, OFF by default
  db.rs              # rusqlite schema + queries
  models.rs          # Game, Drive, Gpu, Sample ...
```

### Data model (core)
```
Game   { id, source(steam|epic|local), external_id, name, install_path,
         exe_path?, size_bytes?, drive, last_played?, launch_count,
         cover_path?, tags[], favorite }
Drive  { letter, total_bytes, free_bytes }
Gpu    { name, vendor, driver_version, driver_date, vram_bytes? }
Sample { ts, mem_used, mem_total, net_up_bps, net_down_bps,
         disk_read_bps, disk_write_bps, gpu_util_pct?, vram_used?, vram_total?, cpu_pct } # live only, not persisted
```

---

## 7. Security / footgun summary

| # | Item | Severity | Mitigation |
|---|---|---|---|
| 1 | NVIDIA `RmProfilingAdminOnly` blocks non-admin GPU PDH counters | **MED** | Detect empty counters → message + NVML fallback (NVIDIA) |
| 2 | Disk walker on junctions/reparse points → double-count/loop | **LOW** | Skip reparse points; track visited paths |
| 3 | Launching arbitrary local `.exe` | **LOW** | User-confirmed paths only; no auto-exec on scan; validate path |
| 4 | Steam art cache empty for never-viewed installs | **LOW** | Fallback icon/placeholder |
| 5 | WDDM <2.0 / old drivers → no GPU engine counters | **LOW** | Degrade gracefully; hide GPU util |
| 6 | SteamGridDB opt-in introduces network + 3rd-party key | **LOW** | OFF by default; explicit toggle; key in OS keychain (`keyring`); never in logs |
| 7 | Trademark: "Steam"/"Epic" names/logos in shipped app | **LOW–MED** | Open-source, non-commercial, official-launch-mechanism only, clear disclaimer (mirror Playnite/Heroic) |

**Riskiest assumption:** GPU utilization counters are readable without elevation across the target hardware mix. If NVIDIA gating bites, util degrades to NVML (NVIDIA-only) and AMD/Intel may show VRAM-only. Core launcher + disk + memory/net monitor unaffected.

With store/login removed, the prior HIGH (Epic private GraphQL) and MED (Steam undocumented store ToS) risks are **eliminated** — there are no remote calls in the default build.

---

## 8. Open questions
1. Cover art: ship SteamGridDB opt-in (one optional network feature) or keep build strictly zero-network and accept weaker Epic/local art?
2. Monitor: include per-process (launched game) stats in v1, or system-wide only? (Per-process GPU attribution via PDH is fiddly — likely M4.)
3. Sampling interval default — 1s vs 2s? Expose as setting?
4. Playtime tracking (process poll) — v1 or M4?
5. App name + branding (avoid trademark).

---

## 9. Out of scope (explicit)
Login/accounts, store browse/search/sales/pricing, purchases, downloads/installs for Steam/Epic, file deletion by us, cloud, telemetry, ads, CI/CD, anti-cheat interaction, non-Windows platforms. Network in default build = none (SteamGridDB is opt-in only).
