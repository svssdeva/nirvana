//! Game launching (plan Task 11, FR-LAUNCH).
//!
//! v1 launches Steam games through the **official URL protocol**
//! (`steam://rungameid/<appid>`) opened via `tauri-plugin-opener` — never by
//! spawning the Steam binary (threat-model TB3). Epic uses its own protocol
//! (Task 12); local games spawn their exe via argv with path validation
//! (Task 13). This module owns the pure, testable URL construction; the command
//! layer (`commands::launch_game`) performs the open + records the launch.

use crate::error::{CoreError, CoreResult};
use std::path::{Path, PathBuf};

/// Build the Steam run URL for an appid. The appid is validated to be
/// all-ASCII-digits before interpolation (TB2 boundary check) so nothing can be
/// smuggled into the URL — even though our scanner only ever yields numeric
/// appids (parsed from the manifest as `u32`).
pub fn steam_launch_url(appid: &str) -> CoreResult<String> {
    if appid.is_empty() || !appid.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CoreError::Parse(format!("invalid steam appid: {appid:?}")));
    }
    Ok(format!("steam://rungameid/{appid}"))
}

/// Build the Epic launch URL for an `AppName`. The name is validated to be
/// URL-safe (alphanumerics plus `_-.`) before interpolation (TB2) so a crafted
/// manifest can't inject extra query params or path segments.
pub fn epic_launch_url(app_name: &str) -> CoreResult<String> {
    let safe = !app_name.is_empty()
        && app_name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    if !safe {
        return Err(CoreError::Parse(format!(
            "invalid epic app name: {app_name:?}"
        )));
    }
    Ok(format!(
        "com.epicgames.launcher://apps/{app_name}?action=launch&silent=true"
    ))
}

/// Build the Steam uninstall deep-link for an appid (opens Steam's own
/// uninstall flow — the user confirms; we never delete files). Same digit-only
/// validation as [`steam_launch_url`] (TB2).
pub fn steam_uninstall_url(appid: &str) -> CoreResult<String> {
    if appid.is_empty() || !appid.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CoreError::Parse(format!("invalid steam appid: {appid:?}")));
    }
    Ok(format!("steam://uninstall/{appid}"))
}

/// Validate a local game's executable before spawning it (threat-model TB3).
/// Canonicalizes `exe` and `install_root`, then requires the exe to be an
/// existing `.exe` file located **under** the install root — defeating `..`
/// traversal and a manifest pointing somewhere unexpected. Returns the canonical
/// exe path (caller spawns it via argv with cwd = its parent dir).
pub fn validate_local_exe(exe: &Path, install_root: &Path) -> CoreResult<PathBuf> {
    // canonicalize also proves existence (errors if the path doesn't resolve).
    let exe_canon = std::fs::canonicalize(exe)
        .map_err(|e| CoreError::Io(std::io::Error::other(format!("exe {exe:?}: {e}"))))?;
    if !exe_canon.is_file() {
        return Err(CoreError::Unsupported(format!("not a file: {exe_canon:?}")));
    }
    let is_exe = exe_canon
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
    if !is_exe {
        return Err(CoreError::Unsupported(format!(
            "not an .exe: {exe_canon:?}"
        )));
    }
    let root_canon = std::fs::canonicalize(install_root)
        .map_err(|e| CoreError::Io(std::io::Error::other(format!("root {install_root:?}: {e}"))))?;
    if !exe_canon.starts_with(&root_canon) {
        return Err(CoreError::Unsupported(format!(
            "exe {exe_canon:?} escapes install root {root_canon:?}"
        )));
    }
    Ok(exe_canon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_run_url_for_numeric_appid() {
        assert_eq!(steam_launch_url("440").unwrap(), "steam://rungameid/440");
    }

    #[test]
    fn rejects_empty_appid() {
        assert!(matches!(
            steam_launch_url("").unwrap_err(),
            CoreError::Parse(_)
        ));
    }

    #[test]
    fn rejects_non_numeric_appid() {
        assert!(steam_launch_url("abc").is_err());
    }

    #[test]
    fn rejects_injection_attempt_in_appid() {
        // A crafted external_id must never reach the URL.
        assert!(steam_launch_url("440 && calc").is_err());
        assert!(steam_launch_url("440/../../evil").is_err());
    }

    #[test]
    fn builds_epic_launch_url() {
        assert_eq!(
            epic_launch_url("CrabEmoji").unwrap(),
            "com.epicgames.launcher://apps/CrabEmoji?action=launch&silent=true"
        );
        assert!(epic_launch_url("App.Name_1-2").is_ok());
    }

    #[test]
    fn rejects_unsafe_epic_app_name() {
        assert!(epic_launch_url("").is_err());
        assert!(epic_launch_url("App?action=evil").is_err());
        assert!(epic_launch_url("App Name").is_err()); // space
        assert!(epic_launch_url("App/../x").is_err());
    }

    #[test]
    fn builds_and_validates_steam_uninstall_url() {
        assert_eq!(steam_uninstall_url("440").unwrap(), "steam://uninstall/440");
        assert!(steam_uninstall_url("4 4 0").is_err());
    }

    #[test]
    fn validate_local_exe_accepts_exe_under_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let exe = root.join("game.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        let validated = validate_local_exe(&exe, root).unwrap();
        assert!(validated.ends_with("game.exe"));
    }

    #[test]
    fn validate_local_exe_rejects_exe_outside_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let exe = outside.join("evil.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        assert!(validate_local_exe(&exe, &root).is_err());
    }

    #[test]
    fn validate_local_exe_rejects_non_exe_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let txt = root.join("notes.txt");
        std::fs::write(&txt, b"x").unwrap();
        assert!(validate_local_exe(&txt, root).is_err());
        assert!(validate_local_exe(&root.join("ghost.exe"), root).is_err());
    }
}
