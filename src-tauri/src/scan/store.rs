//! Store registry (spec §3): one [`Descriptor`] per [`Source`] — the single
//! place a store's metadata lives. Keeps `Source` a closed enum while removing
//! the per-store scatter across scan orchestration, dedup ranking, launch
//! dispatch, and frontend theming. Adding a store becomes: enum variant + one
//! descriptor row + one scanner + register here.

use crate::error::CoreResult;
use crate::models::{Game, Source};
use crate::os::{Appx, FileSystem, Registry};
use std::path::PathBuf;

/// How a store's games are launched (dispatched in `commands::launch_game`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchStrategy {
    /// Official URL protocol (steam/epic).
    Protocol,
    /// Validated argv spawn of the exe (local).
    Exe,
    /// Protocol if the store client is installed, else exe (gog).
    Hybrid,
    /// `shell:AppsFolder\<AUMID>` for packaged Store/Xbox apps (xbox).
    Shell,
}

/// Shared, thread-safe scan dependencies, assembled once per scan. The `+ Sync`
/// bound lets a single `&ScanCtx` be shared across the `thread::scope` spawns.
pub struct ScanCtx<'a> {
    pub registry: &'a (dyn Registry + Sync),
    pub fs: &'a (dyn FileSystem + Sync),
    pub appx: &'a (dyn Appx + Sync),
    pub watch_folders: &'a [PathBuf],
    /// Resolved well-known directories (kept here so scanners stay path-pure).
    pub epic_dir: PathBuf,
    pub program_data: PathBuf,
}

/// One store. `scan` is a plain `fn` pointer (the adapters below capture
/// nothing), so the existing per-store scanners — and their unit tests — are
/// untouched; the descriptor just adapts their current constructors.
pub struct Descriptor {
    pub source: Source,
    pub display: &'static str,
    pub color: &'static str,
    /// Cross-source dedup priority (lower wins).
    pub rank: u8,
    pub scan: fn(&ScanCtx) -> CoreResult<Vec<Game>>,
    pub launch: LaunchStrategy,
}

/// The store registry. Order is irrelevant; dedup uses `rank`. GOG joins this in
/// a later task once its scanner exists.
pub static STORES: &[Descriptor] = &[
    Descriptor {
        source: Source::Steam,
        display: "Steam",
        color: "#1b2838",
        rank: 0,
        scan: |c| crate::scan::steam::SteamScanner::new(c.registry, c.fs).scan(),
        launch: LaunchStrategy::Protocol,
    },
    Descriptor {
        source: Source::Epic,
        display: "Epic",
        color: "#2a2a2a",
        rank: 1,
        scan: |c| crate::scan::epic::EpicScanner::new(c.fs).scan(&c.epic_dir),
        launch: LaunchStrategy::Protocol,
    },
    Descriptor {
        source: Source::Gog,
        display: "GOG",
        color: "#a23fff",
        rank: 2,
        scan: crate::scan::gog::scan,
        launch: LaunchStrategy::Hybrid,
    },
    Descriptor {
        source: Source::Xbox,
        display: "Xbox",
        color: "#107c10",
        rank: 3,
        scan: crate::scan::xbox::scan,
        launch: LaunchStrategy::Shell,
    },
    Descriptor {
        source: Source::Ubisoft,
        display: "Ubisoft",
        color: "#0070ff",
        rank: 4,
        scan: crate::scan::ubisoft::scan,
        launch: LaunchStrategy::Protocol,
    },
    Descriptor {
        source: Source::Ea,
        display: "EA",
        color: "#ff4747",
        rank: 5,
        scan: crate::scan::ea::scan,
        launch: LaunchStrategy::Protocol,
    },
    Descriptor {
        source: Source::Local,
        display: "Local",
        color: "#0070d1",
        // Last: the catch-all fallback, outranked by every real store.
        rank: 6,
        scan: |c| crate::scan::local::LocalScanner::new(c.fs).scan(c.watch_folders),
        launch: LaunchStrategy::Exe,
    },
];

/// The descriptor for a source. Panics only if a `Source` has no row — which the
/// `every_registered_source_is_unique` test guards against for registered stores.
pub fn descriptor_for(source: Source) -> &'static Descriptor {
    STORES
        .iter()
        .find(|d| d.source == source)
        .expect("every scanned Source must have a Descriptor in STORES")
}

/// Cross-source dedup priority (lower wins), read from the registry.
pub fn source_rank(source: Source) -> u8 {
    descriptor_for(source).rank
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_source_has_exactly_one_descriptor() {
        // Guards against adding a Source variant without registering its store.
        for s in [
            Source::Steam,
            Source::Epic,
            Source::Local,
            Source::Gog,
            Source::Xbox,
            Source::Ubisoft,
            Source::Ea,
        ] {
            let n = STORES.iter().filter(|d| d.source == s).count();
            assert_eq!(n, 1, "source {s:?} needs exactly one descriptor");
        }
    }

    #[test]
    fn ranks_order_stores_above_local() {
        assert!(source_rank(Source::Steam) < source_rank(Source::Local));
        assert!(source_rank(Source::Epic) < source_rank(Source::Local));
    }

    #[test]
    fn colors_are_hex() {
        assert!(STORES.iter().all(|d| d.color.starts_with('#')));
    }
}
