//! Core domain models (see CONTEXT.md for canonical terms; api-contract.md for
//! the IPC shapes). Serialized to the frontend with camelCase fields.

use crate::error::{CoreError, CoreResult};
use serde::{Deserialize, Serialize};

/// Where a game came from. Exactly one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Steam,
    Epic,
    Local,
    Gog,
}

impl Source {
    /// Stable string used in the DB `source` column and IPC.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Steam => "steam",
            Source::Epic => "epic",
            Source::Local => "local",
            Source::Gog => "gog",
        }
    }

    /// Parse the DB/IPC string form.
    pub fn parse(s: &str) -> CoreResult<Source> {
        match s {
            "steam" => Ok(Source::Steam),
            "epic" => Ok(Source::Epic),
            "local" => Ok(Source::Local),
            "gog" => Ok(Source::Gog),
            other => Err(CoreError::Parse(format!("unknown source: {other}"))),
        }
    }
}

/// An installed game in the library. `id` is 0 before persistence (the DB
/// assigns it). `name_norm`/`install_path_norm` dedup keys are derived in `db`,
/// not carried here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: i64,
    pub source: Source,
    pub external_id: String,
    pub name: String,
    pub install_path: String,
    pub exe_path: Option<String>,
    pub size_bytes: Option<i64>,
    pub drive: Option<String>,
    pub last_played: Option<i64>,
    pub launch_count: i64,
    pub cover_path: Option<String>,
    pub favorite: bool,
    /// User tags. Populated by `db` on read; scanners leave it empty.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A storage volume, queried live (never persisted — system-design §3). Sizes in
/// bytes as `i64` for lossless JS interop (consistent with `Game.size_bytes`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    /// Mount point, e.g. `"C:\\"`.
    pub mount: String,
    /// Drive letter for grouping with `Game.drive`, e.g. `"C:"`. `None` for UNC.
    pub letter: Option<String>,
    /// Volume label (often empty).
    pub label: String,
    pub total_bytes: i64,
    pub free_bytes: i64,
}

/// A video adapter for the GPU panel, queried live (never persisted). Model +
/// driver from WMI; `vram_bytes` (dedicated) comes from DXGI (Task 18), `None`
/// until then — WMI's `AdapterRAM` is 32-bit and caps at ~4GB, so it's not used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gpu {
    pub name: String,
    pub driver_version: String,
    /// Driver date as `YYYY-MM-DD`, if WMI reported a parseable one.
    pub driver_date: Option<String>,
    pub vram_bytes: Option<i64>,
}

/// A single system-monitor reading (live-only, never persisted — system-design
/// §3). Cumulative counters (net/disk) are turned into per-second rates by the
/// sampler before this is emitted. GPU fields are `None` when counters are
/// unavailable or admin-gated.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub cpu_percent: f32,
    pub mem_used_bytes: i64,
    pub mem_total_bytes: i64,
    pub net_rx_bps: i64,
    pub net_tx_bps: i64,
    pub disk_read_bps: i64,
    pub disk_write_bps: i64,
    pub gpu_percent: Option<f32>,
    pub vram_used_bytes: Option<i64>,
    pub vram_total_bytes: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_roundtrips_through_str() {
        for s in [Source::Steam, Source::Epic, Source::Local, Source::Gog] {
            assert_eq!(Source::parse(s.as_str()).unwrap(), s);
        }
    }

    #[test]
    fn source_parse_rejects_unknown() {
        assert!(Source::parse("origin").is_err());
    }
}
