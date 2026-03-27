//! Progress reporting and cancellation for long-running I/O operations.
//!
//! The [`Progress`] trait provides a callback interface for tracking
//! how much work has been completed.  Any `FnMut(u64, u64) -> bool`
//! closure implements it automatically, and [`NoProgress`] is a
//! zero-cost no-op when progress tracking is not needed.
//!
//! # Example
//!
//! ```
//! use nostdio::Progress;
//!
//! let mut calls = 0u32;
//! let mut progress = |done: u64, total: u64| -> bool {
//!     calls += 1;
//!     true // keep going
//! };
//!
//! assert!(progress.report(50, 100));
//! assert!(progress.report(100, 100));
//! assert_eq!(calls, 2);
//! ```

/// Progress reporting and cancellation for long-running I/O operations.
///
/// Return `true` from [`report`](Progress::report) to continue, or
/// `false` to request cancellation.
///
/// # Using a closure
///
/// Any `FnMut(u64, u64) -> bool` implements `Progress` automatically:
///
/// ```
/// use nostdio::Progress;
///
/// let mut progress = |done: u64, total: u64| -> bool {
///     done < total // cancel when complete
/// };
///
/// assert!(progress.report(5, 10));   // keep going
/// assert!(!progress.report(10, 10)); // cancel
/// ```
///
/// # Opting out
///
/// Pass [`NoProgress`] when progress tracking is not needed — it compiles
/// to a no-op.
///
/// ```
/// use nostdio::{NoProgress, Progress};
///
/// let mut np = NoProgress;
/// assert!(np.report(0, 0)); // always returns true
/// ```
pub trait Progress {
    /// Called after each unit of work.
    ///
    /// * `bytes_done`  — cumulative bytes processed so far.
    /// * `total_bytes` — expected total (may be 0 if unknown).
    ///
    /// Return `true` to continue, `false` to cancel.
    fn report(&mut self, bytes_done: u64, total_bytes: u64) -> bool;
}

impl<F: FnMut(u64, u64) -> bool> Progress for F {
    #[inline]
    fn report(&mut self, bytes_done: u64, total_bytes: u64) -> bool {
        self(bytes_done, total_bytes)
    }
}

/// No-op [`Progress`] implementation that never cancels.
pub struct NoProgress;

impl Progress for NoProgress {
    #[inline]
    fn report(&mut self, _: u64, _: u64) -> bool {
        true
    }
}
