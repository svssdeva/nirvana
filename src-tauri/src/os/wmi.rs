//! WMI seam (ADR-0005) for GPU model/driver info.

use super::WmiGpu;
use crate::error::CoreResult;

/// Read-only WMI access (the subset Nirvana needs).
pub trait Wmi {
    /// Adapters from `Win32_VideoController`.
    fn video_controllers(&self) -> CoreResult<Vec<WmiGpu>>;
}

#[cfg(windows)]
pub use windows_impl::WindowsWmi;

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use crate::error::CoreError;
    use serde::Deserialize;
    use wmi::{COMLibrary, WMIConnection};

    /// `Win32_VideoController` rows we read. Optional everywhere — a flaky adapter
    /// shouldn't fail the query (TB1). `DriverDate` stays a raw CIM_DATETIME
    /// string; `gpu::format_driver_date` parses it.
    #[derive(Deserialize)]
    #[serde(rename = "Win32_VideoController")]
    #[serde(rename_all = "PascalCase")]
    struct VideoController {
        name: Option<String>,
        driver_version: Option<String>,
        driver_date: Option<String>,
        adapter_ram: Option<u32>,
    }

    /// Real WMI access via the safe `wmi` crate. Initializes COM per call (the
    /// crate makes this idempotent on an already-initialized thread). Thin
    /// adapter — not unit-tested; exercised manually on Windows.
    pub struct WindowsWmi;

    impl Wmi for WindowsWmi {
        fn video_controllers(&self) -> CoreResult<Vec<WmiGpu>> {
            // Run on a freshly-spawned thread so COM initializes cleanly as MTA.
            // The Tauri command may run on the WebView's STA thread, where
            // `COMLibrary::new()` (MTA) fails with RPC_E_CHANGED_MODE — which would
            // make the GPU panel show nothing. A new thread has no prior apartment.
            std::thread::scope(|s| {
                s.spawn(query_controllers).join().unwrap_or_else(|_| {
                    Err(CoreError::GpuUnavailable("wmi thread panicked".into()))
                })
            })
        }
    }

    fn query_controllers() -> CoreResult<Vec<WmiGpu>> {
        let com = COMLibrary::new().map_err(wmi_err)?;
        let conn = WMIConnection::new(com).map_err(wmi_err)?;
        let rows: Vec<VideoController> = conn.query().map_err(wmi_err)?;
        Ok(rows
            .into_iter()
            .map(|r| WmiGpu {
                name: r.name.unwrap_or_default(),
                driver_version: r.driver_version.unwrap_or_default(),
                driver_date: r.driver_date,
                adapter_ram: r.adapter_ram,
            })
            .collect())
    }

    fn wmi_err(e: wmi::WMIError) -> CoreError {
        CoreError::GpuUnavailable(format!("wmi: {e}"))
    }
}

#[cfg(test)]
pub use fake::FakeWmi;

#[cfg(test)]
mod fake {
    use super::*;

    /// In-memory WMI for tests.
    #[derive(Default)]
    pub struct FakeWmi {
        gpus: Vec<WmiGpu>,
    }

    impl FakeWmi {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn with_gpu(mut self, gpu: WmiGpu) -> Self {
            self.gpus.push(gpu);
            self
        }
    }

    impl Wmi for FakeWmi {
        fn video_controllers(&self) -> CoreResult<Vec<WmiGpu>> {
            Ok(self.gpus.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_controllers_returns_seeded_gpus() {
        let wmi = FakeWmi::new().with_gpu(WmiGpu {
            name: "NVIDIA GeForce RTX 4070".into(),
            driver_version: "32.0.15.6094".into(),
            driver_date: Some("2025-01-15".into()),
            adapter_ram: Some(u32::MAX),
        });
        let gpus = wmi.video_controllers().unwrap();
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].name, "NVIDIA GeForce RTX 4070");
    }

    #[test]
    fn video_controllers_empty_by_default() {
        assert!(FakeWmi::new().video_controllers().unwrap().is_empty());
    }
}
