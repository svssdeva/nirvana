//! GOG discovery (spec §4). The Windows registry is the reliable spine (present
//! for any installed GOG game, with or without the Galaxy client); the Galaxy
//! `galaxy-2.0.db` enriches it in a later task. Both sources are local — no
//! network. Mirrors the Epic scanner's stale-entry guard: a registry entry whose
//! install folder is gone (uninstalled) is skipped.

use crate::error::CoreResult;
use crate::models::{Game, Source};
use crate::os::{FileSystem, Hive, Registry};
use crate::scan::drive_of;
use crate::scan::store::ScanCtx;
use rusqlite::{Connection, OpenFlags};
use std::collections::HashSet;
use std::path::Path;

/// Descriptor adapter (spec §4): registry games (the reliable spine) enriched
/// with Galaxy DB entries, merged by productId. This is the `scan` fn the GOG
/// [`crate::scan::store::Descriptor`] points at.
pub fn scan(ctx: &ScanCtx) -> CoreResult<Vec<Game>> {
    let games = scan_registry(ctx.registry, ctx.fs)?;
    Ok(merge(games, &GalaxyDb, &ctx.program_data))
}

/// Per-game registry keys live under here; each subkey name is a GOG productId.
const GOG_GAMES_KEY: &str = r"SOFTWARE\WOW6432Node\GOG.com\Games";

/// Read installed GOG games from the registry. Each subkey under
/// `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games` is a productId with `gameName`,
/// `path`, and `exe` values. Entries whose `path` no longer exists are skipped.
pub fn scan_registry(reg: &dyn Registry, fs: &dyn FileSystem) -> CoreResult<Vec<Game>> {
    let mut games = Vec::new();
    for product_id in reg.enum_subkeys(Hive::LocalMachine, GOG_GAMES_KEY)? {
        let key = format!(r"{GOG_GAMES_KEY}\{product_id}");
        let read = |name: &str| reg.read_string(Hive::LocalMachine, &key, name);
        let (Some(path), Some(name)) = (read("path")?, read("gameName")?) else {
            continue; // not a real game entry (missing required fields)
        };
        // Skip stale entries whose install folder is gone (uninstalled).
        if !fs
            .metadata(Path::new(&path))
            .map(|m| m.is_dir)
            .unwrap_or(false)
        {
            continue;
        }
        let exe_path = read("exe")?.filter(|e| !e.is_empty());
        let drive = drive_of(Path::new(&path));
        games.push(Game {
            id: 0,
            source: Source::Gog,
            external_id: product_id,
            name,
            install_path: path,
            exe_path,
            size_bytes: None,
            drive,
            last_played: None,
            launch_count: 0,
            cover_path: None,
            favorite: false,
            tags: Vec::new(),
        });
    }
    Ok(games)
}

/// An installed product read from Galaxy's local DB (seam output).
#[derive(Clone, Debug)]
pub struct GogEntry {
    pub product_id: String,
    pub name: String,
    pub install_path: String,
}

/// Reads installed products from GOG Galaxy's local `galaxy-2.0.db`. Best-effort:
/// implementations return `Ok(vec![])` on any missing-file / locked / unexpected-
/// schema condition, so the DB never blocks a scan (the registry is the spine).
pub trait GalaxySource {
    fn installed(&self, program_data: &Path) -> CoreResult<Vec<GogEntry>>;
}

/// Merge registry games with Galaxy entries by productId. The registry wins on
/// conflict (it's the reliable source, already existence-checked); Galaxy-only
/// products — installs the registry somehow lacks — are appended.
pub fn merge(mut games: Vec<Game>, galaxy: &dyn GalaxySource, program_data: &Path) -> Vec<Game> {
    let have: HashSet<String> = games.iter().map(|g| g.external_id.clone()).collect();
    if let Ok(entries) = galaxy.installed(program_data) {
        for e in entries {
            if have.contains(&e.product_id) {
                continue; // registry already has it (and it wins)
            }
            games.push(Game {
                id: 0,
                source: Source::Gog,
                external_id: e.product_id,
                name: e.name,
                drive: drive_of(Path::new(&e.install_path)),
                install_path: e.install_path,
                exe_path: None,
                size_bytes: None,
                last_played: None,
                launch_count: 0,
                cover_path: None,
                favorite: false,
                tags: Vec::new(),
            });
        }
    }
    games
}

/// The real Galaxy DB reader.
pub struct GalaxyDb;

impl GalaxySource for GalaxyDb {
    fn installed(&self, program_data: &Path) -> CoreResult<Vec<GogEntry>> {
        let path = program_data.join(r"GOG.com\Galaxy\storage\galaxy-2.0.db");
        // Best-effort: any failure (no Galaxy, locked, schema drift) → empty.
        Ok(read_galaxy_db(&path).unwrap_or_default())
    }
}

/// Read installed products from a `galaxy-2.0.db` file, opened **read-only** so we
/// never lock or mutate GOG's database.
///
/// NOTE: GOG's internal schema is undocumented and varies by Galaxy version. This
/// targets the widely-observed shape (`InstalledBaseProducts` joined to
/// `LimitedDetails` for the title) and should be re-confirmed against a real
/// install; on any mismatch the caller degrades to registry-only discovery.
fn read_galaxy_db(path: &Path) -> CoreResult<Vec<GogEntry>> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare(
        "SELECT i.productId, COALESCE(d.title, ''), i.installationPath \
         FROM InstalledBaseProducts i \
         LEFT JOIN LimitedDetails d ON d.productId = i.productId",
    )?;
    let rows = stmt.query_map([], |r| {
        let product_id: i64 = r.get(0)?;
        let title: String = r.get(1)?;
        let install_path: String = r.get(2)?;
        let product_id = product_id.to_string();
        Ok(GogEntry {
            name: if title.is_empty() {
                product_id.clone()
            } else {
                title
            },
            product_id,
            install_path,
        })
    })?;
    Ok(rows
        .filter_map(Result::ok)
        .filter(|e| !e.install_path.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::fs::FakeFs;
    use crate::os::registry::FakeRegistry;

    const GAMES: &str = r"SOFTWARE\WOW6432Node\GOG.com\Games";

    fn reg_with_witcher() -> FakeRegistry {
        let key = format!(r"{GAMES}\1207658924");
        FakeRegistry::new()
            .with_subkeys(Hive::LocalMachine, GAMES, &["1207658924"])
            .with_value(Hive::LocalMachine, &key, "gameName", "The Witcher 3")
            .with_value(Hive::LocalMachine, &key, "path", r"C:\GOG\Witcher3")
            .with_value(
                Hive::LocalMachine,
                &key,
                "exe",
                r"C:\GOG\Witcher3\witcher3.exe",
            )
    }

    #[test]
    fn scans_installed_gog_game_from_registry() {
        let reg = reg_with_witcher();
        let fs = FakeFs::new().with_dir(r"C:\GOG\Witcher3", vec![]); // install exists
        let games = scan_registry(&reg, &fs).unwrap();
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!(g.source, Source::Gog);
        assert_eq!(g.external_id, "1207658924");
        assert_eq!(g.name, "The Witcher 3");
        assert_eq!(g.install_path, r"C:\GOG\Witcher3");
        assert_eq!(g.exe_path.as_deref(), Some(r"C:\GOG\Witcher3\witcher3.exe"));
        assert_eq!(g.drive.as_deref(), Some("C:"));
    }

    #[test]
    fn skips_gog_game_whose_install_folder_is_gone() {
        let reg = reg_with_witcher();
        let fs = FakeFs::new(); // install dir NOT seeded → uninstalled leftover
        let games = scan_registry(&reg, &fs).unwrap();
        assert!(games.is_empty());
    }

    #[test]
    fn skips_entry_missing_required_values() {
        // Subkey exists but lacks path/gameName → not a usable game entry.
        let reg = FakeRegistry::new().with_subkeys(Hive::LocalMachine, GAMES, &["999"]);
        let fs = FakeFs::new();
        assert!(scan_registry(&reg, &fs).unwrap().is_empty());
    }

    #[test]
    fn empty_when_gog_not_installed() {
        let reg = FakeRegistry::new();
        let fs = FakeFs::new();
        assert!(scan_registry(&reg, &fs).unwrap().is_empty());
    }

    // ── Galaxy DB merge + reader ─────────────────────────────────────────────

    struct FakeGalaxy(Vec<GogEntry>);
    impl GalaxySource for FakeGalaxy {
        fn installed(&self, _pd: &Path) -> CoreResult<Vec<GogEntry>> {
            Ok(self.0.clone())
        }
    }

    fn entry(id: &str, name: &str, path: &str) -> GogEntry {
        GogEntry {
            product_id: id.into(),
            name: name.into(),
            install_path: path.into(),
        }
    }

    #[test]
    fn merge_prefers_registry_and_adds_galaxy_only() {
        let reg_games = {
            let reg = reg_with_witcher();
            let fs = FakeFs::new().with_dir(r"C:\GOG\Witcher3", vec![]);
            scan_registry(&reg, &fs).unwrap()
        };
        let galaxy = FakeGalaxy(vec![
            // Same product as the registry → registry wins (name unchanged).
            entry("1207658924", "Witcher (galaxy)", r"C:\GOG\Witcher3"),
            // Galaxy-only product → added.
            entry("12345", "Stellaris", r"D:\GOG\Stellaris"),
        ]);
        let merged = merge(reg_games, &galaxy, Path::new(r"C:\ProgramData"));

        assert_eq!(merged.len(), 2);
        let witcher = merged
            .iter()
            .find(|g| g.external_id == "1207658924")
            .unwrap();
        assert_eq!(witcher.name, "The Witcher 3", "registry wins on conflict");
        let stellaris = merged.iter().find(|g| g.external_id == "12345").unwrap();
        assert_eq!(stellaris.name, "Stellaris");
        assert_eq!(stellaris.source, Source::Gog);
        assert_eq!(stellaris.drive.as_deref(), Some("D:"));
    }

    #[test]
    fn merge_with_no_galaxy_entries_keeps_registry_games() {
        let reg_games = vec![Game {
            id: 0,
            source: Source::Gog,
            external_id: "1".into(),
            name: "A".into(),
            install_path: r"C:\g\a".into(),
            exe_path: None,
            size_bytes: None,
            drive: Some("C:".into()),
            last_played: None,
            launch_count: 0,
            cover_path: None,
            favorite: false,
            tags: vec![],
        }];
        let merged = merge(reg_games, &FakeGalaxy(vec![]), Path::new(r"C:\ProgramData"));
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn reads_installed_products_from_a_galaxy_db() {
        // Build a DB mirroring the assumed Galaxy schema, then read it back.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("galaxy-2.0.db");
        {
            let c = Connection::open(&db_path).unwrap();
            c.execute_batch(
                "CREATE TABLE InstalledBaseProducts (productId INTEGER, installationPath TEXT);
                 CREATE TABLE LimitedDetails (productId INTEGER, title TEXT);
                 INSERT INTO InstalledBaseProducts VALUES (1207658924, 'C:\\GOG\\Witcher3');
                 INSERT INTO LimitedDetails VALUES (1207658924, 'The Witcher 3');",
            )
            .unwrap();
        }
        let entries = read_galaxy_db(&db_path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].product_id, "1207658924");
        assert_eq!(entries[0].name, "The Witcher 3");
        assert_eq!(entries[0].install_path, r"C:\GOG\Witcher3");
    }

    #[test]
    fn galaxy_db_missing_or_bad_degrades_to_empty() {
        // No file at the resolved path → best-effort empty, never an error.
        let entries = GalaxyDb
            .installed(Path::new(r"C:\definitely\not\here"))
            .unwrap();
        assert!(entries.is_empty());
    }
}
