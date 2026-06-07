//! Steam library discovery (plan Task 7, M1).
//!
//! Pipeline: registry `SteamPath` → `<steam>\steamapps\libraryfolders.vdf` →
//! each library's `steamapps\appmanifest_*.acf` → [`Game`] tagged
//! [`Source::Steam`]. All OS access goes through the [`Registry`]/[`FileSystem`]
//! seams (ADR-0005), so the discovery logic is unit-tested against in-memory
//! fakes on any OS; VDF parsing is delegated to the [`crate::scan::vdf`] adapter
//! (ADR-0003).
//!
//! Robustness (threat-model TB1): untrusted on-disk data never panics or aborts
//! the whole scan. A missing Steam install yields an empty list (not an error);
//! a single unreadable directory or malformed manifest is skipped, not fatal.

use crate::error::CoreResult;
use crate::models::{Game, Source};
use crate::os::{FileSystem, Hive, Registry};
use crate::scan::{drive_of, is_not_found, vdf};
use std::path::Path;

/// HKCU key holding the per-user Steam install path (`SteamPath`).
const STEAM_KEY_HKCU: &str = r"Software\Valve\Steam";
/// HKLM (WOW6432 view) key holding the machine-wide install path (`InstallPath`).
const STEAM_KEY_HKLM: &str = r"SOFTWARE\WOW6432Node\Valve\Steam";

/// Discovers installed Steam games via the OS seams. Borrows its dependencies so
/// the orchestrator (Task 8) can share one set of OS adapters across scanners.
pub struct SteamScanner<'a> {
    registry: &'a dyn Registry,
    fs: &'a dyn FileSystem,
}

impl<'a> SteamScanner<'a> {
    pub fn new(registry: &'a dyn Registry, fs: &'a dyn FileSystem) -> Self {
        Self { registry, fs }
    }

    /// Discover every installed Steam game across all library folders. Returns an
    /// empty list when Steam isn't installed (no registry path).
    pub fn scan(&self) -> CoreResult<Vec<Game>> {
        let Some(steam_root) = self.steam_root()? else {
            return Ok(Vec::new());
        };
        let folders = self.read_library_folders(&steam_root)?;
        let mut games = Vec::new();
        for folder in &folders {
            self.collect_library_games(Path::new(&folder.path), &mut games);
        }
        Ok(games)
    }

    /// Locate the Steam root (see [`find_steam_root`]).
    fn steam_root(&self) -> CoreResult<Option<String>> {
        find_steam_root(self.registry)
    }

    /// Read + parse `<steam_root>\steamapps\libraryfolders.vdf`. A missing file
    /// means "no libraries" (`Ok(empty)`), not an error — Steam may be installed
    /// with no library yet.
    fn read_library_folders(&self, steam_root: &str) -> CoreResult<Vec<vdf::LibraryFolder>> {
        let vdf_path = Path::new(steam_root)
            .join("steamapps")
            .join("libraryfolders.vdf");
        match self.fs.read_to_string(&vdf_path) {
            Ok(text) => vdf::parse_library_folders(&text),
            Err(e) if is_not_found(&e) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// Append every parseable game in one library root to `games`. An unreadable
    /// `steamapps` directory and individually malformed/unreadable manifests are
    /// skipped (TB1) so one bad entry never drops the rest of the library.
    fn collect_library_games(&self, library: &Path, games: &mut Vec<Game>) {
        let steamapps = library.join("steamapps");
        let Ok(entries) = self.fs.read_dir(&steamapps) else {
            return;
        };
        for entry in entries {
            if entry.is_dir || !is_app_manifest(&entry.path) {
                continue;
            }
            let Ok(text) = self.fs.read_to_string(&entry.path) else {
                continue;
            };
            let Ok(manifest) = vdf::parse_app_manifest(&text) else {
                continue;
            };
            games.push(game_from_manifest(&steamapps, &manifest));
        }
    }
}

/// Locate the Steam install root via the registry: HKCU `SteamPath` first
/// (per-user, the common case), then HKLM `InstallPath` (machine-wide fallback).
/// `Ok(None)` when Steam isn't installed. Shared by the scanner (Task 7) and
/// cover-art resolution (Task 10).
pub fn find_steam_root(registry: &dyn Registry) -> CoreResult<Option<String>> {
    if let Some(p) = registry.read_string(Hive::CurrentUser, STEAM_KEY_HKCU, "SteamPath")? {
        return Ok(Some(p));
    }
    registry.read_string(Hive::LocalMachine, STEAM_KEY_HKLM, "InstallPath")
}

/// `appmanifest_<id>.acf` filename test (the per-game manifests Steam writes into
/// each `steamapps` directory). Excludes siblings like `appworkshop_*.acf`.
fn is_app_manifest(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name.starts_with("appmanifest_") && name.ends_with(".acf"))
}

/// Build a [`Game`] from a parsed manifest. `install_path` is
/// `<steamapps>\common\<installdir>`; `drive` is its drive prefix (e.g. `"C:"`).
/// `id`/`exe_path`/`cover_path`/`last_played` are filled by later stages (persist,
/// launch, art); `size_bytes` comes from the manifest's `SizeOnDisk`.
fn game_from_manifest(steamapps: &Path, m: &vdf::AppManifest) -> Game {
    let install_path = steamapps.join("common").join(&m.installdir);
    let drive = drive_of(&install_path);
    Game {
        id: 0,
        source: Source::Steam,
        external_id: m.appid.to_string(),
        name: m.name.clone(),
        install_path: install_path.to_string_lossy().into_owned(),
        exe_path: None,
        // SizeOnDisk (u64) can in theory exceed i64; a real game install never
        // will, but cast fallibly rather than risk a wrap.
        size_bytes: i64::try_from(m.size_on_disk).ok(),
        drive,
        last_played: None,
        launch_count: 0,
        cover_path: None,
        favorite: false,
        tags: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::fs::FakeFs;
    use crate::os::registry::FakeRegistry;
    use crate::os::DirEntryInfo;
    use std::path::PathBuf;

    const LIBRARYFOLDERS: &str = include_str!("../../tests/fixtures/steam/libraryfolders.vdf");
    const TF2: &str = include_str!("../../tests/fixtures/steam/appmanifest_440.acf");
    const DOTA: &str = include_str!("../../tests/fixtures/steam/appmanifest_570.acf");
    const WARFRAME: &str = include_str!("../../tests/fixtures/steam/appmanifest_230410.acf");

    const STEAM_ROOT: &str = r"C:\Program Files (x86)\Steam";
    const LIB2_ROOT: &str = r"D:\SteamLibrary";

    fn file_entry(path: &str) -> DirEntryInfo {
        DirEntryInfo {
            path: PathBuf::from(path),
            is_dir: false,
            is_reparse_point: false,
        }
    }

    fn dir_entry(path: &str) -> DirEntryInfo {
        DirEntryInfo {
            path: PathBuf::from(path),
            is_dir: true,
            is_reparse_point: false,
        }
    }

    /// Fakes wired to mirror a real two-library Steam install (main lib on C:
    /// with TF2 + Dota; a second lib on D: with Warframe). The C: `steamapps`
    /// also holds non-manifest noise to exercise the filter.
    fn installed_steam() -> (FakeRegistry, FakeFs) {
        let reg = FakeRegistry::new().with_value(
            Hive::CurrentUser,
            STEAM_KEY_HKCU,
            "SteamPath",
            STEAM_ROOT,
        );

        let lib1 = format!(r"{STEAM_ROOT}\steamapps");
        let lib2 = format!(r"{LIB2_ROOT}\steamapps");

        let fs = FakeFs::new()
            .with_file(format!(r"{lib1}\libraryfolders.vdf"), LIBRARYFOLDERS)
            .with_dir(
                lib1.as_str(),
                vec![
                    file_entry(&format!(r"{lib1}\appmanifest_440.acf")),
                    file_entry(&format!(r"{lib1}\appmanifest_570.acf")),
                    // Noise that must be ignored:
                    file_entry(&format!(r"{lib1}\appworkshop_440.acf")),
                    file_entry(&format!(r"{lib1}\libraryfolder.vdf")),
                    dir_entry(&format!(r"{lib1}\common")),
                ],
            )
            .with_file(format!(r"{lib1}\appmanifest_440.acf"), TF2)
            .with_file(format!(r"{lib1}\appmanifest_570.acf"), DOTA)
            .with_dir(
                lib2.as_str(),
                vec![file_entry(&format!(r"{lib2}\appmanifest_230410.acf"))],
            )
            .with_file(format!(r"{lib2}\appmanifest_230410.acf"), WARFRAME);

        (reg, fs)
    }

    fn find<'g>(games: &'g [Game], external_id: &str) -> &'g Game {
        games
            .iter()
            .find(|g| g.external_id == external_id)
            .unwrap_or_else(|| panic!("no game with external_id {external_id}"))
    }

    #[test]
    fn scan_discovers_games_from_all_libraries() {
        let (reg, fs) = installed_steam();
        let games = SteamScanner::new(&reg, &fs).scan().unwrap();
        let mut ids: Vec<&str> = games.iter().map(|g| g.external_id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["230410", "440", "570"]);
    }

    #[test]
    fn scan_tags_every_game_as_steam() {
        let (reg, fs) = installed_steam();
        let games = SteamScanner::new(&reg, &fs).scan().unwrap();
        assert!(games.iter().all(|g| g.source == Source::Steam));
    }

    #[test]
    fn scan_reads_name_from_manifest() {
        let (reg, fs) = installed_steam();
        let games = SteamScanner::new(&reg, &fs).scan().unwrap();
        assert_eq!(find(&games, "440").name, "Team Fortress 2");
    }

    #[test]
    fn scan_builds_install_path_under_library_common() {
        let (reg, fs) = installed_steam();
        let games = SteamScanner::new(&reg, &fs).scan().unwrap();
        assert_eq!(
            find(&games, "230410").install_path,
            r"D:\SteamLibrary\steamapps\common\Warframe"
        );
    }

    #[test]
    fn scan_derives_drive_from_library_root() {
        let (reg, fs) = installed_steam();
        let games = SteamScanner::new(&reg, &fs).scan().unwrap();
        assert_eq!(find(&games, "440").drive.as_deref(), Some("C:"));
        assert_eq!(find(&games, "230410").drive.as_deref(), Some("D:"));
    }

    #[test]
    fn scan_takes_size_from_size_on_disk() {
        let (reg, fs) = installed_steam();
        let games = SteamScanner::new(&reg, &fs).scan().unwrap();
        // > u32::MAX — proves the u64 path round-trips through i64.
        assert_eq!(find(&games, "440").size_bytes, Some(24_305_820_742));
    }

    #[test]
    fn scan_leaves_persistence_and_art_fields_unset() {
        let (reg, fs) = installed_steam();
        let games = SteamScanner::new(&reg, &fs).scan().unwrap();
        let g = find(&games, "440");
        assert_eq!(g.id, 0);
        assert_eq!(g.launch_count, 0);
        assert!(!g.favorite);
        assert!(g.exe_path.is_none() && g.cover_path.is_none() && g.last_played.is_none());
    }

    #[test]
    fn scan_returns_empty_when_steam_not_installed() {
        let reg = FakeRegistry::new();
        let fs = FakeFs::new();
        let games = SteamScanner::new(&reg, &fs).scan().unwrap();
        assert!(games.is_empty());
    }

    #[test]
    fn scan_falls_back_to_hklm_install_path() {
        let reg = FakeRegistry::new().with_value(
            Hive::LocalMachine,
            STEAM_KEY_HKLM,
            "InstallPath",
            STEAM_ROOT,
        );
        let lib1 = format!(r"{STEAM_ROOT}\steamapps");
        let fs = FakeFs::new()
            .with_file(format!(r"{lib1}\libraryfolders.vdf"), LIBRARYFOLDERS)
            .with_dir(
                lib1.as_str(),
                vec![file_entry(&format!(r"{lib1}\appmanifest_440.acf"))],
            )
            // libraryfolders.vdf lists a D: library; leave it unseeded → skipped.
            .with_file(format!(r"{lib1}\appmanifest_440.acf"), TF2);
        let games = SteamScanner::new(&reg, &fs).scan().unwrap();
        assert_eq!(find(&games, "440").name, "Team Fortress 2");
    }

    #[test]
    fn scan_returns_empty_when_libraryfolders_missing() {
        // Steam path is set, but the vdf file isn't there → no libraries, no error.
        let reg = FakeRegistry::new().with_value(
            Hive::CurrentUser,
            STEAM_KEY_HKCU,
            "SteamPath",
            STEAM_ROOT,
        );
        let fs = FakeFs::new();
        let games = SteamScanner::new(&reg, &fs).scan().unwrap();
        assert!(games.is_empty());
    }

    #[test]
    fn scan_skips_malformed_manifest_but_keeps_siblings() {
        let reg = FakeRegistry::new().with_value(
            Hive::CurrentUser,
            STEAM_KEY_HKCU,
            "SteamPath",
            STEAM_ROOT,
        );
        let lib1 = format!(r"{STEAM_ROOT}\steamapps");
        let fs = FakeFs::new()
            .with_file(format!(r"{lib1}\libraryfolders.vdf"), LIBRARYFOLDERS)
            .with_dir(
                lib1.as_str(),
                vec![
                    file_entry(&format!(r"{lib1}\appmanifest_440.acf")),
                    file_entry(&format!(r"{lib1}\appmanifest_999.acf")),
                ],
            )
            .with_file(format!(r"{lib1}\appmanifest_440.acf"), TF2)
            .with_file(
                format!(r"{lib1}\appmanifest_999.acf"),
                "\"AppState\" { \"appid\" ",
            );
        let games = SteamScanner::new(&reg, &fs).scan().unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].external_id, "440");
    }

    #[test]
    fn is_app_manifest_matches_only_appmanifest_acf() {
        assert!(is_app_manifest(Path::new(r"C:\s\appmanifest_440.acf")));
        assert!(!is_app_manifest(Path::new(r"C:\s\appworkshop_440.acf")));
        assert!(!is_app_manifest(Path::new(r"C:\s\appmanifest_440.txt")));
        assert!(!is_app_manifest(Path::new(r"C:\s\libraryfolders.vdf")));
    }
}
