//! Store context for running component instances.

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// Per-instance execution context stored in Wasmtime's Store.
///
/// Holds mutable state scoped to a single component instance. Each instance
/// maintains independent sequence numbering for RPC correlation.
///
/// # Thread Safety
///
/// Wasmtime's Store is !Send + !Sync, providing single-threaded access.
/// Interior mutability via AtomicU64 allows incrementing the sequence counter
/// without requiring &mut self.
pub struct ExorunCtx {
    seq: AtomicU64,
}

impl ExorunCtx {
    /// Creates a new context with sequence numbering starting at 1.
    pub fn new() -> Self {
        Self {
            seq: AtomicU64::new(1),
        }
    }

    /// Generates the next sequence number for outbound RPC calls.
    ///
    /// Increments atomically using relaxed ordering (sufficient since Store
    /// access is single-threaded).
    pub(crate) fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }
}

impl Default for ExorunCtx {
    fn default() -> Self {
        Self::new()
    }
}
