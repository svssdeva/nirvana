//! SQLite persistence (ADR-0001): a single bundled-`rusqlite` database for the
//! library, settings, and tags. Schema evolves via forward-only migrations
//! keyed on `PRAGMA user_version`. (rusqlite 0.40 API: docs.rs/rusqlite/0.40.1)

use crate::error::{CoreError, CoreResult};
use crate::models::{Game, Source};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::Path;

/// Current schema version. Bump and append to `MIGRATIONS` for each change.
pub const SCHEMA_VERSION: i64 = 2;

/// Forward-only migrations. Index `i` brings the DB from version `i` to `i+1`.
const MIGRATIONS: &[&str] = &[
    // v1 — initial schema (system-design.md §3).
    r#"
    CREATE TABLE game (
        id                INTEGER PRIMARY KEY,
        source            TEXT NOT NULL CHECK (source IN ('steam','epic','local')),
        external_id       TEXT NOT NULL,
        name              TEXT NOT NULL,
        name_norm         TEXT NOT NULL,
        install_path      TEXT NOT NULL,
        install_path_norm TEXT NOT NULL,
        exe_path          TEXT,
        size_bytes        INTEGER,
        drive             TEXT,
        last_played       INTEGER,
        launch_count      INTEGER NOT NULL DEFAULT 0,
        cover_path        TEXT,
        favorite          INTEGER NOT NULL DEFAULT 0,
        UNIQUE (install_path_norm, name_norm)
    );
    CREATE TABLE tag (
        id   INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE
    );
    CREATE TABLE game_tag (
        game_id INTEGER NOT NULL REFERENCES game(id) ON DELETE CASCADE,
        tag_id  INTEGER NOT NULL REFERENCES tag(id)  ON DELETE CASCADE,
        PRIMARY KEY (game_id, tag_id)
    );
    CREATE TABLE setting (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    "#,
    // v2 — widen the `source` CHECK to the full planned store allowlist so new
    // stores need no further migration. SQLite can't alter a CHECK, so rebuild
    // `game` (same columns/order as v1, only the CHECK changes) and copy rows.
    // `migrate` runs this with foreign_keys OFF, so dropping `game` can't
    // cascade-delete `game_tag`; the new table reuses the same ids, so the
    // existing tag links stay valid.
    r#"
    CREATE TABLE game_new (
        id                INTEGER PRIMARY KEY,
        source            TEXT NOT NULL CHECK (source IN
            ('steam','epic','local','gog','xbox','ea','ubisoft','battlenet','itch','riot')),
        external_id       TEXT NOT NULL,
        name              TEXT NOT NULL,
        name_norm         TEXT NOT NULL,
        install_path      TEXT NOT NULL,
        install_path_norm TEXT NOT NULL,
        exe_path          TEXT,
        size_bytes        INTEGER,
        drive             TEXT,
        last_played       INTEGER,
        launch_count      INTEGER NOT NULL DEFAULT 0,
        cover_path        TEXT,
        favorite          INTEGER NOT NULL DEFAULT 0,
        UNIQUE (install_path_norm, name_norm)
    );
    INSERT INTO game_new SELECT * FROM game;
    DROP TABLE game;
    ALTER TABLE game_new RENAME TO game;
    "#,
];

/// Columns selected for a [`Game`], in `row_to_game` order. Not user input.
const GAME_COLUMNS: &str = "id, source, external_id, name, install_path, exe_path, \
     size_bytes, drive, last_played, launch_count, cover_path, favorite";

/// Handle to the open database. One per app (shared via managed state later).
pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open an in-memory DB (tests) and run migrations.
    pub fn open_in_memory() -> CoreResult<Self> {
        Self::from_conn(Connection::open_in_memory()?)
    }

    /// Open (or create) a file-backed DB and run migrations.
    pub fn open(path: &Path) -> CoreResult<Self> {
        Self::from_conn(Connection::open(path)?)
    }

    fn from_conn(mut conn: Connection) -> CoreResult<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// Current `user_version`.
    pub fn version(&self) -> CoreResult<i64> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    /// Insert a game, or update it if one with the same dedup key
    /// (normalized install path + name) already exists. Returns its id.
    pub fn upsert_game(&self, game: &Game) -> CoreResult<i64> {
        let name_norm = norm(&game.name);
        let path_norm = norm(&game.install_path);
        // UPSERT returns the row id directly (SQLite RETURNING, 3.35+), avoiding
        // a second SELECT on the update path.
        let id: i64 = self.conn.query_row(
            "INSERT INTO game \
             (source, external_id, name, name_norm, install_path, install_path_norm, \
              exe_path, size_bytes, drive, last_played, launch_count, cover_path, favorite) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13) \
             ON CONFLICT (install_path_norm, name_norm) DO UPDATE SET \
              source=excluded.source, external_id=excluded.external_id, name=excluded.name, \
              exe_path=excluded.exe_path, size_bytes=excluded.size_bytes, drive=excluded.drive, \
              last_played=excluded.last_played, launch_count=excluded.launch_count, \
              cover_path=excluded.cover_path, favorite=excluded.favorite \
             RETURNING id",
            params![
                game.source.as_str(),
                game.external_id,
                game.name,
                name_norm,
                game.install_path,
                path_norm,
                game.exe_path,
                game.size_bytes,
                game.drive,
                game.last_played,
                game.launch_count,
                game.cover_path,
                game.favorite,
            ],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    /// Fetch a game by id (with its tags).
    pub fn get_game(&self, id: i64) -> CoreResult<Option<Game>> {
        let sql = format!("SELECT {GAME_COLUMNS} FROM game WHERE id=?1");
        let mut game: Option<Game> = self
            .conn
            .query_row(&sql, params![id], row_to_game)
            .optional()?;
        if let Some(g) = game.as_mut() {
            g.tags = self.tags_for(g.id)?;
        }
        Ok(game)
    }

    /// Toggle a game's favorite flag. Errors `NotFound` if no such game.
    pub fn set_favorite(&self, id: i64, favorite: bool) -> CoreResult<()> {
        let rows = self.conn.execute(
            "UPDATE game SET favorite = ?1 WHERE id = ?2",
            params![favorite, id],
        )?;
        if rows == 0 {
            return Err(CoreError::NotFound(format!("game {id}")));
        }
        Ok(())
    }

    /// Replace a game's tags with `tags` (deduped, trimmed, non-empty). Unknown
    /// tag names are created; orphaned `tag` rows are left for a future cleanup.
    /// Errors `NotFound` if no such game. Runs in one transaction.
    pub fn set_tags(&self, id: i64, tags: &[String]) -> CoreResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        let exists: bool = tx
            .query_row("SELECT 1 FROM game WHERE id = ?1", params![id], |_| Ok(()))
            .optional()?
            .is_some();
        if !exists {
            return Err(CoreError::NotFound(format!("game {id}")));
        }
        tx.execute("DELETE FROM game_tag WHERE game_id = ?1", params![id])?;
        for name in normalize_tags(tags) {
            tx.execute(
                "INSERT INTO tag (name) VALUES (?1) ON CONFLICT (name) DO NOTHING",
                params![name],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO game_tag (game_id, tag_id) \
                 SELECT ?1, id FROM tag WHERE name = ?2",
                params![id, name],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// A game's tag names, sorted.
    pub fn tags_for(&self, id: i64) -> CoreResult<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.name FROM tag t \
             JOIN game_tag gt ON gt.tag_id = t.id \
             WHERE gt.game_id = ?1 ORDER BY t.name COLLATE NOCASE",
        )?;
        let tags = stmt
            .query_map(params![id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(tags)
    }

    /// Read a `setting` value by key, or `None` if unset.
    pub fn get_setting(&self, key: &str) -> CoreResult<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM setting WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Insert or replace a `setting` value (parameterized — TB5).
    pub fn set_setting(&self, key: &str, value: &str) -> CoreResult<()> {
        self.conn.execute(
            "INSERT INTO setting (key, value) VALUES (?1, ?2) \
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Set a game's cover image path (a custom thumbnail). Errors `NotFound` if
    /// no such game.
    pub fn set_cover_path(&self, id: i64, path: &str) -> CoreResult<()> {
        let rows = self.conn.execute(
            "UPDATE game SET cover_path = ?1 WHERE id = ?2",
            params![path, id],
        )?;
        if rows == 0 {
            return Err(CoreError::NotFound(format!("game {id}")));
        }
        Ok(())
    }

    /// Wipe the whole library: all games, tags, and settings. The schema (and
    /// `user_version`) is kept, so the DB is immediately reusable — equivalent to
    /// a fresh database. Used by the Settings "delete database" action.
    pub fn reset(&self) -> CoreResult<()> {
        // game_tag rows cascade from the game/tag deletes (FK ON DELETE CASCADE).
        self.conn
            .execute_batch("DELETE FROM game; DELETE FROM tag; DELETE FROM setting;")?;
        Ok(())
    }

    /// Remove games of `source` whose id is **not** in `keep_ids` — the rows a
    /// fresh scan of that source no longer reports (uninstalled games, or
    /// previously mis-detected non-game apps). Tags/junctions cascade. Returns the
    /// count removed. Only call for a source that scanned successfully, so a
    /// transient scan failure never wipes a source's library.
    pub fn prune_source(&self, source: Source, keep_ids: &[i64]) -> CoreResult<usize> {
        let removed = if keep_ids.is_empty() {
            self.conn.execute(
                "DELETE FROM game WHERE source = ?1",
                params![source.as_str()],
            )?
        } else {
            // ids are our own i64s (never user input) — safe to inline into IN().
            let csv = keep_ids
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!("DELETE FROM game WHERE source = ?1 AND id NOT IN ({csv})");
            self.conn.execute(&sql, params![source.as_str()])?
        };
        Ok(removed)
    }

    /// Persist a freshly computed on-disk size (bytes) for a game. Errors
    /// `NotFound` if no such game.
    pub fn set_size_bytes(&self, id: i64, bytes: i64) -> CoreResult<()> {
        let rows = self.conn.execute(
            "UPDATE game SET size_bytes = ?1 WHERE id = ?2",
            params![bytes, id],
        )?;
        if rows == 0 {
            return Err(CoreError::NotFound(format!("game {id}")));
        }
        Ok(())
    }

    /// Record a launch: stamp `last_played` and bump `launch_count`. `at` is a
    /// Unix timestamp (seconds). Errors `NotFound` if no such game.
    pub fn record_launch(&self, id: i64, at: i64) -> CoreResult<()> {
        let rows = self.conn.execute(
            "UPDATE game SET last_played = ?1, launch_count = launch_count + 1 WHERE id = ?2",
            params![at, id],
        )?;
        if rows == 0 {
            return Err(CoreError::NotFound(format!("game {id}")));
        }
        Ok(())
    }

    /// All games (with tags), ordered by normalized name.
    pub fn list_games(&self) -> CoreResult<Vec<Game>> {
        let sql = format!("SELECT {GAME_COLUMNS} FROM game ORDER BY name_norm");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut games = stmt
            .query_map([], row_to_game)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut tags = self.all_tags()?;
        for game in &mut games {
            if let Some(t) = tags.remove(&game.id) {
                game.tags = t;
            }
        }
        Ok(games)
    }

    /// All game→tag-names in one query, grouped by game id.
    fn all_tags(&self) -> CoreResult<std::collections::HashMap<i64, Vec<String>>> {
        let mut stmt = self.conn.prepare(
            "SELECT gt.game_id, t.name FROM game_tag gt \
             JOIN tag t ON t.id = gt.tag_id ORDER BY t.name COLLATE NOCASE",
        )?;
        let mut map: std::collections::HashMap<i64, Vec<String>> = std::collections::HashMap::new();
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (game_id, name) = row?;
            map.entry(game_id).or_default().push(name);
        }
        Ok(map)
    }
}

/// Trim, drop empties, and case-insensitively dedup tag names (preserving the
/// first spelling and input order).
fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    tags.iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .filter(|t| seen.insert(t.to_lowercase()))
        .map(str::to_string)
        .collect()
}

/// Apply any migrations the DB hasn't seen yet, each in its own transaction.
///
/// Foreign-key enforcement is disabled for the duration so a table-rebuild
/// migration (e.g. v2's `DROP TABLE game`) can't trigger an implicit cascade
/// into referencing tables like `game_tag`. `PRAGMA foreign_keys` is a no-op
/// inside a transaction, so it's toggled here, around the per-migration ones,
/// and restored to its prior state afterwards (SQLite's recommended procedure).
fn migrate(conn: &mut Connection) -> CoreResult<()> {
    let fk_was_on: bool = conn.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
    conn.pragma_update(None, "foreign_keys", false)?;

    let result = (|| -> CoreResult<()> {
        let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate() {
            let target = i as i64 + 1;
            if current >= target {
                continue;
            }
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", target)?;
            tx.commit()?;
        }
        Ok(())
    })();

    // Restore prior FK enforcement regardless of migration outcome.
    conn.pragma_update(None, "foreign_keys", fk_was_on)?;
    result
}

/// Dedup-normalize a name or path: trimmed, lowercased, forward slashes → back.
/// Windows paths are case-insensitive and slash-agnostic. Shared with
/// `scan::dedup` so the in-memory cross-source dedup key matches the DB's
/// `UNIQUE (install_path_norm, name_norm)` constraint exactly.
pub(crate) fn norm(s: &str) -> String {
    s.trim()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn row_to_game(r: &Row) -> rusqlite::Result<Game> {
    let source_str: String = r.get("source")?;
    let source = Source::parse(&source_str).map_err(|_| {
        rusqlite::Error::InvalidColumnType(1, "source".into(), rusqlite::types::Type::Text)
    })?;
    Ok(Game {
        id: r.get("id")?,
        source,
        external_id: r.get("external_id")?,
        name: r.get("name")?,
        install_path: r.get("install_path")?,
        exe_path: r.get("exe_path")?,
        size_bytes: r.get("size_bytes")?,
        drive: r.get("drive")?,
        last_played: r.get("last_played")?,
        launch_count: r.get("launch_count")?,
        cover_path: r.get("cover_path")?,
        favorite: r.get("favorite")?,
        tags: Vec::new(), // filled by list_games/get_game via a tags query
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Source;

    fn sample_game(name: &str, path: &str) -> Game {
        Game {
            id: 0,
            source: Source::Steam,
            external_id: "440".into(),
            name: name.into(),
            install_path: path.into(),
            exe_path: None,
            size_bytes: Some(1024),
            drive: Some("C".into()),
            last_played: None,
            launch_count: 0,
            cover_path: None,
            favorite: false,
            tags: Vec::new(),
        }
    }

    #[test]
    fn fresh_db_is_at_current_version() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn upsert_then_get_returns_the_game() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .upsert_game(&sample_game("Team Fortress 2", r"C:\Steam\tf2"))
            .unwrap();
        let got = db.get_game(id).unwrap().expect("game should exist");
        assert_eq!(got.id, id);
        assert_eq!(got.name, "Team Fortress 2");
        assert_eq!(got.source, Source::Steam);
        assert_eq!(got.size_bytes, Some(1024));
    }

    #[test]
    fn get_missing_game_returns_none() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.get_game(999).unwrap().is_none());
    }

    #[test]
    fn upsert_dedups_on_normalized_path_and_name() {
        let db = Db::open_in_memory().unwrap();
        // Same game, different path casing/separators + updated size.
        let id1 = db
            .upsert_game(&sample_game("Portal 2", r"C:\Steam\portal2"))
            .unwrap();
        let mut again = sample_game("portal 2", r"c:/steam/portal2");
        again.size_bytes = Some(2048);
        let id2 = db.upsert_game(&again).unwrap();
        assert_eq!(id1, id2, "same dedup key should update, not insert");
        assert_eq!(db.list_games().unwrap().len(), 1);
        assert_eq!(db.get_game(id1).unwrap().unwrap().size_bytes, Some(2048));
    }

    #[test]
    fn reopen_file_db_is_idempotent_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nirvana.db");
        let id = {
            let db = Db::open(&path).unwrap();
            db.upsert_game(&sample_game("Half-Life", r"C:\Steam\hl"))
                .unwrap()
        };
        // Reopen: migrations must be a no-op and data must persist.
        let db = Db::open(&path).unwrap();
        assert_eq!(db.version().unwrap(), SCHEMA_VERSION);
        let games = db.list_games().unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].id, id);
        assert_eq!(games[0].name, "Half-Life");
    }

    #[test]
    fn v2_migration_widens_check_and_preserves_tagged_rows() {
        use rusqlite::Connection;
        // Hand-build a v1 DB (old CHECK) and stop at version 1.
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1i64).unwrap();
        // Seed a game with a tag association — the thing a naive table rebuild
        // would cascade-delete when it drops `game`.
        conn.execute(
            "INSERT INTO game (source, external_id, name, name_norm, install_path, install_path_norm)
             VALUES ('steam','1','Keep','keep',?1,?1)",
            [r"c:\g\keep"],
        )
        .unwrap();
        conn.execute("INSERT INTO tag (name) VALUES ('RPG')", [])
            .unwrap();
        conn.execute("INSERT INTO game_tag (game_id, tag_id) VALUES (1, 1)", [])
            .unwrap();
        // Mimic the app: FK enforcement ON entering migrate. The rebuild must
        // NOT cascade-delete game_tag.
        conn.pragma_update(None, "foreign_keys", true).unwrap();

        migrate(&mut conn).unwrap();

        // The widened CHECK now accepts 'gog'.
        conn.execute(
            "INSERT INTO game (source, external_id, name, name_norm, install_path, install_path_norm)
             VALUES ('gog','111','G','g',?1,?1)",
            [r"c:\g\gog"],
        )
        .unwrap();
        let games: i64 = conn
            .query_row("SELECT COUNT(*) FROM game", [], |r| r.get(0))
            .unwrap();
        let links: i64 = conn
            .query_row("SELECT COUNT(*) FROM game_tag", [], |r| r.get(0))
            .unwrap();
        assert_eq!(games, 2, "kept row + new gog row");
        assert_eq!(links, 1, "tag association must survive the table rebuild");
        // FK enforcement is restored after migrate.
        let fk: i64 = conn
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1, "foreign_keys restored to on");
    }

    #[test]
    fn record_launch_stamps_time_and_increments_count() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .upsert_game(&sample_game("Celeste", r"C:\Steam\celeste"))
            .unwrap();
        db.record_launch(id, 1_700_000_000).unwrap();
        let after = db.get_game(id).unwrap().unwrap();
        assert_eq!(after.last_played, Some(1_700_000_000));
        assert_eq!(after.launch_count, 1);

        db.record_launch(id, 1_700_000_100).unwrap();
        let again = db.get_game(id).unwrap().unwrap();
        assert_eq!(again.last_played, Some(1_700_000_100));
        assert_eq!(again.launch_count, 2);
    }

    #[test]
    fn reset_wipes_games_tags_and_settings_keeping_schema() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .upsert_game(&sample_game("Hades", r"C:\g\hades"))
            .unwrap();
        db.set_tags(id, &["Roguelike".into()]).unwrap();
        db.set_setting("watchFolders", r#"["D:\\Games"]"#).unwrap();

        db.reset().unwrap();

        assert!(db.list_games().unwrap().is_empty());
        assert_eq!(db.get_setting("watchFolders").unwrap(), None);
        assert_eq!(db.version().unwrap(), SCHEMA_VERSION, "schema kept");
        // DB still usable after reset.
        assert!(db
            .upsert_game(&sample_game("Celeste", r"C:\g\celeste"))
            .is_ok());
    }

    #[test]
    fn set_cover_path_updates_and_errors_on_missing() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .upsert_game(&sample_game("Tunic", r"C:\g\tunic"))
            .unwrap();
        db.set_cover_path(id, r"C:\cache\covers\custom-1.png")
            .unwrap();
        assert_eq!(
            db.get_game(id).unwrap().unwrap().cover_path.as_deref(),
            Some(r"C:\cache\covers\custom-1.png")
        );
        assert!(matches!(
            db.set_cover_path(404, "x").unwrap_err(),
            CoreError::NotFound(_)
        ));
    }

    #[test]
    fn set_size_bytes_updates_and_errors_on_missing() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .upsert_game(&sample_game("Hades", r"C:\Steam\hades"))
            .unwrap();
        db.set_size_bytes(id, 9_876_543_210).unwrap();
        assert_eq!(
            db.get_game(id).unwrap().unwrap().size_bytes,
            Some(9_876_543_210)
        );
        assert!(matches!(
            db.set_size_bytes(424242, 1).unwrap_err(),
            CoreError::NotFound(_)
        ));
    }

    #[test]
    fn set_favorite_persists_and_errors_on_missing() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .upsert_game(&sample_game("Hades", r"C:\g\hades"))
            .unwrap();
        db.set_favorite(id, true).unwrap();
        assert!(db.get_game(id).unwrap().unwrap().favorite);
        db.set_favorite(id, false).unwrap();
        assert!(!db.get_game(id).unwrap().unwrap().favorite);
        assert!(matches!(
            db.set_favorite(9999, true).unwrap_err(),
            CoreError::NotFound(_)
        ));
    }

    #[test]
    fn tags_replace_dedup_and_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .upsert_game(&sample_game("Celeste", r"C:\g\celeste"))
            .unwrap();

        db.set_tags(id, &["Platformer".into(), "indie".into(), " INDIE ".into()])
            .unwrap();
        // Case-insensitive dedup + trim → "indie" kept once; sorted on read.
        assert_eq!(db.tags_for(id).unwrap(), vec!["indie", "Platformer"]);
        assert_eq!(
            db.get_game(id).unwrap().unwrap().tags,
            vec!["indie", "Platformer"]
        );
        assert_eq!(
            db.list_games().unwrap()[0].tags,
            vec!["indie", "Platformer"]
        );

        // set_tags replaces (not appends).
        db.set_tags(id, &["favorite-genre".into()]).unwrap();
        assert_eq!(db.tags_for(id).unwrap(), vec!["favorite-genre"]);

        // Clearing tags works.
        db.set_tags(id, &[]).unwrap();
        assert!(db.tags_for(id).unwrap().is_empty());
    }

    #[test]
    fn set_tags_on_missing_game_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        assert!(matches!(
            db.set_tags(123, &["x".into()]).unwrap_err(),
            CoreError::NotFound(_)
        ));
    }

    #[test]
    fn setting_set_get_and_overwrite() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.get_setting("watchFolders").unwrap(), None);
        db.set_setting("watchFolders", r#"["D:\\Games"]"#).unwrap();
        assert_eq!(
            db.get_setting("watchFolders").unwrap().as_deref(),
            Some(r#"["D:\\Games"]"#)
        );
        db.set_setting("watchFolders", "[]").unwrap();
        assert_eq!(
            db.get_setting("watchFolders").unwrap().as_deref(),
            Some("[]")
        );
    }

    #[test]
    fn record_launch_on_missing_game_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        assert!(matches!(
            db.record_launch(999, 1).unwrap_err(),
            CoreError::NotFound(_)
        ));
    }

    #[test]
    fn upsert_dedups_ignoring_trailing_separator() {
        let db = Db::open_in_memory().unwrap();
        let id1 = db
            .upsert_game(&sample_game("Dota 2", r"C:\Steam\dota2"))
            .unwrap();
        let id2 = db
            .upsert_game(&sample_game("Dota 2", r"C:\Steam\dota2\"))
            .unwrap();
        assert_eq!(id1, id2, "trailing separator should not create a duplicate");
        assert_eq!(db.list_games().unwrap().len(), 1);
    }
}
