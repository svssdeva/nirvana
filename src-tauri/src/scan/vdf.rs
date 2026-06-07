//! VDF (Valve KeyValues) adapter — outcome of the ADR-0003 spike (Task 6).
//!
//! Thin wrapper around `keyvalues-serde` (v0.2): the Steam scanner depends on
//! THIS module, not the crate directly, so the implementation stays swappable
//! (ADR-0003). `keyvalues_serde::from_str` ignores the top-level KeyValues key
//! ("libraryfolders" / "AppState") and deserializes its value into our structs;
//! unknown fields — including nested objects like `InstalledDepots` — are
//! skipped by serde. All parse failures map to `CoreError::Parse` (TB1: never
//! panic on untrusted on-disk data).

use crate::error::{CoreError, CoreResult};
use serde::Deserialize;
use std::collections::BTreeMap;

/// One Steam library root, parsed from `libraryfolders.vdf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryFolder {
    /// Absolute path to the library root (unescaped: `\\` → `\`).
    pub path: String,
    /// User label (often empty).
    pub label: String,
    /// Installed apps in this library: appid → bytes-on-disk (Steam's tally).
    pub apps: BTreeMap<String, String>,
}

/// Wire shape of a library folder entry; trims to the fields we keep.
#[derive(Debug, Deserialize)]
struct RawLibraryFolder {
    path: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    apps: BTreeMap<String, String>,
}

/// The fields of `appmanifest_<appid>.acf` the launcher needs. Other keys
/// (Universe, buildid, InstalledDepots, UserConfig, …) are intentionally
/// ignored. Numeric VDF values are quoted strings; serde parses them into
/// these integer types.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AppManifest {
    pub appid: u32,
    pub name: String,
    pub installdir: String,
    #[serde(rename = "StateFlags")]
    pub state_flags: u32,
    #[serde(rename = "SizeOnDisk")]
    pub size_on_disk: u64,
    #[serde(rename = "LastUpdated", default)]
    pub last_updated: u64,
}

/// Parse `libraryfolders.vdf` into the list of library roots. Order follows the
/// numeric index keys ("0", "1", …) as sorted by the map.
pub fn parse_library_folders(vdf: &str) -> CoreResult<Vec<LibraryFolder>> {
    let raw: BTreeMap<String, RawLibraryFolder> =
        keyvalues_serde::from_str(vdf).map_err(|e| CoreError::Parse(e.to_string()))?;
    Ok(raw
        .into_values()
        .map(|f| LibraryFolder {
            path: f.path,
            label: f.label,
            apps: f.apps,
        })
        .collect())
}

/// Parse a single `appmanifest_<appid>.acf` into the typed [`AppManifest`].
pub fn parse_app_manifest(acf: &str) -> CoreResult<AppManifest> {
    keyvalues_serde::from_str(acf).map_err(|e| CoreError::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARYFOLDERS: &str = include_str!("../../tests/fixtures/steam/libraryfolders.vdf");
    const TF2: &str = include_str!("../../tests/fixtures/steam/appmanifest_440.acf");
    const DOTA: &str = include_str!("../../tests/fixtures/steam/appmanifest_570.acf");

    #[test]
    fn parses_library_folders_with_unescaped_paths_and_apps() {
        let folders = parse_library_folders(LIBRARYFOLDERS).unwrap();
        assert_eq!(folders.len(), 2);
        // `\\` in the VDF is unescaped to a single backslash.
        assert_eq!(folders[0].path, r"C:\Program Files (x86)\Steam");
        assert_eq!(folders[1].path, r"D:\SteamLibrary");
        assert_eq!(folders[1].label, "Games");
        // apps: appid -> size string.
        assert_eq!(
            folders[0].apps.get("440").map(String::as_str),
            Some("24305820742")
        );
        assert!(folders[0].apps.contains_key("570"));
        assert_eq!(
            folders[1].apps.get("230410").map(String::as_str),
            Some("45088972841")
        );
    }

    #[test]
    fn parses_app_manifest_numeric_fields() {
        let m = parse_app_manifest(TF2).unwrap();
        assert_eq!(m.appid, 440);
        assert_eq!(m.name, "Team Fortress 2");
        assert_eq!(m.installdir, "Team Fortress 2");
        assert_eq!(m.state_flags, 4);
        assert_eq!(m.size_on_disk, 24_305_820_742); // > u32::MAX — needs u64.
        assert_eq!(m.last_updated, 1_716_950_400);
    }

    #[test]
    fn skips_unknown_nested_objects() {
        // The Dota manifest carries InstalledDepots (a map of maps) we don't
        // model; deserialization must skip it, not fail.
        let m = parse_app_manifest(DOTA).unwrap();
        assert_eq!(m.appid, 570);
        assert_eq!(m.installdir, "dota 2 beta");
        assert_eq!(m.size_on_disk, 17_773_670_227);
        assert_eq!(m.last_updated, 1_716_000_000);
    }

    #[test]
    fn malformed_vdf_returns_parse_error_not_panic() {
        let err = parse_app_manifest("\"AppState\" { \"appid\" ").unwrap_err();
        assert!(matches!(err, CoreError::Parse(_)));
    }
}
