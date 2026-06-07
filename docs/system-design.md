# Atrium — System Design

> Output of the `system-design` engineering skill. Authoritative architecture for
> v1 (M1–M4). Pairs with `docs/PRD-game-launcher.md` (what/why) and
> `docs/plan/v1-implementation-plan.md` (how/when). UI is governed by
> `docs/design.md` (binding). No code until this is approved.

## 1. Requirements

### Functional (from PRD §5)
- **FR-SCAN** one-click discovery of installed Steam/Epic/local games → dedup → persist.
- **FR-LAUNCH** launch via official mechanism; record `last_played`/`launch_count`.
- **FR-DISK** per-drive total/used/free; per-game on-disk size; biggest-games view.
- **FR-GPU** GPU model/driver/date via WMI; optional NVML enrich.
- **FR-MONITOR** ~1Hz system-wide RAM/net/disk/GPU(/CPU) sampling; paused when hidden/unfocused.
- **FR-ART** offline-first cover art (Steam cache → exe icon → placeholder); optional SteamGridDB.
- **FR-UI** grid/list, filters, sort, local search, favorites/tags, light/dark theme.

### Non-functional
- **Binary < 50MB**, **cold start < 1s**, **idle CPU ≈ 0** (sampler paused when not viewed).
- **Zero network in default build.** Network only via opt-in Cargo feature.
- Single user, single machine, no concurrency beyond in-process async. No availability/HA concerns.
- Sampling overhead < 1% CPU at 1Hz (no full process refresh per tick).

### Constraints
- Solo developer; Windows 11; toolchain: Rust (MSVC), Tauri 2, Lit 3 + Vite + TS, bun.
- WebView2 runtime present. MSVC C++ build tools required to build the Rust core.
- UI must conform to `docs/design.md` (PlayStation-style system).

## 2. High-Level Design

### Component diagram
```
┌──────────────────────────── Tauri Window (system WebView2) ────────────────────────────┐
│  Frontend (Lit 3 + Vite + TS)                                                           │
│  app-root ─ view switch (enum) ─┬─ library-view ─ game-grid ─ game-tile                 │
│                                 ├─ disk-view                                            │
│                                 ├─ monitor-view  (subscribes monitor://sample)          │
│                                 └─ settings-view                                        │
│  store (observable) + @lit/context DI ;  ipc.ts wraps @tauri-apps/api invoke/listen     │
└───────────────▲───────────────────────────────────────────────────────┬───────────────┘
        commands │ (Result<T,AppError>)                          events   │ (scan://progress, monitor://sample)
┌───────────────┴───────────────────────────────────────────────────────▼───────────────┐
│  Rust core (src-tauri)                                                                  │
│  commands.rs  ── thin #[tauri::command] surface, maps core Result → AppError            │
│      ├── scan/      orchestrate → steam.rs | epic.rs | local.rs  → dedup → persist       │
│      ├── launch.rs  protocol/exe launch + last_played                                   │
│      ├── disk.rs    drive stats + jwalk per-game sizing (reparse-guarded)               │
│      ├── gpu.rs      WMI model/driver (+ nvml feature)                                   │
│      ├── monitor/   sampler task @1Hz, pause gate; metrics.rs + gpu_counters.rs         │
│      ├── art/        steam_cache.rs | exe_icon.rs | gridindb.rs (feature)               │
│      └── db.rs       rusqlite schema + queries + migrations                             │
│  os/ (trait seams): Registry, FileSystem, Wmi, Pdh  →  real impls + test fakes          │
└─────────────────────────────────────────────────────────────────────────────────────────┘
```

### Data flow — Scan (representative)
```
UI: invoke("scan_library", {full})
  → commands::scan_library
    → scan::run(os): steam.collect() ‖ epic.collect() ‖ local.collect()   (join via tokio)
      each emits scan://progress {source, found, done}
    → dedup(normalized path + name)  → db.upsert_games()
  → returns Vec<Game>  (UI renders grid; art resolved lazily per tile via get_cover)
```

### Data flow — Monitor
```
monitor-view mounts → invoke("monitor_start")
  → spawn sampler task (if not running): tokio::time::interval(cfg.interval)
      reuse one sysinfo::System; refresh_specifics(only mem/net/disk/cpu)
      read PDH GPU util + DXGI VRAM (best-effort; degrade on empty)
      emit monitor://sample {Sample}
  view hidden / window blur → invoke("monitor_stop") → pause gate halts ticks (idle CPU ≈ 0)
```

## 3. Deep Dive

### Data model (SQLite; see CONTEXT.md for terms)
```sql
-- user_version-based migrations (db.rs); bundled rusqlite
game(
  id INTEGER PK, source TEXT CHECK(source IN ('steam','epic','local')),
  external_id TEXT, name TEXT, name_norm TEXT,        -- name_norm for dedup/search
  install_path TEXT, install_path_norm TEXT, exe_path TEXT,
  size_bytes INTEGER, drive TEXT,
  last_played INTEGER, launch_count INTEGER DEFAULT 0,
  cover_path TEXT, favorite INTEGER DEFAULT 0,
  UNIQUE(install_path_norm, name_norm)                -- dedup key
);
tag(id PK, name UNIQUE);
game_tag(game_id, tag_id, PRIMARY KEY(game_id, tag_id));
setting(key TEXT PK, value TEXT);                     -- toggles, interval, theme, watch folders (JSON)
-- Drive, Gpu: queried live, not stored. Sample: live-only, never persisted.
```
Secrets (SteamGridDB API key) are NOT in SQLite — stored via `keyring` (OS credential vault).

### IPC contract (commands)
All commands are `async` and return `Result<T, AppError>`. Names are stable; UI depends on them.

| Command | Args | Returns | Notes |
|---|---|---|---|
| `scan_library` | `{ full: bool }` | `Vec<Game>` | emits `scan://progress` |
| `get_library` | `{ filter?, sort? }` | `Vec<Game>` | from DB, no scan |
| `launch_game` | `{ id }` | `()` | official mechanism only |
| `get_cover` | `{ id }` | `CoverRef` (path or placeholder) | lazy, offline-first |
| `list_drives` | — | `Vec<Drive>` | sysinfo |
| `compute_game_sizes` | `{ ids? }` | streamed via event or `Vec<GameSize>` | jwalk, reparse-guarded |
| `get_gpus` | — | `Vec<Gpu>` | WMI (+nvml) |
| `monitor_start` / `monitor_stop` | — | `()` | controls sampler gate |
| `get_settings` / `set_setting` | `{ key, value? }` | `Setting`/`()` | SQLite `setting` table |
| `set_favorite` / `set_tags` | `{ id, ... }` | `()` | library UX |
| `open_install_folder` | `{ id }` | `()` | via `tauri-plugin-opener` |

Events: `scan://progress`, `monitor://sample`, `size://progress`.

### Error handling
- Core fns return `Result<T, CoreError>` (`thiserror` enum: `Io`, `Parse`, `Registry`, `Db`,
  `Gpu`, `NotFound`, `Unsupported`, …).
- `AppError` is the serializable boundary type: `{ kind: string, message: string }`
  (`serde`). `commands.rs` maps `CoreError → AppError`. UI shows `message`, branches on `kind`.
- Degrade, don't crash: empty GPU counters → "GPU util unavailable"; missing art → placeholder;
  one source failing a scan never aborts the others (per-source results collected independently).

### Async & lifecycle
- Tokio (Tauri 2 default runtime). Scans fan out per source and `join`.
- **Tauri gotcha (verified):** async commands cannot borrow `State<'_,T>`. Managed
  state is `Arc<…>` via `.manage()`; a sync command clones the `Arc` out before
  async work. Full rules + command catalog in `docs/api-contract.md`.
  (Source: https://v2.tauri.app/develop/calling-rust/#async-commands)
- One sampler task; a shared pause gate (`tokio::sync::Notify` + `AtomicBool`/watch) toggled by
  `monitor_start/stop`, themselves driven by view mount/unmount + window focus/visibility events.
- jwalk for parallel directory sizing; **skip reparse points, track visited paths** (junction guard).

### Frontend architecture
- No router/framework: `app-root` holds a `view` enum; views are Lit elements lazy-imported.
- Small observable `store` for library/filter/settings; `@lit/context` to inject `ipc` + store.
- `ipc.ts` is the single seam over `@tauri-apps/api` (`invoke`, `listen`) — typed wrappers per command.
- All styling pulls tokens/components from `docs/design.md`. Each element owns `static styles`.

### Feature flags (Cargo)
- default = offline. `steamgriddb` (adds `reqwest`, network art, `keyring`), `nvml`
  (NVIDIA enrich). Default build compiles out all network + NVML.

### Security & hardening (full analysis: `docs/threat-model.md`)
- Set a **restrictive CSP** (scaffold ships `csp: null`); keep Tauri **capabilities
  least-privilege** (core:* only — no broad `fs`/`shell` exposed to JS; all
  privileged work stays in Rust commands); **validate exe paths** before launch and
  spawn via argv (no shell); **parameterize all SQL**; keep the SteamGridDB key in
  `keyring`, never in DB/logs. (Sources: https://v2.tauri.app/security/csp/ ,
  https://v2.tauri.app/security/capabilities/)
- **VRAM source:** use DXGI `QueryVideoMemoryInfo` / NVML, **not** WMI `AdapterRAM`
  (32-bit, caps at ~4GB). WMI is for model/driver/date only.

## 4. Scale & Reliability (desktop-appropriate)
- No multi-user/HA. "Reliability" = never block the UI thread, never crash on a bad manifest,
  never leak the sampler. Long scans/sizing run async with progress events + cancellation.
- Startup budget: defer DB open + first scan off the critical path; render shell immediately.
- Resilience: corrupt/locked manifest → skip with a logged warning, continue. DB migration on
  open is forward-only and idempotent.

## 5. Trade-off Analysis

| Decision | Chosen | Alternative | Why / cost |
|---|---|---|---|
| Persistence | Single SQLite (+keyring) | DB + config file | One transactional store; secrets still vaulted. ADR-0001. |
| Network | Offline default, feature-gated | Always-available SteamGridDB | Preserves zero-network guarantee; opt-in complexity. ADR-0002. |
| OS access | Trait seams + fakes | Direct Win32 calls | Testable parse/dedup on any OS; some boilerplate. ADR-0005. |
| Frontend | Lit + tiny store, no router | React / a router lib | Tiny bundle, matches design system; we hand-roll view switching. |
| VDF parsing | `keyvalues-parser`, hand-rolled fallback | Only hand-rolled | Reuse if maintained; risk-flagged. ADR-0003. |
| Monitor | System-wide v1 | Per-process | PDH per-process GPU is fiddly; deferred. |
| Sampler | One task, pause-gated | Always-on / per-metric tasks | Idle CPU ≈ 0 requirement. |

## 6. What I'd revisit as it grows
- **Per-process attribution** (playtime + per-game GPU) — needs a process watcher; M4+.
- **Cross-platform** — the `os/` trait seams are the port boundary; add Linux/mac impls later.
- **Art pipeline** — if SteamGridDB proves popular, add a local art cache table + eviction.
- **Search** — `name_norm LIKE` is fine at hundreds of games; revisit FTS5 only if needed.
- **Auto-update** — none in v1; if added, must not break the offline default.
