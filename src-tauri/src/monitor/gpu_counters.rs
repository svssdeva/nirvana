//! GPU + disk-I/O counters (plan Task 18, M3). Windows-only.
//!
//! Enriches a [`Sample`] with GPU engine utilization and disk throughput (PDH)
//! and VRAM (DXGI). Everything is **best-effort**: PDH GPU counters can be empty
//! or admin-gated (NVIDIA `RmProfilingAdminOnly`) and DXGI can fail on old
//! drivers — any failure leaves the field `None`/`0` so the monitor degrades
//! ("GPU unavailable") instead of erroring. All `unsafe` FFI is confined here
//! (it's the os boundary for the monitor); handles are closed on drop. The pure
//! 3D-engine aggregation is unit-tested; live values are verified manually
//! against Task Manager.
//!
//! Thread-safety: the sampler future owns a `GpuCounters` across `.await`s, so
//! the multi-threaded tokio runtime may resume it on a different worker thread.
//! That's sound here — PDH query/counter handles are process-global (usable from
//! any thread) and DXGI's `QueryVideoMemoryInfo` is free-threaded; we never enter
//! an STA. No counter handle outlives its query (closed together on `Drop`).

use crate::models::Sample;

/// Sum the utilization of the 3D-engine instances (Task Manager's headline GPU
/// figure), clamped to 0–100. Instance names look like
/// `pid_1234_..._engtype_3D`; non-3D engines (Copy/Video/Compute) are ignored.
/// Pure + unit-tested.
fn aggregate_3d_util(instances: &[(String, f64)]) -> f32 {
    let sum: f64 = instances
        .iter()
        .filter(|(name, _)| name.contains("engtype_3D"))
        .map(|(_, value)| value)
        .sum();
    sum.clamp(0.0, 100.0) as f32
}

/// Holds the live PDH query + the GPU's total VRAM (from DXGI, read once).
/// `first` skips the GPU-util/disk read on the very first tick (rate/util
/// counters need two collections; the VRAM gauge is valid immediately).
pub struct GpuCounters {
    pdh: Option<pdh::PdhCounters>,
    vram_total: Option<i64>,
    first: bool,
}

impl Default for GpuCounters {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuCounters {
    pub fn new() -> Self {
        Self {
            pdh: pdh::PdhCounters::open(),
            vram_total: dxgi::total_vram(),
            first: true,
        }
    }

    /// Fill GPU util, disk I/O, and VRAM into `sample` (best-effort).
    pub fn enrich(&mut self, sample: &mut Sample) {
        if let Some(pdh) = self.pdh.as_mut() {
            if let Some(reading) = pdh.collect() {
                // VRAM is a gauge — valid on the first collection too.
                if reading.vram_used_bytes > 0 {
                    sample.vram_used_bytes = Some(reading.vram_used_bytes);
                }
                // Util/rate counters need a baseline — emit from the 2nd tick.
                if !self.first {
                    sample.gpu_percent = Some(aggregate_3d_util(&reading.gpu_instances));
                    sample.disk_read_bps = reading.disk_read_bps;
                    sample.disk_write_bps = reading.disk_write_bps;
                }
            }
        }
        self.first = false;

        if let Some(total) = self.vram_total {
            sample.vram_total_bytes = Some(total);
        }
    }
}

/// One PDH collection's results.
struct PdhReading {
    gpu_instances: Vec<(String, f64)>,
    disk_read_bps: i64,
    disk_write_bps: i64,
    /// System-wide dedicated VRAM in use (bytes), summed across adapters.
    vram_used_bytes: i64,
}

mod pdh {
    use super::PdhReading;
    use windows::core::w;
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT_COUNTERVALUE,
        PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
    };

    const ERROR_SUCCESS: u32 = 0;
    const PDH_MORE_DATA: u32 = 0x8000_07D2;

    /// Live PDH query for GPU engine utilization + disk throughput + VRAM usage.
    /// `vram` is optional — the GPU-memory counter may be absent on some systems,
    /// and that must not lose the (core) GPU/disk counters.
    pub struct PdhCounters {
        query: isize,
        gpu: isize,
        disk_read: isize,
        disk_write: isize,
        vram: Option<isize>,
    }

    impl PdhCounters {
        /// Open the query and add counters, or `None` if PDH is unavailable.
        pub fn open() -> Option<Self> {
            // SAFETY: standard PDH setup; every status is checked and the query is
            // closed on Drop. Counter handles are owned by the query.
            unsafe {
                let mut query: isize = 0;
                if PdhOpenQueryW(None, 0, &mut query) != ERROR_SUCCESS {
                    return None;
                }
                let add = |path| {
                    let mut counter: isize = 0;
                    let status = PdhAddEnglishCounterW(query, path, 0, &mut counter);
                    (status == ERROR_SUCCESS).then_some(counter)
                };
                let gpu = add(w!("\\GPU Engine(*)\\Utilization Percentage"));
                let disk_read = add(w!("\\PhysicalDisk(_Total)\\Disk Read Bytes/sec"));
                let disk_write = add(w!("\\PhysicalDisk(_Total)\\Disk Write Bytes/sec"));
                let (Some(gpu), Some(disk_read), Some(disk_write)) = (gpu, disk_read, disk_write)
                else {
                    let _ = PdhCloseQuery(query);
                    return None;
                };
                // System-wide dedicated VRAM in use (optional — not on every box).
                let vram = add(w!("\\GPU Adapter Memory(*)\\Dedicated Usage"));
                // Prime the counters (rate/util counters need a baseline sample).
                let _ = PdhCollectQueryData(query);
                Some(Self {
                    query,
                    gpu,
                    disk_read,
                    disk_write,
                    vram,
                })
            }
        }

        /// Collect a fresh sample, or `None` if the collection failed.
        pub fn collect(&mut self) -> Option<PdhReading> {
            // SAFETY: query + counter handles are valid for self's lifetime.
            unsafe {
                if PdhCollectQueryData(self.query) != ERROR_SUCCESS {
                    return None;
                }
                let vram_used_bytes = self
                    .vram
                    .map(|c| read_counter_array(c).iter().map(|(_, v)| *v).sum::<f64>())
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .map(|v| v as i64)
                    .unwrap_or(0);
                Some(PdhReading {
                    gpu_instances: read_counter_array(self.gpu),
                    disk_read_bps: read_counter_value(self.disk_read),
                    disk_write_bps: read_counter_value(self.disk_write),
                    vram_used_bytes,
                })
            }
        }
    }

    impl Drop for PdhCounters {
        fn drop(&mut self) {
            // SAFETY: closing a valid query also frees its counters.
            unsafe {
                let _ = PdhCloseQuery(self.query);
            }
        }
    }

    /// Read a single formatted double counter as `i64` (0 on failure).
    unsafe fn read_counter_value(counter: isize) -> i64 {
        let mut value = PDH_FMT_COUNTERVALUE::default();
        if PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, None, &mut value) != ERROR_SUCCESS {
            return 0;
        }
        // doubleValue is valid because we requested PDH_FMT_DOUBLE.
        let v = value.Anonymous.doubleValue;
        if v.is_finite() && v >= 0.0 {
            v as i64
        } else {
            0
        }
    }

    /// Read a wildcard counter's instances as `(name, value)` pairs (empty on
    /// failure — e.g. admin-gated GPU counters).
    unsafe fn read_counter_array(counter: isize) -> Vec<(String, f64)> {
        let mut size: u32 = 0;
        let mut count: u32 = 0;
        let status =
            PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None);
        if status != PDH_MORE_DATA || size == 0 {
            return Vec::new();
        }
        // 8-aligned backing store: PDH writes an array of
        // PDH_FMT_COUNTERVALUE_ITEM_W (align 8) plus trailing name strings into
        // this blob. A `Vec<u8>` isn't guaranteed 8-aligned, so back it with
        // `u64` to keep the cast sound. `size` is a byte count.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let status = PdhGetFormattedCounterArrayW(
            counter,
            PDH_FMT_DOUBLE,
            &mut size,
            &mut count,
            Some(buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>()),
        );
        if status != ERROR_SUCCESS {
            return Vec::new();
        }
        let items = std::slice::from_raw_parts(
            buffer.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
            count as usize,
        );
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            if item.szName.is_null() {
                continue;
            }
            let name = item.szName.to_string().unwrap_or_default();
            let value = item.FmtValue.Anonymous.doubleValue;
            if value.is_finite() {
                out.push((name, value));
            }
        }
        out
    }
}

mod dxgi {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

    /// Total **dedicated** VRAM (bytes) of the largest adapter — the discrete GPU
    /// on a hybrid laptop. This is the fixed capacity Task Manager shows as
    /// "Dedicated GPU memory" total; usage comes from PDH, not from DXGI's
    /// per-process `QueryVideoMemoryInfo`. `None` if DXGI is unavailable.
    pub fn total_vram() -> Option<i64> {
        // SAFETY: standard DXGI enumeration; `GetDesc1` fills a POD struct and we
        // stop at the first enumeration error. COM released by the RAII wrapper.
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            let mut best: u64 = 0;
            let mut index = 0u32;
            while let Ok(adapter) = factory.EnumAdapters1(index) {
                if let Ok(desc) = adapter.GetDesc1() {
                    best = best.max(desc.DedicatedVideoMemory as u64);
                }
                index += 1;
            }
            (best > 0).then(|| i64::try_from(best).unwrap_or(i64::MAX))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_only_3d_engine_instances() {
        let instances = vec![
            ("pid_1_eng_0_engtype_3D".to_string(), 30.0),
            ("pid_2_eng_1_engtype_3D".to_string(), 25.0),
            ("pid_3_eng_2_engtype_Copy".to_string(), 99.0), // ignored
            ("pid_4_eng_3_engtype_VideoDecode".to_string(), 80.0), // ignored
        ];
        assert_eq!(aggregate_3d_util(&instances), 55.0);
    }

    #[test]
    fn aggregate_clamps_to_100() {
        let instances = vec![
            ("a_engtype_3D".to_string(), 70.0),
            ("b_engtype_3D".to_string(), 70.0),
        ];
        assert_eq!(aggregate_3d_util(&instances), 100.0);
    }

    #[test]
    fn aggregate_empty_is_zero() {
        assert_eq!(aggregate_3d_util(&[]), 0.0);
    }

    // Diagnostic: prints what WMI/PDH/DXGI actually return on this machine.
    // Run: cargo test --lib probe_real_gpu -- --ignored --nocapture
    #[test]
    #[ignore = "diagnostic; needs a real GPU"]
    fn probe_real_gpu() {
        let gpus = crate::gpu::get_gpus(&crate::os::wmi::WindowsWmi);
        eprintln!("=== WMI get_gpus ===\n{gpus:#?}");

        match super::pdh::PdhCounters::open() {
            Some(mut pdh) => {
                let _ = pdh.collect();
                std::thread::sleep(std::time::Duration::from_millis(1100));
                match pdh.collect() {
                    Some(r) => {
                        eprintln!("=== PDH gpu_instances ({}) ===", r.gpu_instances.len());
                        for (n, v) in r.gpu_instances.iter().take(40) {
                            eprintln!("  {n} = {v}");
                        }
                        eprintln!(
                            "disk_read_bps={} disk_write_bps={}",
                            r.disk_read_bps, r.disk_write_bps
                        );
                    }
                    None => eprintln!("PDH collect -> None"),
                }
            }
            None => eprintln!("PDH open -> None (counters unavailable/admin-gated)"),
        }

        let mut s = crate::models::Sample {
            cpu_percent: 0.0,
            mem_used_bytes: 0,
            mem_total_bytes: 0,
            net_rx_bps: 0,
            net_tx_bps: 0,
            disk_read_bps: 0,
            disk_write_bps: 0,
            gpu_percent: None,
            vram_used_bytes: None,
            vram_total_bytes: None,
        };
        let mut c = GpuCounters::new();
        c.enrich(&mut s);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        c.enrich(&mut s);
        eprintln!(
            "=== sample: gpu_percent={:?} vram_used={:?} vram_total={:?}",
            s.gpu_percent, s.vram_used_bytes, s.vram_total_bytes
        );
    }
}
