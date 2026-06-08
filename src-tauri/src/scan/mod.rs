//! Library discovery + orchestration (plan Task 8, M1).
//!
//! Per-source scanners (`steam`, later `epic`/`local`) discover installed games;
//! this module merges their results, **dedups** across sources (same normalized
//! install path + name), and **persists** the survivors via [`Db::upsert_game`].
//!
//! The orchestration core here is deliberately free of Tauri and OS specifics so
//! it is unit-tested on any OS: it takes already-scanned results plus a
//! [`ScanEvents`] sink. The command layer (`commands::scan_library`) supplies the
//! real scanners (concurrently, behind the `os` seams) and a sink that forwards
//! to Tauri's `scan://progress` / `scan://done` events (`docs/api-contract.md`).
//!
//! Robustness (threat-model TB1): a single source's failure is logged and skipped
//! (its games are simply absent) — it never aborts the whole scan; likewise an
//! individual row that fails to persist is skipped, not fatal.

pub mod epic;
pub mod local;
pub mod steam;
pub mod store;
pub mod vdf;

use crate::db::{self, Db};
use crate::error::{CoreError, CoreResult};
use crate::models::{Game, Source};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Component, Path, Prefix};

/// Drive prefix of an absolute Windows path, e.g. `"C:"`. `None` for paths
/// without a disk prefix (UNC, relative). Uppercased for stable dedup keys.
/// Shared by the per-source scanners.
pub(crate) fn drive_of(path: &Path) -> Option<String> {
    match path.components().next() {
        Some(Component::Prefix(p)) => match p.kind() {
            Prefix::Disk(b) | Prefix::VerbatimDisk(b) => {
                Some(format!("{}:", (b as char).to_ascii_uppercase()))
            }
            _ => None,
        },
        _ => None,
    }
}

/// Whether an error means "the file/key isn't there". Bridges the fake's
/// `NotFound` and the real `WindowsFs`'s `Io(ErrorKind::NotFound)` so a missing
/// directory degrades to "nothing found" on both. Shared by the scanners.
pub(crate) fn is_not_found(e: &CoreError) -> bool {
    match e {
        CoreError::NotFound(_) => true,
        CoreError::Io(io) => io.kind() == std::io::ErrorKind::NotFound,
        _ => false,
    }
}

/// `scan://progress` payload: one emitted per source as it finishes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub source: Source,
    /// Raw games this source discovered (pre-dedup).
    pub found: usize,
    /// Whether this source has finished (always true in the current coarse model).
    pub done: bool,
}

/// `scan://done` payload: emitted once when the whole scan is persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanDone {
    /// Number of deduped games persisted to the library.
    pub total: usize,
}

/// Sink for scan lifecycle events. The command wires this to Tauri `emit`; tests
/// record into a buffer. Keeps the Tauri dependency out of the orchestration core.
pub trait ScanEvents {
    fn progress(&self, progress: ScanProgress);
    fn done(&self, done: ScanDone);
}

/// Collapse games that normalize to the same `(install_path, name)` key — the
/// same key the DB enforces — keeping the highest-priority source per
/// [`store::source_rank`] (richer store metadata outranks a bare local-exe match).
/// First appearance determines output position (stable, scanner-order friendly).
pub fn dedup(games: Vec<Game>) -> Vec<Game> {
    let mut out: Vec<Game> = Vec::with_capacity(games.len());
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for game in games {
        let key = (db::norm(&game.install_path), db::norm(&game.name));
        // `.copied()` drops the borrow of `index` before the arms so the `None`
        // arm can re-borrow it mutably to insert.
        match index.get(&key).copied() {
            Some(i) if store::source_rank(game.source) < store::source_rank(out[i].source) => {
                out[i] = game
            }
            Some(_) => {} // a higher- or equal-priority duplicate already kept
            None => {
                index.insert(key, out.len());
                out.push(game);
            }
        }
    }
    out
}

/// Merge per-source scan results, dedup, persist, and report progress.
///
/// `results` pairs each source with its scan outcome (the command runs the
/// scanners; this function owns merge/dedup/persist/report). Emits one
/// [`ScanProgress`] per source, then [`ScanDone`]. Returns the persisted games
/// with their DB-assigned ids. Per TB1, a failed source or a failed upsert is
/// logged and skipped rather than aborting the scan.
pub fn merge_and_persist(
    db: &Db,
    results: Vec<(Source, CoreResult<Vec<Game>>)>,
    events: &dyn ScanEvents,
) -> CoreResult<Vec<Game>> {
    let mut all = Vec::new();
    let mut scanned_ok: Vec<Source> = Vec::new();
    for (source, result) in results {
        let found = match result {
            Ok(games) => games,
            Err(e) => {
                tracing::warn!(source = source.as_str(), error = %e, "source scan failed; skipping");
                events.progress(ScanProgress {
                    source,
                    found: 0,
                    done: true,
                });
                continue;
            }
        };
        events.progress(ScanProgress {
            source,
            found: found.len(),
            done: true,
        });
        scanned_ok.push(source);
        all.extend(found);
    }

    let mut persisted = Vec::new();
    for mut game in dedup(all) {
        match db.upsert_game(&game) {
            Ok(id) => {
                game.id = id;
                persisted.push(game);
            }
            Err(e) => {
                tracing::warn!(name = %game.name, error = %e, "upsert failed; skipping")
            }
        }
    }

    // Prune stale rows: for each source that scanned successfully, drop any DB
    // game of that source the fresh scan no longer reports — uninstalled games
    // and old mis-detected non-game apps (e.g. the former Uninstall-registry
    // scan's noise). A source that *failed* keeps its rows (no accidental wipe).
    for source in scanned_ok {
        let keep: Vec<i64> = persisted
            .iter()
            .filter(|g| g.source == source)
            .map(|g| g.id)
            .collect();
        match db.prune_source(source, &keep) {
            Ok(n) if n > 0 => {
                tracing::info!(source = source.as_str(), removed = n, "pruned stale games")
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(source = source.as_str(), error = %e, "prune failed; skipping")
            }
        }
    }

    events.done(ScanDone {
        total: persisted.len(),
    });
    Ok(persisted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;
    use std::cell::RefCell;

    fn game(source: Source, name: &str, path: &str) -> Game {
        Game {
            id: 0,
            source,
            external_id: "x".into(),
            name: name.into(),
            install_path: path.into(),
            exe_path: None,
            size_bytes: Some(1),
            drive: Some("C:".into()),
            last_played: None,
            launch_count: 0,
            cover_path: None,
            favorite: false,
            tags: Vec::new(),
        }
    }

    /// Records emitted events for assertions.
    #[derive(Default)]
    struct Recorder {
        progress: RefCell<Vec<ScanProgress>>,
        done: RefCell<Vec<ScanDone>>,
    }
    impl ScanEvents for Recorder {
        fn progress(&self, progress: ScanProgress) {
            self.progress.borrow_mut().push(progress);
        }
        fn done(&self, done: ScanDone) {
            self.done.borrow_mut().push(done);
        }
    }

    #[test]
    fn dedup_keeps_one_per_normalized_key() {
        let games = vec![
            game(Source::Steam, "Portal 2", r"C:\Steam\portal2"),
            // Same game, different casing/separators → same key.
            game(Source::Local, "portal 2", r"c:/steam/portal2/"),
        ];
        let out = dedup(games);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn dedup_prefers_steam_over_local() {
        // Local seen first, but Steam outranks it and must win.
        let games = vec![
            game(Source::Local, "Portal 2", r"C:\Steam\portal2"),
            game(Source::Steam, "Portal 2", r"C:\Steam\portal2"),
        ];
        let out = dedup(games);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source, Source::Steam);
    }

    #[test]
    fn dedup_preserves_distinct_games_in_order() {
        let games = vec![
            game(Source::Steam, "A", r"C:\g\a"),
            game(Source::Steam, "B", r"C:\g\b"),
        ];
        let out = dedup(games);
        let names: Vec<&str> = out.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["A", "B"]);
    }

    #[test]
    fn merge_and_persist_stores_deduped_games_with_ids() {
        let db = Db::open_in_memory().unwrap();
        let rec = Recorder::default();
        let results = vec![(
            Source::Steam,
            Ok(vec![
                game(Source::Steam, "A", r"C:\g\a"),
                game(Source::Steam, "B", r"C:\g\b"),
                game(Source::Local, "a", r"c:/g/a/"), // dup of A (path+name norm)
            ]),
        )];
        let persisted = merge_and_persist(&db, results, &rec).unwrap();
        assert_eq!(persisted.len(), 2, "dup collapsed");
        assert!(persisted.iter().all(|g| g.id != 0), "ids assigned");
        assert_eq!(db.list_games().unwrap().len(), 2, "rows in DB");
    }

    #[test]
    fn merge_and_persist_prunes_stale_games_of_scanned_sources() {
        let db = Db::open_in_memory().unwrap();
        let rec = Recorder::default();
        // An old (e.g. mis-detected) steam row that a fresh scan won't report.
        db.upsert_game(&game(Source::Steam, "Old App", r"C:\old\app"))
            .unwrap();
        let results = vec![(Source::Steam, Ok(vec![game(Source::Steam, "A", r"C:\g\a")]))];
        merge_and_persist(&db, results, &rec).unwrap();
        let names: Vec<String> = db
            .list_games()
            .unwrap()
            .into_iter()
            .map(|g| g.name)
            .collect();
        assert_eq!(names, ["A"], "stale row pruned, fresh kept");
    }

    #[test]
    fn merge_and_persist_does_not_prune_a_failed_source() {
        let db = Db::open_in_memory().unwrap();
        let rec = Recorder::default();
        db.upsert_game(&game(Source::Steam, "Keep Me", r"C:\steam\keep"))
            .unwrap();
        // Steam scan FAILED this run → its existing rows must survive.
        let results = vec![(
            Source::Steam,
            Err(CoreError::Registry("steam path missing".into())),
        )];
        merge_and_persist(&db, results, &rec).unwrap();
        assert_eq!(
            db.list_games().unwrap().len(),
            1,
            "failed source not pruned"
        );
    }

    #[test]
    fn merge_and_persist_emits_progress_per_source_then_done() {
        let db = Db::open_in_memory().unwrap();
        let rec = Recorder::default();
        let results = vec![
            (Source::Steam, Ok(vec![game(Source::Steam, "A", r"C:\g\a")])),
            (Source::Epic, Ok(vec![game(Source::Epic, "B", r"C:\g\b")])),
        ];
        merge_and_persist(&db, results, &rec).unwrap();

        let progress = rec.progress.borrow();
        assert_eq!(progress.len(), 2);
        assert_eq!(progress[0].source, Source::Steam);
        assert_eq!(progress[0].found, 1);
        assert_eq!(progress[1].source, Source::Epic);
        let done = rec.done.borrow();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].total, 2);
    }

    #[test]
    fn merge_and_persist_skips_failed_source_but_keeps_others() {
        let db = Db::open_in_memory().unwrap();
        let rec = Recorder::default();
        let results = vec![
            (
                Source::Steam,
                Err(CoreError::Registry("steam path missing".into())),
            ),
            (Source::Epic, Ok(vec![game(Source::Epic, "B", r"C:\g\b")])),
        ];
        let persisted = merge_and_persist(&db, results, &rec).unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].name, "B");
        // The failed source still reports progress (found = 0).
        let progress = rec.progress.borrow();
        assert_eq!(progress[0].source, Source::Steam);
        assert_eq!(progress[0].found, 0);
        assert_eq!(rec.done.borrow()[0].total, 1);
    }

    #[test]
    fn drive_of_extracts_uppercase_disk_letter() {
        assert_eq!(drive_of(Path::new(r"d:\games\x")).as_deref(), Some("D:"));
        assert_eq!(drive_of(Path::new(r"\\server\share")), None);
    }

    #[test]
    fn scan_progress_serializes_camelcase_lowercase_source() {
        let p = ScanProgress {
            source: Source::Steam,
            found: 3,
            done: true,
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["source"], "steam");
        assert_eq!(json["found"], 3);
        assert_eq!(json["done"], true);
    }
}
