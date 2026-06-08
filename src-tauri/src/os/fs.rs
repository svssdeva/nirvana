//! Filesystem seam (ADR-0005). Surfaces reparse-point info so the disk walker
//! can skip junctions (FR-DISK footgun).

use super::{DirEntryInfo, FileMeta};
use crate::error::CoreResult;
use std::path::Path;

/// Read-only filesystem access.
pub trait FileSystem {
    fn read_to_string(&self, path: &Path) -> CoreResult<String>;
    /// Immediate entries of a directory. Does NOT recurse, does NOT follow links.
    fn read_dir(&self, path: &Path) -> CoreResult<Vec<DirEntryInfo>>;
    /// Metadata WITHOUT following symlinks/reparse points (so `is_reparse_point` is meaningful).
    fn metadata(&self, path: &Path) -> CoreResult<FileMeta>;
}

#[cfg(windows)]
pub use windows_impl::WindowsFs;

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    fn is_reparse(attrs: u32) -> bool {
        attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    /// Real filesystem via `std::fs` with Windows reparse-point detection.
    /// Thin adapter — not unit-tested (covered by scanner/disk integration tests).
    pub struct WindowsFs;

    impl FileSystem for WindowsFs {
        fn read_to_string(&self, path: &Path) -> CoreResult<String> {
            Ok(std::fs::read_to_string(path)?)
        }

        fn read_dir(&self, path: &Path) -> CoreResult<Vec<DirEntryInfo>> {
            let mut out = Vec::new();
            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                // DirEntry::metadata does not traverse symlinks → reparse attr is visible.
                let md = entry.metadata()?;
                out.push(DirEntryInfo {
                    path: entry.path(),
                    is_dir: md.is_dir(),
                    is_reparse_point: is_reparse(md.file_attributes()),
                });
            }
            Ok(out)
        }

        fn metadata(&self, path: &Path) -> CoreResult<FileMeta> {
            // symlink_metadata: do not traverse, so we can detect a reparse point.
            let md = std::fs::symlink_metadata(path)?;
            Ok(FileMeta {
                len: md.len(),
                is_dir: md.is_dir(),
                is_reparse_point: is_reparse(md.file_attributes()),
            })
        }
    }
}

#[cfg(test)]
pub use fake::FakeFs;

#[cfg(test)]
mod fake {
    use super::*;
    use crate::error::CoreError;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// In-memory filesystem for tests. Files map path → contents; dirs map path →
    /// child entries.
    #[derive(Default)]
    pub struct FakeFs {
        files: HashMap<PathBuf, String>,
        dirs: HashMap<PathBuf, Vec<DirEntryInfo>>,
    }

    impl FakeFs {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn with_file(mut self, path: impl Into<PathBuf>, contents: &str) -> Self {
            self.files.insert(path.into(), contents.to_string());
            self
        }
        pub fn with_dir(mut self, path: impl Into<PathBuf>, entries: Vec<DirEntryInfo>) -> Self {
            self.dirs.insert(path.into(), entries);
            self
        }
    }

    impl FileSystem for FakeFs {
        fn read_to_string(&self, path: &Path) -> CoreResult<String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| CoreError::NotFound(path.display().to_string()))
        }

        fn read_dir(&self, path: &Path) -> CoreResult<Vec<DirEntryInfo>> {
            self.dirs
                .get(path)
                .cloned()
                .ok_or_else(|| CoreError::NotFound(path.display().to_string()))
        }

        fn metadata(&self, path: &Path) -> CoreResult<FileMeta> {
            if let Some(contents) = self.files.get(path) {
                return Ok(FileMeta {
                    len: contents.len() as u64,
                    is_dir: false,
                    is_reparse_point: false,
                });
            }
            // Prefer a child entry — it carries the real is_dir / reparse flags
            // (so a junction listed as a child keeps its reparse bit).
            if let Some(meta) = self
                .dirs
                .values()
                .flatten()
                .find(|e| e.path == path)
                .map(|e| FileMeta {
                    len: 0,
                    is_dir: e.is_dir,
                    is_reparse_point: e.is_reparse_point,
                })
            {
                return Ok(meta);
            }
            // Fall back to a directory registered via `with_dir` that isn't also
            // listed as someone's child (e.g. a top-level install root) — it
            // still exists.
            if self.dirs.contains_key(path) {
                return Ok(FileMeta {
                    len: 0,
                    is_dir: true,
                    is_reparse_point: false,
                });
            }
            Err(CoreError::NotFound(path.display().to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn read_to_string_returns_seeded_contents() {
        let fs = FakeFs::new().with_file(r"C:\Steam\libraryfolders.vdf", "\"libraryfolders\"{}");
        assert_eq!(
            fs.read_to_string(Path::new(r"C:\Steam\libraryfolders.vdf"))
                .unwrap(),
            "\"libraryfolders\"{}"
        );
    }

    #[test]
    fn read_to_string_errors_when_missing() {
        let fs = FakeFs::new();
        assert!(fs.read_to_string(Path::new(r"C:\nope.txt")).is_err());
    }

    #[test]
    fn read_dir_returns_entries_with_reparse_flag() {
        let entries = vec![
            DirEntryInfo {
                path: PathBuf::from(r"C:\Games\Real"),
                is_dir: true,
                is_reparse_point: false,
            },
            DirEntryInfo {
                path: PathBuf::from(r"C:\Games\Junction"),
                is_dir: true,
                is_reparse_point: true,
            },
        ];
        let fs = FakeFs::new().with_dir(r"C:\Games", entries.clone());
        let got = fs.read_dir(Path::new(r"C:\Games")).unwrap();
        assert_eq!(got, entries);
        assert!(got[1].is_reparse_point);
    }

    #[test]
    fn metadata_reports_len_and_reparse() {
        let entries = vec![DirEntryInfo {
            path: PathBuf::from(r"C:\Games\Junction"),
            is_dir: true,
            is_reparse_point: true,
        }];
        let fs = FakeFs::new().with_dir(r"C:\Games", entries);
        let md = fs.metadata(Path::new(r"C:\Games\Junction")).unwrap();
        assert!(md.is_reparse_point);
        assert!(md.is_dir);
    }

    #[test]
    fn read_dir_errors_on_unknown_dir() {
        // Fidelity: real WindowsFs errors on a missing dir, so the fake must too
        // (returning empty would mask a scanner reading the wrong path).
        let fs = FakeFs::new();
        assert!(fs.read_dir(Path::new(r"C:\Unseeded")).is_err());
    }
}
