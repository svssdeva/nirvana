//! Tauri command surface (the IPC boundary). Every command returns
//! `Result<T, AppError>`; core fns return `CoreError` and convert via `?`
//! (`From<CoreError> for AppError`). Full catalog + async/State rules:
//! `docs/api-contract.md`.

use crate::art::{self, CoverRef};
use crate::error::{AppError, CoreError, CoreResult};
use crate::library::{self, LibraryQuery};
use crate::models::{Drive, Game, Gpu, Source};
use crate::scan::{ScanDone, ScanEvents, ScanProgress};
use crate::state::AppState;
use crate::{disk, launch, monitor, scan};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

/// Result type every `#[tauri::command]` returns.
pub type CommandResult<T> = Result<T, AppError>;

/// Sample command exercising the error boundary (replaces the scaffold `greet`).
/// `fail = true` surfaces a [`CoreError`] as a serialized [`AppError`].
#[tauri::command]
pub fn ping(fail: bool) -> CommandResult<String> {
    if fail {
        // A core error propagates and converts to AppError via `?`/`.into()`.
        return Err(CoreError::Unsupported("ping failed on request".into()).into());
    }
    Ok("pong".into())
}

/// Forwards [`ScanEvents`] to the frontend over Tauri's event bus. Emit errors
/// (e.g. no listener attached) are non-fatal — they must not abort a scan.
struct TauriScanEvents {
    app: AppHandle,
}

impl ScanEvents for TauriScanEvents {
    fn progress(&self, progress: ScanProgress) {
        let _ = self.app.emit("scan://progress", progress);
    }
    fn done(&self, done: ScanDone) {
        let _ = self.app.emit("scan://done", done);
    }
}

/// Discover installed games across all sources, dedup, persist, and return the
/// stored library. Emits `scan://progress` per source and `scan://done` when
/// finished (`docs/api-contract.md`).
///
/// Synchronous: Tauri runs sync commands off the UI thread, so the window stays
/// responsive while sources scan concurrently (see [`scan_all_sources`]) and the
/// caller receives the deduped `Game[]` directly. (The api-contract's streaming
/// `ScanHandle` + cooperative cancellation is a later enhancement; not needed for
/// the v1 one-shot scan.)
#[tauri::command]
pub fn scan_library(
    app: AppHandle,
    state: State<'_, AppState>,
    full: bool,
) -> CommandResult<Vec<Game>> {
    tracing::info!(full, "library scan starting");
    // Read user-configured watch folders before scanning (released immediately).
    let watch_folders = {
        let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
        read_watch_folders(&db)
    };
    let results = scan_all_sources(watch_folders);
    let events = TauriScanEvents { app };
    // Don't hold the lock across the (blocking) scan above — only for persistence.
    // Recover from a poisoned lock: a panic in another command shouldn't brick
    // scanning, and our DB writes are individually transactional.
    let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
    let games = scan::merge_and_persist(&db, results, &events)?;
    tracing::info!(count = games.len(), "library scan complete");
    Ok(games)
}

/// Watch folders for the local scanner, from the `watchFolders` setting (a JSON
/// array of paths). Empty when unset or malformed. (Configuring them is a v1 UI
/// task — plan Task 22; the data path is wired here.)
fn read_watch_folders(db: &crate::db::Db) -> Vec<std::path::PathBuf> {
    match db.get_setting("watchFolders") {
        Ok(Some(json)) => serde_json::from_str::<Vec<String>>(&json)
            .map(|paths| paths.into_iter().map(std::path::PathBuf::from).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Return the persisted library (no scan), filtered/sorted/searched per `query`
/// (none → all, name-sorted). Sync because async Tauri commands can't borrow
/// `State`; the read is a single fast query under the lock.
#[tauri::command]
pub fn get_library(
    state: State<'_, AppState>,
    query: Option<LibraryQuery>,
) -> CommandResult<Vec<Game>> {
    let games = {
        let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
        db.list_games()?
    };
    Ok(library::apply_query(games, &query.unwrap_or_default()))
}

/// Wipe the entire local database (games, tags, settings) — the Settings
/// "delete database" action. Destructive; the UI confirms first.
#[tauri::command]
pub fn reset_database(state: State<'_, AppState>) -> CommandResult<()> {
    let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
    db.reset()?;
    Ok(())
}

/// Toggle a game's favorite flag (persisted).
#[tauri::command]
pub fn set_favorite(state: State<'_, AppState>, id: i64, favorite: bool) -> CommandResult<()> {
    let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
    db.set_favorite(id, favorite)?;
    Ok(())
}

/// Replace a game's tags (persisted; deduped + trimmed in the DB layer).
#[tauri::command]
pub fn set_tags(state: State<'_, AppState>, id: i64, tags: Vec<String>) -> CommandResult<()> {
    let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
    db.set_tags(id, &tags)?;
    Ok(())
}

/// Launch a game via its official mechanism and record the launch (FR-LAUNCH,
/// threat-model TB3). v1 supports Steam (`steam://rungameid/<appid>` opened via
/// `tauri-plugin-opener` — never spawning the Steam binary); Epic/local land in
/// Tasks 12/13. On success, stamps `last_played` + bumps `launch_count`.
#[tauri::command]
pub fn launch_game(app: AppHandle, state: State<'_, AppState>, id: i64) -> CommandResult<()> {
    let game = {
        let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
        db.get_game(id)?
            .ok_or_else(|| CoreError::NotFound(format!("game {id}")))?
    };

    match game.source {
        Source::Steam => {
            let url = launch::steam_launch_url(&game.external_id)?;
            // Open via the official protocol; never spawn the store binary (TB3).
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))?;
        }
        Source::Epic => {
            let url = launch::epic_launch_url(&game.external_id)?;
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))?;
        }
        Source::Local => launch_local(&game)?,
        // Hybrid GOG launch (goggalaxy:// or validated exe) lands in a later task.
        Source::Gog => {
            return Err(CoreError::Unsupported("gog launch not yet implemented".into()).into())
        }
    }

    let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
    db.record_launch(id, now_unix())?;
    Ok(())
}

/// Spawn a local game's exe via argv (no shell), after validating the path stays
/// under its install root, with cwd = the exe's directory (threat-model TB3).
fn launch_local(game: &Game) -> CoreResult<()> {
    let exe = game
        .exe_path
        .as_deref()
        .ok_or_else(|| CoreError::Unsupported(format!("{} has no executable", game.name)))?;
    let validated = launch::validate_local_exe(
        std::path::Path::new(exe),
        std::path::Path::new(&game.install_path),
    )?;
    let cwd = validated
        .parent()
        .ok_or_else(|| CoreError::Unsupported("executable has no parent directory".into()))?;
    std::process::Command::new(&validated)
        .current_dir(cwd)
        .spawn()
        .map_err(CoreError::Io)?;
    Ok(())
}

/// Donation details for the Settings "Support Nirvana" section: the UPI ID + an
/// offline-generated UPI QR (SVG). No network, no state.
#[tauri::command]
pub fn get_donation_info() -> CommandResult<crate::donation::DonationInfo> {
    Ok(crate::donation::donation_info()?)
}

/// List video adapters with model + driver (WMI). VRAM (DXGI) is added in
/// Task 18. Empty on non-Windows builds.
#[tauri::command]
pub fn get_gpus() -> CommandResult<Vec<Gpu>> {
    #[cfg(windows)]
    {
        Ok(crate::gpu::get_gpus(&crate::os::wmi::WindowsWmi)?)
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

/// Start (or restart) the system-monitor sampler: emits `monitor://sample` every
/// `interval_ms` (default 1000, clamped 250–5000). Idempotent — called on Monitor
/// view mount + window focus.
#[tauri::command]
pub fn monitor_start(
    app: AppHandle,
    state: State<'_, AppState>,
    interval_ms: Option<u64>,
) -> CommandResult<()> {
    let interval = interval_ms.unwrap_or(1000).clamp(250, 5000);
    state.monitor.set(monitor::spawn_sampler(app, interval));
    Ok(())
}

/// Stop the sampler (aborts the task → idle CPU ≈ 0). Called on view unmount +
/// window blur/hide. Idempotent.
#[tauri::command]
pub fn monitor_stop(state: State<'_, AppState>) -> CommandResult<()> {
    state.monitor.stop();
    Ok(())
}

/// Persisted preferences (the `setting` table). Theme lives in localStorage
/// (instant, pre-DB) — these are the DB-backed ones.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    monitor_interval_ms: u64,
    watch_folders: Vec<String>,
    steamgriddb_enabled: bool,
}

/// Keys `set_setting` accepts — validated at the boundary (TB2).
const KNOWN_SETTINGS: &[&str] = &[
    "monitorIntervalMs",
    "watchFolders",
    "steamgriddbEnabled",
    "theme",
];

/// Read all DB-backed settings with defaults.
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> CommandResult<Settings> {
    let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
    let monitor_interval_ms = db
        .get_setting("monitorIntervalMs")?
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1000)
        .clamp(250, 5000);
    let watch_folders = db
        .get_setting("watchFolders")?
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default();
    let steamgriddb_enabled = db.get_setting("steamgriddbEnabled")?.as_deref() == Some("true");
    Ok(Settings {
        monitor_interval_ms,
        watch_folders,
        steamgriddb_enabled,
    })
}

/// Persist a single setting. Rejects unknown keys (TB2 boundary validation).
#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> CommandResult<()> {
    if !KNOWN_SETTINGS.contains(&key.as_str()) {
        return Err(CoreError::Unsupported(format!("unknown setting: {key}")).into());
    }
    let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
    db.set_setting(&key, &value)?;
    Ok(())
}

/// Static system info for the monitor's "System" panel + the About section.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    os_name: String,
    os_version: String,
    kernel_version: String,
    hostname: String,
    cpu: String,
    cpu_threads: usize,
    mem_total_bytes: i64,
}

/// Read host OS/CPU/memory info (sysinfo). One-shot; not a live metric.
#[tauri::command]
pub fn get_system_info() -> CommandResult<SystemInfo> {
    let sys = sysinfo::System::new_all();
    let cpu = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_default();
    Ok(SystemInfo {
        os_name: sysinfo::System::name().unwrap_or_default(),
        os_version: sysinfo::System::os_version().unwrap_or_default(),
        kernel_version: sysinfo::System::kernel_version().unwrap_or_default(),
        hostname: sysinfo::System::host_name().unwrap_or_default(),
        cpu,
        cpu_threads: sys.cpus().len(),
        mem_total_bytes: i64::try_from(sys.total_memory()).unwrap_or(i64::MAX),
    })
}

/// Open the Windows Task Manager (convenience from the monitor page). Launches
/// `taskmgr.exe` directly — Nirvana never manages processes itself.
#[tauri::command]
pub fn open_task_manager() -> CommandResult<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("taskmgr.exe")
            .spawn()
            .map_err(CoreError::Io)?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(CoreError::Unsupported("Task Manager is Windows-only".into()).into())
    }
}

/// `size://progress` payload: a game's freshly computed on-disk size.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SizeProgress {
    id: i64,
    size_bytes: i64,
}

/// List storage volumes with capacity (sysinfo). Live; never cached.
#[tauri::command]
pub fn list_drives() -> CommandResult<Vec<Drive>> {
    Ok(disk::list_drives())
}

/// Compute accurate on-disk sizes for the given games (or all when `ids` is
/// `None`), persist each, and stream `size://progress {id, sizeBytes}` so the UI
/// updates live while manifest sizes stand in. Sync (borrows `State`), runs off
/// the UI thread; sizes sequentially — fine for v1. A per-game persist failure is
/// logged and skipped, not fatal.
#[tauri::command]
pub fn compute_game_sizes(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Option<Vec<i64>>,
) -> CommandResult<()> {
    let games = {
        let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
        db.list_games()?
    };
    let targets: Vec<Game> = match ids {
        Some(ids) => games.into_iter().filter(|g| ids.contains(&g.id)).collect(),
        None => games,
    };
    for game in targets {
        let bytes = i64::try_from(compute_install_size(&game.install_path)).unwrap_or(i64::MAX);
        {
            let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
            if let Err(e) = db.set_size_bytes(game.id, bytes) {
                tracing::warn!(id = game.id, error = %e, "persisting size failed; skipping");
                continue;
            }
        }
        let _ = app.emit(
            "size://progress",
            SizeProgress {
                id: game.id,
                size_bytes: bytes,
            },
        );
    }
    Ok(())
}

/// On-disk size of an install path. Windows-only (uses the real FS adapter);
/// other targets report 0 (the app ships on Windows).
#[cfg(windows)]
fn compute_install_size(install_path: &str) -> u64 {
    disk::dir_size(
        &crate::os::fs::WindowsFs,
        std::path::Path::new(install_path),
    )
}
#[cfg(not(windows))]
fn compute_install_size(_install_path: &str) -> u64 {
    0
}

/// Let the user pick a local image as a game's custom cover (offline). Opens a
/// native file picker, validates it's a reasonably-sized image, copies it into
/// the app cache (`covers/`, already in the asset scope), and persists the path.
/// Returns the new cover path, or `None` if the user cancelled.
#[tauri::command]
pub fn set_cover(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> CommandResult<Option<String>> {
    use tauri::Manager;

    // Game must exist before we bother picking a file.
    {
        let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
        db.get_game(id)?
            .ok_or_else(|| CoreError::NotFound(format!("game {id}")))?;
    }

    let picked = app
        .dialog()
        .file()
        .add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "gif"])
        .blocking_pick_file();
    let Some(picked) = picked else {
        return Ok(None); // user cancelled
    };
    let src = picked
        .into_path()
        .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))?;

    // Validate (TB1): real image extension + bounded size.
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif"))
        .ok_or_else(|| CoreError::Unsupported("not an image file".into()))?;
    if std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0) > 64 * 1024 * 1024 {
        return Err(CoreError::Unsupported("image too large (>64 MB)".into()).into());
    }

    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))?
        .join("covers");
    std::fs::create_dir_all(&cache).map_err(CoreError::Io)?;
    let dest = cache.join(format!("custom-{id}.{ext}"));
    std::fs::copy(&src, &dest).map_err(CoreError::Io)?;
    let dest_str = dest.to_string_lossy().into_owned();

    {
        let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
        db.set_cover_path(id, &dest_str)?;
    }
    Ok(Some(dest_str))
}

/// Open a game's install folder in the OS file manager (via `tauri-plugin-opener`).
#[tauri::command]
pub fn open_install_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> CommandResult<()> {
    let install_path = {
        let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
        db.get_game(id)?
            .ok_or_else(|| CoreError::NotFound(format!("game {id}")))?
            .install_path
    };
    app.opener()
        .open_path(install_path, None::<&str>)
        .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))?;
    Ok(())
}

/// Open the store's own uninstall flow (we never delete files; the store handles
/// it and the user confirms — threat-model "no deletion by us"). Steam only for
/// now; other sources return `Unsupported` (uninstall via Windows Settings).
#[tauri::command]
pub fn uninstall_game(app: AppHandle, state: State<'_, AppState>, id: i64) -> CommandResult<()> {
    let game = {
        let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
        db.get_game(id)?
            .ok_or_else(|| CoreError::NotFound(format!("game {id}")))?
    };
    let url = match game.source {
        Source::Steam => launch::steam_uninstall_url(&game.external_id)?,
        other => {
            return Err(CoreError::Unsupported(format!(
                "no store uninstall for {} games — use Windows Settings",
                other.as_str()
            ))
            .into())
        }
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))?;
    Ok(())
}

/// Current Unix time in seconds (0 if the clock is before the epoch).
fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Resolve a game's cover art (offline-first: Steam cache → exe icon →
/// placeholder; `docs/api-contract.md`). Lazy, per-tile. Never errors on a
/// missing cover — returns the placeholder variant. Sync (borrows `State`).
#[tauri::command]
pub fn get_cover(app: AppHandle, state: State<'_, AppState>, id: i64) -> CommandResult<CoverRef> {
    let game = {
        let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
        db.get_game(id)?
    };
    let Some(game) = game else {
        return Ok(CoverRef::Placeholder);
    };
    let cover = resolve_cover_for(&app, &game);
    // Offline-first: only reach for the opt-in network source (if built + enabled)
    // when nothing local was found.
    if matches!(cover, CoverRef::Placeholder) {
        if let Some(enriched) = steamgriddb_cover(&app, &state, &game) {
            return Ok(enriched);
        }
    }
    Ok(cover)
}

/// SteamGridDB enrichment — compiled in only with the `steamgriddb` feature and
/// used only when the user has enabled it. `None` otherwise (offline default).
#[cfg(feature = "steamgriddb")]
fn steamgriddb_cover(
    app: &AppHandle,
    state: &State<'_, AppState>,
    game: &Game,
) -> Option<CoverRef> {
    use tauri::Manager;
    let enabled = {
        let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
        db.get_setting("steamgriddbEnabled")
            .ok()
            .flatten()
            .as_deref()
            == Some("true")
    };
    if !enabled {
        return None;
    }
    let cache = app.path().app_cache_dir().ok()?.join("covers");
    match crate::art::gridindb::fetch_cover(game, &cache) {
        Ok(Some(path)) => Some(CoverRef::Image {
            path: path.to_string_lossy().into_owned(),
        }),
        _ => None,
    }
}

#[cfg(not(feature = "steamgriddb"))]
fn steamgriddb_cover(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
    _game: &Game,
) -> Option<CoverRef> {
    None
}

/// DEV ONLY: seed ~50 varied dummy games (mixed sources, sizes, drives,
/// favorites, tags, last-played) for local UI testing. Compiled out of release
/// builds — returns `Unsupported` there. Idempotent (upserts on the dedup key).
#[tauri::command]
pub fn seed_dummy_games(state: State<'_, AppState>) -> CommandResult<usize> {
    #[cfg(debug_assertions)]
    {
        let db = state.db.lock().unwrap_or_else(|p| p.into_inner());
        let mut count = 0;
        for (game, tags) in dummy_games() {
            let id = db.upsert_game(&game)?;
            if !tags.is_empty() {
                db.set_tags(id, &tags)?;
            }
            count += 1;
        }
        Ok(count)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = state;
        Err(CoreError::Unsupported("seeding is dev-only".into()).into())
    }
}

/// 56 plausible games with varied attributes so every M4 surface (filters, sort,
/// search, favorites, tags, disk sizes) has something to show.
#[cfg(debug_assertions)]
fn dummy_games() -> Vec<(Game, Vec<String>)> {
    const NAMES: &[&str] = &[
        "Elden Ring",
        "Cyberpunk 2077",
        "The Witcher 3",
        "Hades",
        "Hollow Knight",
        "Stardew Valley",
        "Celeste",
        "Baldur's Gate 3",
        "Doom Eternal",
        "Portal 2",
        "Half-Life Alyx",
        "Dota 2",
        "Counter-Strike 2",
        "Team Fortress 2",
        "Disco Elysium",
        "Outer Wilds",
        "Subnautica",
        "Terraria",
        "Factorio",
        "RimWorld",
        "Slay the Spire",
        "Dead Cells",
        "Cuphead",
        "Ori and the Blind Forest",
        "Sekiro",
        "Dark Souls III",
        "Red Dead Redemption 2",
        "Grand Theft Auto V",
        "Forza Horizon 5",
        "Hitman 3",
        "Resident Evil 4",
        "Monster Hunter World",
        "Sea of Thieves",
        "Valheim",
        "Vampire Survivors",
        "Balatro",
        "Lethal Company",
        "Palworld",
        "Helldivers 2",
        "Satisfactory",
        "Deep Rock Galactic",
        "Risk of Rain 2",
        "Enter the Gungeon",
        "Katana ZERO",
        "Tunic",
        "Inscryption",
        "Pizza Tower",
        "Hi-Fi Rush",
        "Returnal",
        "Death Stranding",
        "God of War",
        "Spider-Man Remastered",
        "Horizon Zero Dawn",
        "Control",
        "Alan Wake 2",
        "Animal Well",
    ];
    const GENRES: &[&str] = &[
        "RPG",
        "Action",
        "Indie",
        "Roguelike",
        "Soulslike",
        "Co-op",
        "Strategy",
        "Metroidvania",
        "Shooter",
        "Cozy",
    ];
    let drives = ["C:", "D:", "E:"];

    NAMES
        .iter()
        .enumerate()
        .map(|(i, &name)| {
            let source = match i % 3 {
                0 => Source::Steam,
                1 => Source::Epic,
                _ => Source::Local,
            };
            let drive = drives[i % drives.len()];
            let slug: String = name
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == ' ')
                .collect();
            let install = format!(r"{drive}\Games\{slug}");
            let external_id = match source {
                Source::Steam => (100_000 + (i as u32) * 10).to_string(),
                Source::Epic => format!("Fake{}", slug.replace(' ', "")),
                Source::Local => format!(r"{install}\game.exe"),
                // Dummy sources only cycle Steam/Epic/Local (i % 3), so this is
                // unreachable in practice — present only for exhaustiveness.
                Source::Gog => (1_200_000_000 + (i as u32)).to_string(),
            };
            let exe_path = matches!(source, Source::Local).then(|| format!(r"{install}\game.exe"));
            // Every 9th has an unknown size; others range ~2–112 GB.
            let size_bytes = (i % 9 != 0).then(|| ((i as i64 % 56) + 1) * 2_000_000_000);
            let game = Game {
                id: 0,
                source,
                external_id,
                name: name.to_string(),
                install_path: install,
                exe_path,
                size_bytes,
                drive: Some(drive.to_string()),
                last_played: (i % 4 == 0).then(|| 1_700_000_000 + (i as i64) * 86_400),
                launch_count: (i % 5) as i64,
                cover_path: None,
                favorite: i % 7 == 0,
                tags: Vec::new(),
            };
            let tags = if i % 3 == 0 {
                vec![
                    GENRES[i % GENRES.len()].to_string(),
                    GENRES[(i + 4) % GENRES.len()].to_string(),
                ]
            } else {
                Vec::new()
            };
            (game, tags)
        })
        .collect()
}

/// Store the SteamGridDB API key in the OS vault. Only functional in a build
/// with the `steamgriddb` feature; otherwise returns `Unsupported`.
#[tauri::command]
pub fn set_steamgriddb_key(key: String) -> CommandResult<()> {
    #[cfg(feature = "steamgriddb")]
    {
        crate::art::gridindb::set_api_key(&key)?;
        Ok(())
    }
    #[cfg(not(feature = "steamgriddb"))]
    {
        let _ = key;
        Err(
            CoreError::Unsupported("steamgriddb feature is not enabled in this build".into())
                .into(),
        )
    }
}

/// Resolve a cover using the real OS adapters. Windows-only; other targets have
/// no adapters and always yield a placeholder (the app ships on Windows).
#[cfg(windows)]
fn resolve_cover_for(app: &AppHandle, game: &Game) -> CoverRef {
    use crate::os::fs::WindowsFs;
    use crate::os::icon::WindowsIcons;
    use crate::os::registry::WindowsRegistry;
    use crate::scan::steam::find_steam_root;
    use std::path::PathBuf;
    use tauri::Manager;

    let steam_root = find_steam_root(&WindowsRegistry)
        .ok()
        .flatten()
        .map(PathBuf::from);
    let icon_cache = app
        .path()
        .app_cache_dir()
        .map(|d| d.join("icons"))
        .unwrap_or_else(|_| PathBuf::from("icons"));
    art::resolve_cover(
        game,
        &WindowsFs,
        steam_root.as_deref(),
        &WindowsIcons,
        &icon_cache,
    )
}

#[cfg(not(windows))]
fn resolve_cover_for(_app: &AppHandle, _game: &Game) -> CoverRef {
    CoverRef::Placeholder
}

/// Run every source scanner concurrently and pair each with its result. Each
/// source scans on its own scoped thread, sharing the zero-sized, `Sync` OS
/// adapters by reference; `thread::scope` guarantees all joins before the borrows
/// end. A panicked scanner thread degrades to an error for that source only.
#[cfg(windows)]
fn scan_all_sources(
    watch_folders: Vec<std::path::PathBuf>,
) -> Vec<(Source, CoreResult<Vec<Game>>)> {
    use crate::os::fs::WindowsFs;
    use crate::os::registry::WindowsRegistry;
    use crate::scan::{epic::EpicScanner, local::LocalScanner, steam::SteamScanner};

    let reg = WindowsRegistry;
    let fs = WindowsFs;
    let epic_dir = epic_manifests_dir();

    std::thread::scope(|s| {
        let steam = s.spawn(|| SteamScanner::new(&reg, &fs).scan());
        let epic = s.spawn(|| EpicScanner::new(&fs).scan(&epic_dir));
        let local = s.spawn(|| LocalScanner::new(&fs).scan(&watch_folders));
        vec![
            (
                Source::Steam,
                steam
                    .join()
                    .unwrap_or_else(|_| Err(thread_panicked("steam"))),
            ),
            (
                Source::Epic,
                epic.join().unwrap_or_else(|_| Err(thread_panicked("epic"))),
            ),
            (
                Source::Local,
                local
                    .join()
                    .unwrap_or_else(|_| Err(thread_panicked("local"))),
            ),
        ]
    })
}

/// Error for a scanner thread that panicked (should not happen — scanners don't
/// unwrap on data — but degrade that one source rather than abort the scan).
#[cfg(windows)]
fn thread_panicked(source: &str) -> CoreError {
    CoreError::Unsupported(format!("{source} scan thread panicked"))
}

/// Epic's manifests directory: `%PROGRAMDATA%\Epic\EpicGamesLauncher\Data\Manifests`.
#[cfg(windows)]
fn epic_manifests_dir() -> std::path::PathBuf {
    let program_data = std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".into());
    std::path::PathBuf::from(program_data)
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data")
        .join("Manifests")
}

/// Non-Windows builds have no OS adapters, so nothing is discovered. Keeps the
/// crate compiling off-Windows (CI type-checks); the app only ships on Windows.
#[cfg(not(windows))]
fn scan_all_sources(
    _watch_folders: Vec<std::path::PathBuf>,
) -> Vec<(Source, CoreResult<Vec<Game>>)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    #[test]
    fn ping_ok_returns_pong() {
        assert_eq!(ping(false).unwrap(), "pong");
    }

    #[test]
    fn ping_failure_surfaces_apperror_unsupported() {
        let err = ping(true).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Unsupported);
    }
}
