//! Epic Games discovery (plan Task 12, M1).
//!
//! Epic records each installed title as a JSON `*.item` manifest under
//! `%PROGRAMDATA%\Epic\EpicGamesLauncher\Data\Manifests`. We parse those into
//! [`Game`]s tagged [`Source::Epic`]; launching uses the
//! `com.epicgames.launcher://` protocol (see `launch::epic_launch_url`), so the
//! `external_id` is the manifest `AppName`. All filesystem access goes through
//! the [`FileSystem`] seam (ADR-0005) for OS-independent unit tests.
//!
//! Robustness (TB1): a missing Manifests dir → empty (Epic not installed); a
//! single malformed/unreadable `.item` is skipped, never aborting the scan.

use crate::error::CoreResult;
use crate::models::{Game, Source};
use crate::os::FileSystem;
use crate::scan::{drive_of, is_not_found};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The subset of an Epic `*.item` manifest the launcher needs. Unknown keys
/// (FormatVersion, CatalogItemId, …) are ignored.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct EpicManifest {
    app_name: String,
    #[serde(default)]
    display_name: String,
    install_location: String,
    #[serde(default)]
    launch_executable: String,
    #[serde(default)]
    install_size: i64,
}

/// Discovers installed Epic games via the [`FileSystem`] seam.
pub struct EpicScanner<'a> {
    fs: &'a dyn FileSystem,
}

impl<'a> EpicScanner<'a> {
    pub fn new(fs: &'a dyn FileSystem) -> Self {
        Self { fs }
    }

    /// Parse every `*.item` manifest in `manifests_dir` into a [`Game`]. Empty
    /// when the directory is absent (Epic not installed).
    pub fn scan(&self, manifests_dir: &Path) -> CoreResult<Vec<Game>> {
        let entries = match self.fs.read_dir(manifests_dir) {
            Ok(entries) => entries,
            Err(e) if is_not_found(&e) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut games = Vec::new();
        for entry in entries {
            if entry.is_dir || !is_item_file(&entry.path) {
                continue;
            }
            let Ok(text) = self.fs.read_to_string(&entry.path) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<EpicManifest>(&text) else {
                continue;
            };
            if let Some(game) = game_from_manifest(manifest) {
                // Skip stale manifests whose install folder no longer exists.
                // Epic leaves the `*.item` file behind after a game is
                // uninstalled (and after the launcher itself is removed), which
                // otherwise surfaces phantom games (e.g. a long-gone Fortnite).
                if self.install_exists(&game.install_path) {
                    games.push(game);
                } else {
                    tracing::debug!(
                        name = %game.name,
                        path = %game.install_path,
                        "skipping Epic manifest: install folder is gone"
                    );
                }
            }
        }
        Ok(games)
    }

    /// Whether a manifest's install location still exists as a directory on
    /// disk. A missing path (uninstalled) or a non-directory both count as gone.
    fn install_exists(&self, install_path: &str) -> bool {
        self.fs
            .metadata(Path::new(install_path))
            .map(|m| m.is_dir)
            .unwrap_or(false)
    }
}

/// `*.item` filename test (case-insensitive extension).
fn is_item_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("item"))
}

/// Build a [`Game`] from a manifest. `None` when it lacks the fields needed to
/// launch (app name) or locate it (install location). `external_id` is the Epic
/// `AppName` (the launch-protocol key); `name` falls back to `AppName` when
/// `DisplayName` is empty. `exe_path` (for cover-art icon extraction) is the
/// install location joined with the relative launch executable.
fn game_from_manifest(m: EpicManifest) -> Option<Game> {
    if m.app_name.is_empty() || m.install_location.is_empty() {
        return None;
    }
    let install = PathBuf::from(&m.install_location);
    let drive = drive_of(&install);
    // `name` falls back to AppName only when DisplayName is empty; otherwise move
    // DisplayName. `install` is an independent owned copy, so install_location and
    // app_name can be moved into the struct without cloning.
    let name = if m.display_name.is_empty() {
        m.app_name.clone()
    } else {
        m.display_name
    };
    let exe_path = (!m.launch_executable.is_empty()).then(|| {
        install
            .join(&m.launch_executable)
            .to_string_lossy()
            .into_owned()
    });
    Some(Game {
        id: 0,
        source: Source::Epic,
        external_id: m.app_name,
        name,
        install_path: m.install_location,
        exe_path,
        size_bytes: (m.install_size > 0).then_some(m.install_size),
        drive,
        last_played: None,
        launch_count: 0,
        cover_path: None,
        favorite: false,
        tags: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::fs::FakeFs;
    use crate::os::DirEntryInfo;

    const FORTNITE: &str = include_str!("../../tests/fixtures/epic/Fortnite.item");
    const SATISFACTORY: &str = include_str!("../../tests/fixtures/epic/Satisfactory.item");

    const DIR: &str = r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests";

    fn file_entry(path: &str) -> DirEntryInfo {
        DirEntryInfo {
            path: PathBuf::from(path),
            is_dir: false,
            is_reparse_point: false,
        }
    }

    fn installed_epic() -> FakeFs {
        FakeFs::new()
            .with_dir(
                DIR,
                vec![
                    file_entry(&format!(r"{DIR}\Fortnite.item")),
                    file_entry(&format!(r"{DIR}\Satisfactory.item")),
                    file_entry(&format!(r"{DIR}\manifests.installmanifest")), // noise
                ],
            )
            .with_file(format!(r"{DIR}\Fortnite.item"), FORTNITE)
            .with_file(format!(r"{DIR}\Satisfactory.item"), SATISFACTORY)
            // Install folders must exist or the scan treats them as uninstalled.
            .with_dir(r"C:\Program Files\Epic Games\Fortnite", vec![])
            .with_dir(r"D:\Epic\Satisfactory", vec![])
    }

    fn find<'g>(games: &'g [Game], external_id: &str) -> &'g Game {
        games
            .iter()
            .find(|g| g.external_id == external_id)
            .unwrap_or_else(|| panic!("no game with external_id {external_id}"))
    }

    #[test]
    fn scan_discovers_item_manifests_only() {
        let fs = installed_epic();
        let games = EpicScanner::new(&fs).scan(Path::new(DIR)).unwrap();
        assert_eq!(games.len(), 2, "the .installmanifest noise is ignored");
        assert!(games.iter().all(|g| g.source == Source::Epic));
    }

    #[test]
    fn scan_uses_appname_as_external_id_and_displayname_as_name() {
        let fs = installed_epic();
        let games = EpicScanner::new(&fs).scan(Path::new(DIR)).unwrap();
        let sat = find(&games, "CrabEmoji"); // AppName, not the display name
        assert_eq!(sat.name, "Satisfactory");
        assert_eq!(sat.install_path, r"D:\Epic\Satisfactory");
        assert_eq!(sat.drive.as_deref(), Some("D:"));
        assert_eq!(sat.size_bytes, Some(53_687_091_200));
    }

    #[test]
    fn scan_sets_exe_path_under_install_location() {
        let fs = installed_epic();
        let games = EpicScanner::new(&fs).scan(Path::new(DIR)).unwrap();
        let exe = find(&games, "Fortnite").exe_path.clone().unwrap();
        assert!(exe.starts_with(r"C:\Program Files\Epic Games\Fortnite"));
        assert!(exe
            .to_lowercase()
            .ends_with("fortniteclient-win64-shipping.exe"));
    }

    #[test]
    fn scan_skips_manifest_when_install_folder_is_gone() {
        // The manifest is present and valid, but its install folder was never
        // seeded (i.e. the game was uninstalled, leaving the *.item behind).
        let fs = FakeFs::new()
            .with_dir(DIR, vec![file_entry(&format!(r"{DIR}\Fortnite.item"))])
            .with_file(format!(r"{DIR}\Fortnite.item"), FORTNITE);
        // (no with_dir for C:\Program Files\Epic Games\Fortnite)
        let games = EpicScanner::new(&fs).scan(Path::new(DIR)).unwrap();
        assert!(
            games.is_empty(),
            "a manifest with a missing install folder is treated as uninstalled"
        );
    }

    #[test]
    fn scan_returns_empty_when_manifests_dir_missing() {
        let fs = FakeFs::new();
        let games = EpicScanner::new(&fs).scan(Path::new(DIR)).unwrap();
        assert!(games.is_empty());
    }

    #[test]
    fn scan_skips_malformed_item_but_keeps_valid() {
        let fs = FakeFs::new()
            .with_dir(
                DIR,
                vec![
                    file_entry(&format!(r"{DIR}\Fortnite.item")),
                    file_entry(&format!(r"{DIR}\Broken.item")),
                ],
            )
            .with_file(format!(r"{DIR}\Fortnite.item"), FORTNITE)
            .with_file(format!(r"{DIR}\Broken.item"), "{ not valid json ")
            .with_dir(r"C:\Program Files\Epic Games\Fortnite", vec![]);
        let games = EpicScanner::new(&fs).scan(Path::new(DIR)).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].external_id, "Fortnite");
    }
}
