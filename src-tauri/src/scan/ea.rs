//! EA app / Origin discovery (FUTURE-PLANS Priority 1). Registry-only spine,
//! offline: each subkey under `HKLM\SOFTWARE\WOW6432Node\Origin Games` is an EA
//! content id carrying an `Install Dir`. Entries whose folder is gone are skipped
//! (mirrors `gog.rs`/`ubisoft.rs`). Launch via `origin2://game/launch?offerIds=<id>`.
//!
//! DEFERRED (like GOG's Galaxy DB): `%PROGRAMDATA%` EA-app/Origin manifests
//! (`.mfst` / `installerdata.xml`) could enrich titles the registry lacks. The
//! registry is the reliable spine; manifest enrichment is a later task once the
//! format is confirmed against a real EA-app install.

use crate::error::CoreResult;
use crate::models::{Game, Source};
use crate::os::{FileSystem, Hive, Registry};
use crate::scan::drive_of;
use crate::scan::store::ScanCtx;
use std::path::Path;

/// Per-game install records; each subkey name is an EA content/offer id.
/// ASSUMPTION (verify on a real install): EA app writes Origin-compatible keys
/// here with an `Install Dir` value.
const ORIGIN_GAMES_KEY: &str = r"SOFTWARE\WOW6432Node\Origin Games";

/// The `scan` fn the EA [`crate::scan::store::Descriptor`] points at.
pub fn scan(ctx: &ScanCtx) -> CoreResult<Vec<Game>> {
    scan_registry(ctx.registry, ctx.fs)
}

/// Read installed EA games from the registry. Skips entries with no `Install Dir`
/// or whose folder no longer exists.
pub fn scan_registry(reg: &dyn Registry, fs: &dyn FileSystem) -> CoreResult<Vec<Game>> {
    let mut games = Vec::new();
    for content_id in reg.enum_subkeys(Hive::LocalMachine, ORIGIN_GAMES_KEY)? {
        let key = format!(r"{ORIGIN_GAMES_KEY}\{content_id}");
        let Some(dir) = reg.read_string(Hive::LocalMachine, &key, "Install Dir")? else {
            continue; // not a usable install record
        };
        if !fs
            .metadata(Path::new(&dir))
            .map(|m| m.is_dir)
            .unwrap_or(false)
        {
            continue; // stale: install folder gone (uninstalled)
        }
        let drive = drive_of(Path::new(&dir));
        games.push(Game {
            id: 0,
            source: Source::Ea,
            external_id: content_id,
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

/// Title from the install folder's leaf name (registry carries no title),
/// tolerating a trailing path separator.
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

    fn reg_with_titanfall() -> FakeRegistry {
        FakeRegistry::new()
            .with_subkeys(
                Hive::LocalMachine,
                ORIGIN_GAMES_KEY,
                &["OFB-EAST:109552316"],
            )
            .with_value(
                Hive::LocalMachine,
                &format!(r"{ORIGIN_GAMES_KEY}\OFB-EAST:109552316"),
                "Install Dir",
                r"C:\Program Files (x86)\Origin Games\Titanfall 2\",
            )
    }

    #[test]
    fn scans_installed_ea_game_from_registry() {
        let reg = reg_with_titanfall();
        let fs =
            FakeFs::new().with_dir(r"C:\Program Files (x86)\Origin Games\Titanfall 2\", vec![]);
        let games = scan_registry(&reg, &fs).unwrap();
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!(g.source, Source::Ea);
        assert_eq!(g.external_id, "OFB-EAST:109552316");
        assert_eq!(g.name, "Titanfall 2"); // trailing slash trimmed
        assert_eq!(g.drive.as_deref(), Some("C:"));
    }

    #[test]
    fn skips_ea_game_whose_install_folder_is_gone() {
        let reg = reg_with_titanfall();
        let fs = FakeFs::new();
        assert!(scan_registry(&reg, &fs).unwrap().is_empty());
    }

    #[test]
    fn skips_entry_without_install_dir() {
        let reg = FakeRegistry::new().with_subkeys(Hive::LocalMachine, ORIGIN_GAMES_KEY, &["x"]);
        let fs = FakeFs::new();
        assert!(scan_registry(&reg, &fs).unwrap().is_empty());
    }

    #[test]
    fn empty_when_ea_not_installed() {
        assert!(scan_registry(&FakeRegistry::new(), &FakeFs::new())
            .unwrap()
            .is_empty());
    }
}
