//! System metrics sampling (plan Task 17, M3, FR-MONITOR).
//!
//! [`MetricsSource`] keeps reusable `sysinfo` handles and the previous network
//! counters so each tick is a **targeted** refresh (CPU/memory/network only — no
//! full process scan), meeting the perf goal (<1% CPU at 1Hz). Cumulative
//! network counters become per-second rates via the pure, unit-tested [`rate`].
//! GPU + disk-I/O fields are filled by the caller (Task 18); here they're zero.

use crate::models::Sample;
use std::time::Instant;
use sysinfo::{Networks, System};

/// Per-second rate between two cumulative counter readings. A counter reset
/// (`curr < prev`, e.g. interface re-added) or non-positive elapsed yields `0`
/// rather than a bogus spike.
pub fn rate(prev: u64, curr: u64, elapsed_secs: f64) -> i64 {
    if elapsed_secs <= 0.0 || curr < prev {
        return 0;
    }
    ((curr - prev) as f64 / elapsed_secs) as i64
}

fn clamp_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[derive(Clone, Copy, Default)]
struct NetCounters {
    rx: u64,
    tx: u64,
}

/// Reusable sampling state: one `System` + `Networks`, refreshed in place.
pub struct MetricsSource {
    system: System,
    networks: Networks,
    prev_net: NetCounters,
    last: Instant,
}

impl Default for MetricsSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsSource {
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_memory();
        system.refresh_cpu_usage();
        let networks = Networks::new_with_refreshed_list();
        let prev_net = net_totals(&networks);
        Self {
            system,
            networks,
            prev_net,
            last: Instant::now(),
        }
    }

    /// Refresh CPU/memory/network and produce a [`Sample`]. GPU/disk fields are
    /// left at their defaults for the caller (Task 18) to enrich.
    pub fn sample(&mut self) -> Sample {
        self.system.refresh_memory();
        self.system.refresh_cpu_usage();
        self.networks.refresh(true);

        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;

        let net = net_totals(&self.networks);
        let net_rx_bps = rate(self.prev_net.rx, net.rx, elapsed);
        let net_tx_bps = rate(self.prev_net.tx, net.tx, elapsed);
        self.prev_net = net;

        Sample {
            cpu_percent: self.system.global_cpu_usage(),
            mem_used_bytes: clamp_i64(self.system.used_memory()),
            mem_total_bytes: clamp_i64(self.system.total_memory()),
            net_rx_bps,
            net_tx_bps,
            disk_read_bps: 0,
            disk_write_bps: 0,
            gpu_percent: None,
            vram_used_bytes: None,
            vram_total_bytes: None,
        }
    }
}

fn net_totals(networks: &Networks) -> NetCounters {
    let mut totals = NetCounters::default();
    for (_iface, data) in networks {
        totals.rx = totals.rx.saturating_add(data.total_received());
        totals.tx = totals.tx.saturating_add(data.total_transmitted());
    }
    totals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_computes_bytes_per_second() {
        assert_eq!(rate(1_000, 3_000, 2.0), 1_000);
        assert_eq!(rate(0, 500, 1.0), 500);
    }

    #[test]
    fn rate_handles_counter_reset_and_zero_elapsed() {
        assert_eq!(
            rate(5_000, 1_000, 1.0),
            0,
            "reset → 0, not a negative/spike"
        );
        assert_eq!(rate(0, 1_000, 0.0), 0, "no elapsed time → 0");
    }

    #[test]
    fn sample_reports_plausible_memory() {
        // Light integration check against the host: total memory is positive and
        // used never exceeds total.
        let mut source = MetricsSource::new();
        let s = source.sample();
        assert!(s.mem_total_bytes > 0);
        assert!(s.mem_used_bytes >= 0 && s.mem_used_bytes <= s.mem_total_bytes);
        assert!(s.cpu_percent >= 0.0 && s.cpu_percent <= 100.0);
    }
}
