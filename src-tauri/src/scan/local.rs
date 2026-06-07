//! Local / non-store game discovery (plan Task 13, M1).
//!
//! Source: **user-configured watch folders** (Settings → "Watch folders"),
//! scanned for `.exe`s — top level plus one directory deep (the common
//! `Folder\Game\Game.exe` layout), skipping reparse points to avoid junction
//! loops/double-counting.
//!
//! We intentionally do **not** enumerate the Windows Uninstall registry: it
//! lists *every* installed program (browsers, tools, runtimes…), which a
//! keyword filter can't reliably separate from games — it produced false
//! positives. Watch folders are user-curated, so what shows up is what the user
//! points at. Steam/Epic cover store games; this covers manual/portable installs.
//!
//! Launching a local game spawns its exe via argv with path validation
//! (`launch::validate_local_exe`, threat-model TB3).

use crate::error::CoreResult;
use crate::models::{Game, Source};
use crate::os::FileSystem;
use crate::scan::drive_of;
use std::path::{Path, PathBuf};

/// Discovers local games from watch folders via the [`FileSystem`] seam.
pub struct LocalScanner<'a> {
    fs: &'a dyn FileSystem,
}

impl<'a> LocalScanner<'a> {
    pub fn new(fs: &'a dyn FileSystem) -> Self {
        Self { fs }
    }

    /// Discover `.exe` games under each watch folder. A missing/unreadable folder
    /// is skipped (TB1), not fatal.
    pub fn scan(&self, watch_folders: &[PathBuf]) -> CoreResult<Vec<Game>> {
        let mut games = Vec::new();
        for dir in watch_folders {
            self.scan_watch_dir(dir, &mut games);
        }
        Ok(games)
    }

    /// Scan one watch folder: top-level game `.exe`s plus, for each immediate
    /// subfolder, a single best game exe. Reparse points are skipped (junction
    /// loops/double-counting); installer/runtime/crash-handler exes are filtered
    /// out (see [`is_game_exe`]) so we surface games, not setup utilities.
    fn scan_watch_dir(&self, dir: &Path, games: &mut Vec<Game>) {
        let Ok(entries) = self.fs.read_dir(dir) else {
            return;
        };
        for entry in entries {
            if entry.is_reparse_point {
                continue;
            }
            if !entry.is_dir {
                if is_game_exe(&entry.path) {
                    games.push(local_exe_game(dir, &entry.path));
                }
                continue;
            }
            // One main exe per subfolder (the common `Folder\Game\Game.exe`).
            if let Some(exe) = self.pick_folder_exe(&entry.path) {
                games.push(local_exe_game(&entry.path, &exe));
            }
        }
    }

    /// Choose the single most game-like `.exe` in `folder`: prefer one whose name
    /// matches the folder (e.g. `Hollow Knight\hollow_knight.exe`), else the first
    /// non-utility exe. `None` if the folder has no plausible game exe.
    fn pick_folder_exe(&self, folder: &Path) -> Option<PathBuf> {
        let Ok(children) = self.fs.read_dir(folder) else {
            return None;
        };
        let candidates: Vec<PathBuf> = children
            .into_iter()
            .filter(|c| !c.is_dir && !c.is_reparse_point && is_game_exe(&c.path))
            .map(|c| c.path)
            .collect();
        let folder_key = folder
            .file_name()
            .and_then(|s| s.to_str())
            .map(name_key)
            .filter(|k| !k.is_empty());
        if let Some(key) = folder_key {
            if let Some(matched) = candidates.iter().find(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(name_key)
                    .as_deref()
                    == Some(&key)
            }) {
                return Some(matched.clone());
            }
        }
        candidates.into_iter().next()
    }
}

/// Substrings (in a lowercased exe stem) that mark a non-game executable:
/// installers, redistributables, crash handlers, anti-cheat bootstrappers. Kept
/// specific (`crashhandler`, not `crash`) so real titles like "Crash Bandicoot"
/// aren't filtered.
const NON_GAME_EXE: &[&str] = &[
    "unins",
    "uninstall",
    "setup",
    "install",
    "redist",
    "vcredist",
    "vc_redist",
    "vcruntime",
    "dxsetup",
    "dxwebsetup",
    "directx",
    "dotnet",
    "oalinst",
    "crashhandler",
    "crashreport",
    "crashpad",
    "prereq",
    "touchup",
    "easyanticheat",
    "battleye",
];

/// A `.exe` that looks like a game (not an installer/runtime/helper).
fn is_game_exe(path: &Path) -> bool {
    if !is_exe(path) {
        return false;
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    !NON_GAME_EXE.iter().any(|token| stem.contains(token))
}

/// Lowercased, alphanumeric-only key for fuzzy name matching (so "Hollow Knight"
/// ≈ "hollow_knight" ≈ "HollowKnight").
fn name_key(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// A `Game` built from a discovered exe in a watch folder. `install_path` is the
/// containing folder; `external_id` is the full exe path (a stable unique key).
fn local_exe_game(folder: &Path, exe: &Path) -> Game {
    let name = exe
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();
    Game {
        id: 0,
        source: Source::Local,
        external_id: exe.to_string_lossy().into_owned(),
        name,
        install_path: folder.to_string_lossy().into_owned(),
        exe_path: Some(exe.to_string_lossy().into_owned()),
        size_bytes: None,
        drive: drive_of(exe),
        last_played: None,
        launch_count: 0,
        cover_path: None,
        favorite: false,
        tags: Vec::new(),
    }
}

fn is_exe(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::fs::FakeFs;
    use crate::os::DirEntryInfo;

    fn entry(path: &str, is_dir: bool, reparse: bool) -> DirEntryInfo {
        DirEntryInfo {
            path: PathBuf::from(path),
            is_dir,
            is_reparse_point: reparse,
        }
    }

    #[test]
    fn watch_folder_finds_top_level_and_nested_exes_skipping_junctions() {
        let watch = r"D:\Games";
        let fs = FakeFs::new()
            .with_dir(
                watch,
                vec![
                    entry(r"D:\Games\portable.exe", false, false),
                    entry(r"D:\Games\readme.txt", false, false),
                    entry(r"D:\Games\Celeste", true, false),
                    entry(r"D:\Games\Junction", true, true), // reparse → skipped
                ],
            )
            .with_dir(
                r"D:\Games\Celeste",
                vec![entry(r"D:\Games\Celeste\Celeste.exe", false, false)],
            );
        let games = LocalScanner::new(&fs)
            .scan(&[PathBuf::from(watch)])
            .unwrap();
        let names: std::collections::BTreeSet<&str> =
            games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Celeste", "portable"].into_iter().collect());

        let celeste = games.iter().find(|g| g.name == "Celeste").unwrap();
        assert_eq!(celeste.install_path, r"D:\Games\Celeste");
        assert_eq!(
            celeste.exe_path.as_deref(),
            Some(r"D:\Games\Celeste\Celeste.exe")
        );
        assert_eq!(celeste.source, Source::Local);
    }

    #[test]
    fn filters_installers_and_picks_one_main_exe_per_folder() {
        let watch = r"D:\Games";
        let fs = FakeFs::new()
            .with_dir(
                watch,
                vec![
                    entry(r"D:\Games\vcredist_x64.exe", false, false), // runtime → skip
                    entry(r"D:\Games\Hollow Knight", true, false),
                ],
            )
            .with_dir(
                r"D:\Games\Hollow Knight",
                vec![
                    entry(r"D:\Games\Hollow Knight\hollow_knight.exe", false, false), // the game
                    entry(r"D:\Games\Hollow Knight\unins000.exe", false, false),      // uninstaller
                    entry(
                        r"D:\Games\Hollow Knight\UnityCrashHandler64.exe",
                        false,
                        false,
                    ), // helper
                ],
            );
        let games = LocalScanner::new(&fs)
            .scan(&[PathBuf::from(watch)])
            .unwrap();
        let names: Vec<&str> = games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            ["hollow_knight"],
            "only the game exe, no installers/helpers"
        );
    }

    #[test]
    fn missing_watch_folder_is_ignored() {
        let games = LocalScanner::new(&FakeFs::new())
            .scan(&[PathBuf::from(r"X:\nope")])
            .unwrap();
        assert!(games.is_empty());
    }

    #[test]
    fn no_watch_folders_means_no_local_games() {
        // No registry scanning → an empty config yields nothing (no app noise).
        let games = LocalScanner::new(&FakeFs::new()).scan(&[]).unwrap();
        assert!(games.is_empty());
    }
}
