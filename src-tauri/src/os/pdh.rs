//! PDH seam (ADR-0005) for GPU engine utilization.

use crate::error::CoreResult;

/// Read-only PDH (Performance Data Helper) access.
pub trait Pdh {
    /// Aggregate GPU engine utilization (0.0–100.0), or `Ok(None)` when counters
    /// are unavailable or admin-gated (NVIDIA `RmProfilingAdminOnly`) — callers
    /// degrade gracefully rather than treating this as an error.
    fn gpu_engine_util(&self) -> CoreResult<Option<f32>>;
}

#[cfg(windows)]
pub use windows_impl::WindowsPdh;

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use crate::error::CoreError;

    /// Real PDH access. The counter query (`\GPU Engine(*)\Utilization Percentage`)
    /// lands in plan Task 18; placeholder keeps the seam compiling until then.
    pub struct WindowsPdh;

    impl Pdh for WindowsPdh {
        fn gpu_engine_util(&self) -> CoreResult<Option<f32>> {
            Err(CoreError::Unsupported(
                "PDH gpu_engine_util is implemented in plan Task 18".into(),
            ))
        }
    }
}

#[cfg(test)]
pub use fake::FakePdh;

#[cfg(test)]
mod fake {
    use super::*;

    /// In-memory PDH for tests. `None` models the unavailable/admin-gated case.
    #[derive(Default)]
    pub struct FakePdh {
        util: Option<f32>,
    }

    impl FakePdh {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn with_util(mut self, util: f32) -> Self {
            self.util = Some(util);
            self
        }
    }

    impl Pdh for FakePdh {
        fn gpu_engine_util(&self) -> CoreResult<Option<f32>> {
            Ok(self.util)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_engine_util_returns_seeded_value() {
        let pdh = FakePdh::new().with_util(42.5);
        assert_eq!(pdh.gpu_engine_util().unwrap(), Some(42.5));
    }

    #[test]
    fn gpu_engine_util_none_models_unavailable() {
        assert_eq!(FakePdh::new().gpu_engine_util().unwrap(), None);
    }
}
