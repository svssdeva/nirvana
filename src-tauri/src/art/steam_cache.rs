//! Steam library-cache cover lookup (plan Task 10, FR-ART, offline).
//!
//! Steam stores capsule art under `<steam_root>\appcache\librarycache`. Two
//! client layouts exist (ADR-0003 spike notes) and we handle both:
//!   - per-appid (modern, ~2023+): `librarycache\<appid>\library_600x900.jpg`
//!   - flat (legacy):              `librarycache\<appid>_library_600x900.jpg`
//!
//! Portrait `library_600x900` is preferred (matches the design.md game-tile);
//! `header.jpg` (landscape) is the fallback. Lookup goes through the
//! [`FileSystem`] seam so it's unit-tested with the in-memory fake.

use crate::os::FileSystem;
use std::path::{Path, PathBuf};

/// Portrait library capsule — preferred (design.md game-tile is portrait-ish).
const PORTRAIT: &str = "library_600x900.jpg";
/// Landscape header — fallback when no portrait capsule is cached.
const HEADER: &str = "header.jpg";

/// Resolve an existing cover file for `appid`, or `None` if nothing is cached.
/// Candidates are probed in preference order; the first that exists as a file
/// wins.
pub fn resolve(fs: &dyn FileSystem, steam_root: &Path, appid: &str) -> Option<PathBuf> {
    let base = steam_root.join("appcache").join("librarycache");
    let per_appid = base.join(appid);
    let candidates = [
        per_appid.join(PORTRAIT),
        base.join(format!("{appid}_{PORTRAIT}")),
        per_appid.join(HEADER),
        base.join(format!("{appid}_{HEADER}")),
    ];
    candidates.into_iter().find(|p| is_file(fs, p))
}

fn is_file(fs: &dyn FileSystem, path: &Path) -> bool {
    matches!(fs.metadata(path), Ok(meta) if !meta.is_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::fs::FakeFs;

    const ROOT: &str = r"C:\Program Files (x86)\Steam";

    fn cache_path(rest: &str) -> String {
        format!(r"{ROOT}\appcache\librarycache\{rest}")
    }

    #[test]
    fn resolves_per_appid_portrait_capsule() {
        let fs = FakeFs::new().with_file(cache_path(r"440\library_600x900.jpg"), "jpegbytes");
        let got = resolve(&fs, Path::new(ROOT), "440").unwrap();
        assert_eq!(got, PathBuf::from(cache_path(r"440\library_600x900.jpg")));
    }

    #[test]
    fn resolves_flat_legacy_portrait_capsule() {
        let fs = FakeFs::new().with_file(cache_path("570_library_600x900.jpg"), "jpegbytes");
        let got = resolve(&fs, Path::new(ROOT), "570").unwrap();
        assert_eq!(got, PathBuf::from(cache_path("570_library_600x900.jpg")));
    }

    #[test]
    fn prefers_portrait_over_header() {
        let fs = FakeFs::new()
            .with_file(cache_path(r"440\header.jpg"), "h")
            .with_file(cache_path(r"440\library_600x900.jpg"), "p");
        let got = resolve(&fs, Path::new(ROOT), "440").unwrap();
        assert_eq!(got, PathBuf::from(cache_path(r"440\library_600x900.jpg")));
    }

    #[test]
    fn falls_back_to_header_when_no_portrait() {
        let fs = FakeFs::new().with_file(cache_path(r"440\header.jpg"), "h");
        let got = resolve(&fs, Path::new(ROOT), "440").unwrap();
        assert_eq!(got, PathBuf::from(cache_path(r"440\header.jpg")));
    }

    #[test]
    fn returns_none_when_nothing_cached() {
        let fs = FakeFs::new();
        assert!(resolve(&fs, Path::new(ROOT), "440").is_none());
    }
}
