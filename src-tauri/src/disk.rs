//! Disk insight (plan Task 14, M2, FR-DISK): per-drive capacity (`sysinfo`) and
//! per-game on-disk size (recursive walk via the [`FileSystem`] seam).
//!
//! Sizing **skips reparse points** (junctions/symlinks) and **tracks visited
//! dirs**, so a Steam library move — which uses junctions — is never
//! double-counted or looped (the os/ seam surfaces `is_reparse_point`). The walk
//! is single-threaded over the seam: correct and unit-testable on any OS.
//! Parallelizing it (jwalk/rayon) is a deferred perf optimization — not added
//! until sizing is shown to be a bottleneck (no premature dependency).

use crate::models::Drive;
use crate::os::FileSystem;
use crate::scan::{drive_of, is_not_found};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Recursive on-disk size (bytes) of everything under `root`: skip reparse
/// points, count each real file once. Best-effort — an unreadable subdirectory
/// is skipped (logged), never fatal, so a locked folder can't fail the whole
/// measurement. A missing `root` is `0`.
pub fn dir_size(fs: &dyn FileSystem, root: &Path) -> u64 {
    let mut total: u64 = 0;
    let mut visited: HashSet<String> = HashSet::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        // Visited guard (Windows paths are case-insensitive) — belt-and-suspenders
        // against any cycle the reparse-point check might miss.
        if !visited.insert(dir.to_string_lossy().to_lowercase()) {
            continue;
        }
        let entries = match fs.read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                if !is_not_found(&e) {
                    tracing::debug!(dir = %dir.display(), error = %e, "size: skipping unreadable dir");
                }
                continue;
            }
        };
        for entry in entries {
            if entry.is_reparse_point {
                continue; // junction/symlink — never follow (FR-DISK footgun)
            }
            if entry.is_dir {
                stack.push(entry.path);
            } else if let Ok(meta) = fs.metadata(&entry.path) {
                total = total.saturating_add(meta.len);
            }
        }
    }
    total
}

/// Enumerate storage volumes with capacity via `sysinfo`. Thin OS adapter — the
/// `Drive` mapping is tested via [`drive_from_disk`]; live values are verified
/// manually (like the other real os/ impls).
pub fn list_drives() -> Vec<Drive> {
    sysinfo::Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(drive_from_disk)
        .collect()
}

fn drive_from_disk(disk: &sysinfo::Disk) -> Drive {
    let mount = disk.mount_point();
    Drive {
        mount: mount.to_string_lossy().into_owned(),
        letter: drive_of(mount),
        label: disk.name().to_string_lossy().into_owned(),
        total_bytes: i64::try_from(disk.total_space()).unwrap_or(i64::MAX),
        free_bytes: i64::try_from(disk.available_space()).unwrap_or(i64::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::fs::FakeFs;
    use crate::os::DirEntryInfo;

    fn dir(path: &str, reparse: bool) -> DirEntryInfo {
        DirEntryInfo {
            path: PathBuf::from(path),
            is_dir: true,
            is_reparse_point: reparse,
        }
    }
    fn file(path: &str) -> DirEntryInfo {
        DirEntryInfo {
            path: PathBuf::from(path),
            is_dir: false,
            is_reparse_point: false,
        }
    }

    #[test]
    fn sums_files_recursively_skipping_junctions() {
        let fs = FakeFs::new()
            .with_dir(
                r"C:\Game",
                vec![
                    file(r"C:\Game\game.exe"),
                    dir(r"C:\Game\Data", false),
                    dir(r"C:\Game\LinkedLib", true), // junction → must be skipped
                ],
            )
            .with_file(r"C:\Game\game.exe", &"x".repeat(50))
            .with_dir(
                r"C:\Game\Data",
                vec![file(r"C:\Game\Data\a.bin"), file(r"C:\Game\Data\b.bin")],
            )
            .with_file(r"C:\Game\Data\a.bin", &"x".repeat(100))
            .with_file(r"C:\Game\Data\b.bin", &"x".repeat(200))
            // Behind the junction — would add 999_999 if (wrongly) followed.
            .with_dir(
                r"C:\Game\LinkedLib",
                vec![file(r"C:\Game\LinkedLib\huge.bin")],
            )
            .with_file(r"C:\Game\LinkedLib\huge.bin", &"x".repeat(999_999));

        assert_eq!(dir_size(&fs, Path::new(r"C:\Game")), 350);
    }

    #[test]
    fn missing_root_is_zero() {
        assert_eq!(dir_size(&FakeFs::new(), Path::new(r"C:\Nope")), 0);
    }

    #[test]
    fn visited_guard_counts_a_shared_dir_once() {
        // Both A and B reference the same C:\Shared; it must be counted once.
        let fs = FakeFs::new()
            .with_dir(
                r"C:\Root",
                vec![dir(r"C:\Root\A", false), dir(r"C:\Root\B", false)],
            )
            .with_dir(r"C:\Root\A", vec![dir(r"C:\Shared", false)])
            .with_dir(r"C:\Root\B", vec![dir(r"C:\Shared", false)])
            .with_dir(r"C:\Shared", vec![file(r"C:\Shared\x.bin")])
            .with_file(r"C:\Shared\x.bin", &"x".repeat(123));

        assert_eq!(dir_size(&fs, Path::new(r"C:\Root")), 123);
    }
}
