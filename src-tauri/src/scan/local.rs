//! Local / non-store game discovery (plan Task 13, M1).
//!
//! Source: **user-configured watch folders** (Settings → "Watch folders").
//! Each immediate **subfolder** of a watch folder is treated as one game, named
//! after the folder; its executable is found by a **bounded depth-first search**
//! (up to [`MAX_FOLDER_DEPTH`] levels), because real installs nest the binary
//! under engine/arch subfolders — e.g. `Game\Binaries\Win64\Game.exe` (Unreal),
//! `Game\game\Game.exe`, not just `Game\Game.exe`. Loose top-level `.exe`s are
//! also surfaced (portable games). Reparse points are skipped (junction
//! loops/double-counting), as are redistributable/uninstall subtrees, and
//! installer/runtime/helper/trainer exes are filtered (see [`is_game_exe`]) so we
//! surface games, not setup utilities.
//!
//! We intentionally do **not** enumerate the Windows Uninstall registry: it
//! lists *every* installed program (browsers, tools, runtimes…), which a
//! keyword filter can't reliably separate from games — it produced false
//! positives. Watch folders are user-curated, so what shows up is what the user
//! points at. Steam/Epic/GOG cover store games; this covers manual/portable installs.
//!
//! Launching a local game spawns its exe via argv with path validation
//! (`launch::validate_local_exe`, threat-model TB3).

use crate::error::CoreResult;
use crate::models::{Game, Source};
use crate::os::FileSystem;
use crate::scan::drive_of;
use std::path::{Path, PathBuf};

/// How many directory levels below a watch-folder subfolder we search for the
/// game exe. The common Unreal layout `Game\<Title>\Binaries\Win64\Game.exe`
/// puts the binary 3 levels deep, so one level (the old behaviour) missed it; 4
/// covers that while bounding the walk on pathological trees.
const MAX_FOLDER_DEPTH: usize = 4;

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

    /// Scan one watch folder: each immediate subfolder is one game (named after
    /// the folder, exe found by deep search), plus any loose top-level game
    /// `.exe`. Reparse points are skipped.
    fn scan_watch_dir(&self, dir: &Path, games: &mut Vec<Game>) {
        let Ok(entries) = self.fs.read_dir(dir) else {
            return;
        };
        for entry in entries {
            if entry.is_reparse_point {
                continue;
            }
            if !entry.is_dir {
                // Loose portable exe at the watch root: name from the exe stem
                // (there's no game folder to name it after).
                if is_game_exe(&entry.path) {
                    games.push(local_game(exe_stem_name(&entry.path), dir, &entry.path));
                }
                continue;
            }
            // A subfolder is one game named after the folder; its exe may sit
            // several levels deep (engine/arch subfolders).
            if let Some(exe) = self.pick_folder_exe(&entry.path) {
                games.push(local_game(folder_name(&entry.path), &entry.path, &exe));
            }
        }
    }

    /// Choose the single most game-like `.exe` anywhere under `folder` (bounded to
    /// [`MAX_FOLDER_DEPTH`]). Prefer one whose name matches the folder (e.g.
    /// `PRAGMATA\PRAGMATA.exe`, `Mafia\…\Win64\Mafia.exe`), else the **shallowest**
    /// non-utility exe (a top-level `Launcher.exe` beats a deep helper). `None` if
    /// the folder has no plausible game exe.
    fn pick_folder_exe(&self, folder: &Path) -> Option<PathBuf> {
        let mut candidates: Vec<(usize, PathBuf)> = Vec::new();
        self.collect_exes(folder, 0, &mut candidates);

        let folder_key = name_key_of(folder);
        if !folder_key.is_empty() {
            // Folder-name match wins at any depth (shallowest such, stable order).
            if let Some((_, p)) = candidates
                .iter()
                .filter(|(_, p)| stem_key(p) == folder_key)
                .min_by_key(|(depth, _)| *depth)
            {
                return Some(p.clone());
            }
        }
        // Else the shallowest candidate (ties keep first-seen / scanner order).
        candidates
            .into_iter()
            .min_by_key(|(depth, _)| *depth)
            .map(|(_, p)| p)
    }

    /// Depth-first collect of game `.exe`s under `dir`, tagged with their depth
    /// (files directly in `dir` are depth 0). Skips reparse points and
    /// redistributable/uninstall subtrees, and stops descending past
    /// [`MAX_FOLDER_DEPTH`].
    fn collect_exes(&self, dir: &Path, depth: usize, out: &mut Vec<(usize, PathBuf)>) {
        let Ok(entries) = self.fs.read_dir(dir) else {
            return;
        };
        for entry in entries {
            if entry.is_reparse_point {
                continue;
            }
            if entry.is_dir {
                if depth < MAX_FOLDER_DEPTH && !is_skippable_dir(&entry.path) {
                    self.collect_exes(&entry.path, depth + 1, out);
                }
            } else if is_game_exe(&entry.path) {
                out.push((depth, entry.path));
            }
        }
    }
}

/// Substrings (in a lowercased exe stem) that mark a non-game executable:
/// installers, redistributables, crash handlers, anti-cheat/EOS bootstrappers,
/// launch helpers, and cheat trainers. Kept specific (`crashhandler`, not
/// `crash`) so real titles like "Crash Bandicoot" aren't filtered. Note: a
/// folder-name match still wins over this list, so a game whose own exe happens
/// to contain one of these tokens is still found by name.
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
    "bootstrapper",
    "helper",
    "trainer",
];

/// Lowercased names of subdirectories we never descend into: redistributable
/// payloads and uninstall data. Skipping them bounds the walk and keeps a stray
/// bundled installer exe from ever being considered. (Engine/`Binaries`/`Win64`
/// are NOT here — real game exes live there.)
const SKIP_DIRS: &[&str] = &[
    "_commonredist",
    "commonredist",
    "redist",
    "vcredist",
    "directx",
    "dotnet",
    "_uninstall",
    "uninstall",
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

/// Whether to skip recursing into a directory (redist/uninstall payloads).
fn is_skippable_dir(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    SKIP_DIRS.iter().any(|d| name == *d)
}

/// Lowercased, alphanumeric-only key for fuzzy name matching (so "Hollow Knight"
/// ≈ "hollow_knight" ≈ "HollowKnight").
fn name_key(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Fuzzy key of a path's final component (folder name).
fn name_key_of(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(name_key)
        .unwrap_or_default()
}

/// Fuzzy key of an exe's file stem.
fn stem_key(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(name_key)
        .unwrap_or_default()
}

/// Display name for a game folder (the folder's own name, as the user sees it in
/// Explorer — far better than a picked exe stem like `re9` or `Launcher`).
fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string()
}

/// Display name for a loose top-level exe (its file stem — no folder to name it).
fn exe_stem_name(exe: &Path) -> String {
    exe.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string()
}

/// A `Game` built from a discovered exe. `install_path` is the game's folder
/// (the watch folder itself for a loose top-level exe); `external_id` is the full
/// exe path (a stable unique key).
fn local_game(name: String, folder: &Path, exe: &Path) -> Game {
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
    fn filters_installers_and_names_game_after_its_folder() {
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
            ["Hollow Knight"],
            "named after the folder, not the exe stem; installers/helpers filtered"
        );
        assert_eq!(
            games[0].exe_path.as_deref(),
            Some(r"D:\Games\Hollow Knight\hollow_knight.exe")
        );
    }

    #[test]
    fn finds_exe_nested_several_levels_deep() {
        // Real Unreal layout: Watch\Mafia\MafiaTheOldCountry\Binaries\Win64\Mafia.exe
        let watch = r"D:\Games";
        let fs = FakeFs::new()
            .with_dir(watch, vec![entry(r"D:\Games\Mafia", true, false)])
            .with_dir(
                r"D:\Games\Mafia",
                vec![
                    entry(r"D:\Games\Mafia\MafiaTheOldCountry", true, false),
                    entry(r"D:\Games\Mafia\Uninstall", true, false),
                ],
            )
            .with_dir(
                r"D:\Games\Mafia\MafiaTheOldCountry",
                vec![entry(
                    r"D:\Games\Mafia\MafiaTheOldCountry\Binaries",
                    true,
                    false,
                )],
            )
            .with_dir(
                r"D:\Games\Mafia\MafiaTheOldCountry\Binaries",
                vec![entry(
                    r"D:\Games\Mafia\MafiaTheOldCountry\Binaries\Win64",
                    true,
                    false,
                )],
            )
            .with_dir(
                r"D:\Games\Mafia\MafiaTheOldCountry\Binaries\Win64",
                vec![
                    entry(
                        r"D:\Games\Mafia\MafiaTheOldCountry\Binaries\Win64\Mafia.exe",
                        false,
                        false,
                    ),
                    entry(
                        r"D:\Games\Mafia\MafiaTheOldCountry\Binaries\Win64\CrashReportClient.exe",
                        false,
                        false,
                    ),
                ],
            )
            // The uninstall subtree must be skipped, even though it holds an exe.
            .with_dir(
                r"D:\Games\Mafia\Uninstall",
                vec![entry(
                    r"D:\Games\Mafia\Uninstall\unins000.exe",
                    false,
                    false,
                )],
            );
        let games = LocalScanner::new(&fs)
            .scan(&[PathBuf::from(watch)])
            .unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Mafia");
        assert_eq!(games[0].install_path, r"D:\Games\Mafia");
        assert_eq!(
            games[0].exe_path.as_deref(),
            Some(r"D:\Games\Mafia\MafiaTheOldCountry\Binaries\Win64\Mafia.exe"),
            "the deep game exe, not the crash reporter or the uninstaller"
        );
    }

    #[test]
    fn folder_name_match_beats_a_shallower_helper() {
        // A launcher sits at the top; the real game (matching the folder) is one
        // level down. The name match should win over the shallower launcher.
        let watch = r"D:\Games";
        let fs = FakeFs::new()
            .with_dir(watch, vec![entry(r"D:\Games\LumenTale", true, false)])
            .with_dir(
                r"D:\Games\LumenTale",
                vec![entry(r"D:\Games\LumenTale\game", true, false)],
            )
            .with_dir(
                r"D:\Games\LumenTale\game",
                vec![
                    entry(r"D:\Games\LumenTale\game\EOSBootstrapper.exe", false, false),
                    entry(r"D:\Games\LumenTale\game\LumenTale.exe", false, false),
                ],
            );
        let games = LocalScanner::new(&fs)
            .scan(&[PathBuf::from(watch)])
            .unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "LumenTale");
        assert_eq!(
            games[0].exe_path.as_deref(),
            Some(r"D:\Games\LumenTale\game\LumenTale.exe")
        );
    }

    #[test]
    fn picks_shallowest_when_no_name_match() {
        // No exe matches the folder name → the shallowest non-utility exe (the
        // top-level launcher) is the launch target.
        let watch = r"D:\Games";
        let fs = FakeFs::new()
            .with_dir(watch, vec![entry(r"D:\Games\Neverness", true, false)])
            .with_dir(
                r"D:\Games\Neverness",
                vec![
                    entry(r"D:\Games\Neverness\NTEGlobalLauncher.exe", false, false),
                    entry(r"D:\Games\Neverness\uninst.exe", false, false),
                    entry(r"D:\Games\Neverness\NTEGlobal", true, false),
                ],
            )
            .with_dir(
                r"D:\Games\Neverness\NTEGlobal",
                vec![entry(
                    r"D:\Games\Neverness\NTEGlobal\NTEGlobalGame.exe",
                    false,
                    false,
                )],
            );
        let games = LocalScanner::new(&fs)
            .scan(&[PathBuf::from(watch)])
            .unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Neverness");
        assert_eq!(
            games[0].exe_path.as_deref(),
            Some(r"D:\Games\Neverness\NTEGlobalLauncher.exe"),
            "shallowest exe wins; the deep one is not preferred"
        );
    }

    #[test]
    fn loose_top_level_trainer_exe_is_not_a_game() {
        let watch = r"D:\Games";
        let fs = FakeFs::new().with_dir(
            watch,
            vec![entry(
                r"D:\Games\PRAGMATA v1.0 Plus 27 Trainer.exe",
                false,
                false,
            )],
        );
        let games = LocalScanner::new(&fs)
            .scan(&[PathBuf::from(watch)])
            .unwrap();
        assert!(games.is_empty(), "cheat trainers are not games");
    }

    #[test]
    fn subfolder_with_only_redist_yields_no_game() {
        let watch = r"D:\Games";
        let fs = FakeFs::new()
            .with_dir(watch, vec![entry(r"D:\Games\Thing", true, false)])
            .with_dir(
                r"D:\Games\Thing",
                vec![entry(r"D:\Games\Thing\_CommonRedist", true, false)],
            )
            // Skipped dir — its exe (even with a game-ish name) must never be picked.
            .with_dir(
                r"D:\Games\Thing\_CommonRedist",
                vec![entry(
                    r"D:\Games\Thing\_CommonRedist\Thing.exe",
                    false,
                    false,
                )],
            );
        let games = LocalScanner::new(&fs)
            .scan(&[PathBuf::from(watch)])
            .unwrap();
        assert!(
            games.is_empty(),
            "redist subtree is skipped, so no exe is found"
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
