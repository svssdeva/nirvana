//! Ubisoft Connect (Uplay) discovery (FUTURE-PLANS Priority 1). Registry-only,
//! offline: each subkey under `HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs`
//! is a numeric game id carrying an `InstallDir`. Entries whose install folder is
//! gone (uninstalled) are skipped — mirrors `gog.rs`. The game name is derived
//! from the install folder (the registry carries no title). Launch via
//! `uplay://launch/<id>/0`.

use crate::error::CoreResult;
use crate::models::{Game, Source};
use crate::os::{FileSystem, Hive, Registry};
use crate::scan::drive_of;
use crate::scan::store::ScanCtx;
use std::path::Path;

/// Per-game install records live here; each subkey name is a Ubisoft game id.
/// ASSUMPTION (verify on a real install): id subkeys carry an `InstallDir` value.
const INSTALLS_KEY: &str = r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs";

/// The `scan` fn the Ubisoft [`crate::scan::store::Descriptor`] points at.
pub fn scan(ctx: &ScanCtx) -> CoreResult<Vec<Game>> {
    scan_registry(ctx.registry, ctx.fs)
}

/// Read installed Ubisoft games from the registry. Skips entries with no
/// `InstallDir` or whose folder no longer exists.
pub fn scan_registry(reg: &dyn Registry, fs: &dyn FileSystem) -> CoreResult<Vec<Game>> {
    let mut games = Vec::new();
    for game_id in reg.enum_subkeys(Hive::LocalMachine, INSTALLS_KEY)? {
        let key = format!(r"{INSTALLS_KEY}\{game_id}");
        let Some(dir) = reg.read_string(Hive::LocalMachine, &key, "InstallDir")? else {
            continue; // not a usable install record
        };
        // Skip stale entries whose install folder is gone (uninstalled).
        if !fs
            .metadata(Path::new(&dir))
            .map(|m| m.is_dir)
            .unwrap_or(false)
        {
            continue;
        }
        let drive = drive_of(Path::new(&dir));
        games.push(Game {
            id: 0,
            source: Source::Ubisoft,
            external_id: game_id,
            name: folder_name(&dir),
            install_path: dir,
            exe_path: None,
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

/// Title from the install folder's leaf name (registry carries no name),
/// tolerating a trailing path separator. Falls back to the raw string.
fn folder_name(dir: &str) -> String {
    Path::new(dir.trim_end_matches(['\\', '/']))
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(dir)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::fs::FakeFs;
    use crate::os::registry::FakeRegistry;

    fn reg_with_valhalla() -> FakeRegistry {
        FakeRegistry::new()
            .with_subkeys(Hive::LocalMachine, INSTALLS_KEY, &["3159"])
            .with_value(
                Hive::LocalMachine,
                &format!(r"{INSTALLS_KEY}\3159"),
                "InstallDir",
                r"D:\Ubisoft\games\Assassins Creed Valhalla\",
            )
    }

    #[test]
    fn scans_installed_ubisoft_game_from_registry() {
        let reg = reg_with_valhalla();
        let fs = FakeFs::new().with_dir(r"D:\Ubisoft\games\Assassins Creed Valhalla\", vec![]);
        let games = scan_registry(&reg, &fs).unwrap();
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!(g.source, Source::Ubisoft);
        assert_eq!(g.external_id, "3159");
        assert_eq!(g.name, "Assassins Creed Valhalla"); // trailing slash trimmed
        assert_eq!(g.drive.as_deref(), Some("D:"));
    }

    #[test]
    fn skips_ubisoft_game_whose_install_folder_is_gone() {
        let reg = reg_with_valhalla();
        let fs = FakeFs::new(); // install dir NOT seeded → uninstalled leftover
        assert!(scan_registry(&reg, &fs).unwrap().is_empty());
    }

    #[test]
    fn skips_entry_without_install_dir() {
        let reg = FakeRegistry::new().with_subkeys(Hive::LocalMachine, INSTALLS_KEY, &["999"]);
        let fs = FakeFs::new();
        assert!(scan_registry(&reg, &fs).unwrap().is_empty());
    }

    #[test]
    fn empty_when_ubisoft_not_installed() {
        assert!(scan_registry(&FakeRegistry::new(), &FakeFs::new())
            .unwrap()
            .is_empty());
    }
}
