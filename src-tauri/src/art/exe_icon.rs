//! Exe-icon cover orchestration (plan Task 10, FR-ART, offline).
//!
//! Wraps the [`IconExtractor`] os seam with on-disk PNG caching: an exe's icon is
//! extracted once and encoded to `<cache_dir>\icon-<hash>.png`, reused on later
//! calls. The actual Windows extraction is deferred to Task 13 (see
//! `os::icon`); this layer — cache-key derivation, dimension capping (TB1), PNG
//! encoding, and reuse — is implemented and tested now against a fake extractor.

use crate::error::{CoreError, CoreResult};
use crate::os::IconExtractor;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Largest icon we'll encode — guards against an extractor returning an absurd
/// buffer (TB1: cap dimensions on attacker-influenced inputs).
const MAX_DIM: u32 = 1024;

/// Deterministic cache filename for an exe's icon. Keyed on the lowercased path
/// (Windows paths are case-insensitive) so repeat calls hit the same file.
pub fn cache_file_name(exe: &Path) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    exe.to_string_lossy().to_lowercase().hash(&mut hasher);
    format!("icon-{:016x}.png", hasher.finish())
}

/// Resolve a cached PNG icon for `exe`, extracting + encoding it on first use.
/// `Ok(None)` when the exe has no icon (or its size is implausible). Errors only
/// on encode/IO failure.
pub fn resolve(
    icons: &dyn IconExtractor,
    exe: &Path,
    cache_dir: &Path,
) -> CoreResult<Option<PathBuf>> {
    let dest = cache_dir.join(cache_file_name(exe));
    if dest.is_file() {
        return Ok(Some(dest));
    }
    let Some(icon) = icons.extract(exe)? else {
        return Ok(None);
    };
    if icon.width == 0 || icon.height == 0 || icon.width > MAX_DIM || icon.height > MAX_DIM {
        tracing::warn!(exe = %exe.display(), w = icon.width, h = icon.height, "skipping implausible icon size");
        return Ok(None);
    }
    let buffer = image::RgbaImage::from_raw(icon.width, icon.height, icon.rgba)
        .ok_or_else(|| CoreError::Parse("icon pixel buffer size mismatch".into()))?;
    std::fs::create_dir_all(cache_dir)?;
    buffer
        .save(&dest)
        .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))?;
    Ok(Some(dest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::icon::FakeIcons;
    use crate::os::IconRgba;

    fn solid_icon(w: u32, h: u32) -> IconRgba {
        IconRgba {
            width: w,
            height: h,
            rgba: vec![0x33; (w * h * 4) as usize],
        }
    }

    #[test]
    fn cache_file_name_is_stable_and_case_insensitive() {
        let a = cache_file_name(Path::new(r"C:\Games\Foo\foo.exe"));
        let b = cache_file_name(Path::new(r"c:\games\foo\FOO.exe"));
        assert_eq!(a, b);
        assert!(a.starts_with("icon-") && a.ends_with(".png"));
    }

    #[test]
    fn resolve_extracts_encodes_and_caches_png() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new(r"C:\Games\Foo\foo.exe");
        let icons = FakeIcons::new().with_icon(exe, solid_icon(16, 16));

        let path = resolve(&icons, exe, dir.path()).unwrap().unwrap();
        assert!(path.is_file());
        // Round-trips as a real PNG of the right dimensions.
        let decoded = image::open(&path).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (16, 16));
    }

    #[test]
    fn resolve_returns_none_when_no_icon() {
        let dir = tempfile::tempdir().unwrap();
        let icons = FakeIcons::new();
        let got = resolve(&icons, Path::new(r"C:\Games\Bar\bar.exe"), dir.path()).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn resolve_rejects_oversized_icon() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new(r"C:\Games\Big\big.exe");
        let icons = FakeIcons::new().with_icon(exe, solid_icon(2048, 2048));
        let got = resolve(&icons, exe, dir.path()).unwrap();
        assert!(got.is_none());
    }
}
