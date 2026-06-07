//! System monitor (plan Tasks 17–18, M3, FR-MONITOR).
//!
//! One async task samples at a fixed interval and emits `monitor://sample`. The
//! task is owned by [`crate::state::Monitor`]; `monitor_start`/`monitor_stop`
//! spawn/abort it, so when the Monitor view is hidden or the window is blurred
//! the task is gone entirely — idle CPU ≈ 0 (no paused-but-spinning loop).
//!
//! `metrics` provides CPU/memory/network (sysinfo); `gpu_counters` (Windows)
//! enriches GPU util, VRAM, and disk I/O via PDH/DXGI (Task 18).

pub mod metrics;

#[cfg(windows)]
pub mod gpu_counters;

use crate::models::Sample;
use std::time::Duration;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter};

/// Spawn the sampler loop, emitting a [`Sample`] every `interval_ms`. Returns the
/// task handle so the caller can abort it on stop. The loop ends itself if the
/// emit fails (window gone).
pub fn spawn_sampler(app: AppHandle, interval_ms: u64) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut source = metrics::MetricsSource::new();
        #[cfg(windows)]
        let mut gpu = gpu_counters::GpuCounters::new();
        let mut ticker = tokio::time::interval(Duration::from_millis(interval_ms));

        loop {
            ticker.tick().await;
            #[allow(unused_mut)] // mutated only by the Windows GPU enrich below
            let mut sample: Sample = source.sample();
            #[cfg(windows)]
            gpu.enrich(&mut sample);
            if app.emit("monitor://sample", sample).is_err() {
                break;
            }
        }
    })
}
