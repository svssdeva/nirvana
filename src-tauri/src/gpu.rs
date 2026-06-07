//! GPU model/driver info (plan Task 16, M3, FR-GPU).
//!
//! Reads adapters from WMI `Win32_VideoController` through the [`Wmi`] seam and
//! maps them to [`Gpu`] (model + driver + parsed driver date). VRAM is **not**
//! taken from WMI's `AdapterRAM` (32-bit, caps ~4GB) — DXGI fills `vram_bytes`
//! in Task 18. Mapping/date parsing are unit-tested against the fake; the real
//! WMI query (`os::wmi`, via the `wmi` crate) is the OS adapter.

use crate::error::CoreResult;
use crate::models::Gpu;
use crate::os::{Wmi, WmiGpu};

/// All video adapters with model + driver info. Empty if WMI reports none;
/// errors (`GpuUnavailable`) propagate from the seam.
pub fn get_gpus(wmi: &dyn Wmi) -> CoreResult<Vec<Gpu>> {
    Ok(wmi
        .video_controllers()?
        .into_iter()
        .map(gpu_from_wmi)
        .collect())
}

fn gpu_from_wmi(w: WmiGpu) -> Gpu {
    Gpu {
        driver_date: w.driver_date.as_deref().and_then(format_driver_date),
        name: w.name,
        driver_version: w.driver_version,
        vram_bytes: None, // DXGI dedicated VRAM is wired in Task 18.
    }
}

/// Parse a WMI/CIM_DATETIME (`"yyyymmddHHMMSS.ffffff±UUU"`) into `YYYY-MM-DD`.
/// `None` if the leading 8 chars aren't a date — WMI dates are well-formed, but
/// we never panic on adapter-reported data (TB1).
fn format_driver_date(cim: &str) -> Option<String> {
    let date = cim.get(0..8)?;
    if date.bytes().all(|b| b.is_ascii_digit()) {
        Some(format!("{}-{}-{}", &date[0..4], &date[4..6], &date[6..8]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::wmi::FakeWmi;

    fn wmi_gpu(name: &str, ver: &str, date: Option<&str>) -> WmiGpu {
        WmiGpu {
            name: name.into(),
            driver_version: ver.into(),
            driver_date: date.map(str::to_string),
            adapter_ram: Some(u32::MAX),
        }
    }

    #[test]
    fn maps_model_and_driver_and_parses_date() {
        let wmi = FakeWmi::new().with_gpu(wmi_gpu(
            "NVIDIA GeForce RTX 4070",
            "32.0.15.6094",
            Some("20250115000000.000000-000"),
        ));
        let gpus = get_gpus(&wmi).unwrap();
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].name, "NVIDIA GeForce RTX 4070");
        assert_eq!(gpus[0].driver_version, "32.0.15.6094");
        assert_eq!(gpus[0].driver_date.as_deref(), Some("2025-01-15"));
        assert_eq!(gpus[0].vram_bytes, None, "VRAM comes from DXGI, not WMI");
    }

    #[test]
    fn lists_multiple_adapters() {
        let wmi = FakeWmi::new()
            .with_gpu(wmi_gpu("Intel UHD Graphics", "31.0.101.2111", None))
            .with_gpu(wmi_gpu("NVIDIA GeForce RTX 4070", "32.0.15.6094", None));
        let gpus = get_gpus(&wmi).unwrap();
        assert_eq!(gpus.len(), 2);
    }

    #[test]
    fn empty_when_no_adapters() {
        assert!(get_gpus(&FakeWmi::new()).unwrap().is_empty());
    }

    #[test]
    fn malformed_date_becomes_none() {
        assert_eq!(format_driver_date("notadate"), None);
        assert_eq!(format_driver_date(""), None);
        assert_eq!(format_driver_date("20240320"), Some("2024-03-20".into()));
    }
}
