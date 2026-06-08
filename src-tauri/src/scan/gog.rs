//! GOG discovery (spec §4). The Windows registry is the reliable spine (present
//! for any installed GOG game, with or without the Galaxy client); the Galaxy
//! `galaxy-2.0.db` enriches it in a later task. Both sources are local — no
//! network. Mirrors the Epic scanner's stale-entry guard: a registry entry whose
//! install folder is gone (uninstalled) is skipped.

use crate::error::CoreResult;
use crate::models::{Game, Source};
use crate::os::{FileSystem, Hive, Registry};
use crate::scan::drive_of;
use std::path::Path;

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
}
