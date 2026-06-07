//! Shared, thread-safe application state managed by Tauri (`.manage(..)`).

use crate::db::Db;
use std::sync::{Arc, Mutex};
use tauri::async_runtime::JoinHandle;

/// State handed to commands via `tauri::State<'_, AppState>`.
pub struct AppState {
    /// The library database. Wrapped in `Mutex` because rusqlite's `Connection`
    /// is `Send` but not `Sync`; scans/queries are infrequent and short, so a
    /// single lock is simpler than a pool with negligible contention cost.
    /// `Arc` so a sync command can clone a handle out for background work
    /// (api-contract "State access" rule).
    pub db: Arc<Mutex<Db>>,
    /// Owns the running monitor-sampler task (Task 17).
    pub monitor: Monitor,
}

impl AppState {
    pub fn new(db: Db) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            monitor: Monitor::default(),
        }
    }
}

/// Holds the single monitor-sampler task. Starting replaces (aborts) any prior
/// task; stopping aborts it — so a stopped monitor consumes no CPU.
#[derive(Default)]
pub struct Monitor {
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Monitor {
    fn guard(&self) -> std::sync::MutexGuard<'_, Option<JoinHandle<()>>> {
        self.handle.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Install a new sampler task, aborting any existing one (idempotent restart).
    pub fn set(&self, handle: JoinHandle<()>) {
        let mut current = self.guard();
        if let Some(old) = current.take() {
            old.abort();
        }
        *current = Some(handle);
    }

    /// Abort the running sampler task, if any.
    pub fn stop(&self) {
        if let Some(handle) = self.guard().take() {
            handle.abort();
        }
    }
}
