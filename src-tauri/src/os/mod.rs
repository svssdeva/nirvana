//! OS access behind trait seams (ADR-0005).
//!
//! Each concern (registry, filesystem, WMI, PDH) is a trait with a real
//! Windows implementation (`#[cfg(windows)]`) and an in-memory fake (`#[cfg(test)]`).
//! Scanners/sizers/art take these traits by injection, so their parse/dedup
//! logic is unit-tested against fakes on any OS, and the real impls are the
//! Windows portability boundary. See `docs/api-contract.md`.

use std::path::PathBuf;

pub mod fs;
pub mod icon;
pub mod pdh;
pub mod registry;
pub mod wmi;

pub use fs::FileSystem;
pub use icon::{IconExtractor, IconRgba};
pub use pdh::Pdh;
pub use registry::Registry;
pub use wmi::Wmi;

/// Registry hive selector (the two we read).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

/// A directory entry surfaced by [`FileSystem::read_dir`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntryInfo {
    pub path: PathBuf,
    pub is_dir: bool,
    /// Windows junction/symlink/reparse point. The disk walker must NOT recurse
    /// into these (avoids double-count / cycles — Steam library moves use them).
    pub is_reparse_point: bool,
}

/// Minimal file metadata from [`FileSystem::metadata`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileMeta {
    pub len: u64,
    pub is_dir: bool,
    pub is_reparse_point: bool,
}

/// A video adapter as reported by WMI `Win32_VideoController`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WmiGpu {
    pub name: String,
    pub driver_version: String,
    pub driver_date: Option<String>,
    /// 32-bit WMI field — caps at ~4GB, so it is NOT a reliable VRAM size.
    /// VRAM comes from DXGI `QueryVideoMemoryInfo` / NVML (see api-contract.md).
    pub adapter_ram: Option<u32>,
}
