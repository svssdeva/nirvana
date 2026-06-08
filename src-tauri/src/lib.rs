pub mod art;
pub mod commands;
pub mod db;
pub mod disk;
pub mod donation;
pub mod error;
pub mod gpu;
pub mod launch;
pub mod library;
pub mod models;
pub mod monitor;
pub mod os;
pub mod scan;
pub mod state;

use crate::db::Db;
use crate::state::AppState;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // RUST_LOG-driven logs (default `info`). `try_init` is a no-op if a subscriber
    // is already installed, so this never panics.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Single SQLite store in the per-user app-data dir (ADR-0001). Debug
            // builds use a SEPARATE file so dev-only seeded dummy games (the
            // `seed_dummy_games` command) never pollute the DB a release build
            // reads — they share the app-data dir otherwise (same identifier).
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db_file = if cfg!(debug_assertions) {
                "nirvana-dev.db"
            } else {
                "nirvana.db"
            };
            let db = Db::open(&dir.join(db_file))?;
            app.manage(AppState::new(db));
            allow_cover_dirs(app);
            // Create the main window AFTER state is managed, never from config.
            // The config-defined window's webview starts executing JS (and can
            // invoke `get_library` etc.) concurrently with this setup hook — on a
            // cold start that races `app.manage()` above and fails command state
            // extraction ("state not managed for `get_library`"). Building it here
            // guarantees the webview only loads once `AppState` is in place. The
            // splash window (config-defined, calls no commands) covers the wait.
            build_main_window(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::scan_library,
            commands::get_library,
            commands::get_cover,
            commands::launch_game,
            commands::get_donation_info,
            commands::list_drives,
            commands::compute_game_sizes,
            commands::open_install_folder,
            commands::uninstall_game,
            commands::get_gpus,
            commands::monitor_start,
            commands::monitor_stop,
            commands::set_favorite,
            commands::set_tags,
            commands::get_settings,
            commands::set_setting,
            commands::set_steamgriddb_key,
            commands::seed_dummy_games,
            commands::get_system_info,
            commands::set_cover,
            commands::open_task_manager,
            commands::reset_database
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Build the main app window (hidden + borderless — we draw our own title bar).
/// Mirrors what used to live in `tauri.conf.json`'s `app.windows`, but created
/// here so it only spins up after `AppState` is managed (see the setup hook).
/// Capabilities still bind by the `"main"` label, so the window permissions
/// apply exactly as before.
fn build_main_window(app: &tauri::App) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("Nirvana")
        .inner_size(1280.0, 800.0)
        .min_inner_size(900.0, 600.0)
        .visible(false)
        .decorations(false)
        .build()?;
    Ok(())
}

/// Grant the asset protocol read access to the directories covers come from, so
/// the WebView can render them via `convertFileSrc` under the restrictive CSP
/// (`img-src` allows `asset:`). Least-privilege: only the Steam library-cache and
/// our own icon cache, nothing broader. Best-effort — a failure just means a
/// given cover won't load (the tile keeps its placeholder).
#[cfg(windows)]
fn allow_cover_dirs(app: &tauri::App) {
    use crate::os::registry::WindowsRegistry;
    use crate::scan::steam::find_steam_root;
    use std::path::Path;

    let scope = app.asset_protocol_scope();
    if let Ok(cache) = app.path().app_cache_dir() {
        // Our own cache (exe icons under icons/, SteamGridDB covers under covers/).
        let _ = scope.allow_directory(&cache, true);
    }
    if let Ok(Some(root)) = find_steam_root(&WindowsRegistry) {
        let _ = scope.allow_directory(Path::new(&root).join("appcache").join("librarycache"), true);
    }
}

#[cfg(not(windows))]
fn allow_cover_dirs(_app: &tauri::App) {}
