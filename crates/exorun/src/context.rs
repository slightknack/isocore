//! Store context for running component instances.

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// The Store context for a running component instance.
///
/// This is instance-scoped state that lives inside the Wasmtime Store.
/// Each instance has its own isolated context with independent sequence numbering.
pub struct ExorunCtx {
    seq: AtomicU64,
}

impl ExorunCtx {
    /// Create a new context with sequence numbering starting at 1.
    pub fn new() -> Self {
        Self {
            seq: AtomicU64::new(1),
        }
    }

    /// Generate the next sequence number for RPC calls from this instance.
    pub(crate) fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }
}

impl Default for ExorunCtx {
    fn default() -> Self {
        Self::new()
    }
}
