// created: 2026-03-05
// updated: 2026-03-06
// model: claude-opus-4-6
// driver: Isaac Clayton
// provenance: ai

//! Streaming fixed-memory merkle tree with backend-agnostic storage.
//!
//! `neodata` implements a B-ary merkle tree (default B=16) that supports
//! O(log_B N) incremental append using positional grouping. The tree's
//! internal nodes are stored in a pluggable [`Store`] backend, keeping
//! only the root hashes and entry count in memory.
//!
//! # Quick start
//!
//! ```
//! use neodata::{MerkleLog, MemStore};
//!
//! let mut log = MerkleLog::<MemStore, 16>::new(MemStore::new());
//! let hash = log.append(b"hello").unwrap();
//! assert_eq!(log.length(), 1);
//! ```

pub mod diff;
pub mod hash;
pub mod log;
pub mod proof;
pub mod store;

#[cfg(test)]
mod tests;

pub use diff::{NodeBlock, collect_needed_nodes, verify_partial};
pub use log::{MerkleLog, subtree_sizes};
pub use proof::{Proof, ProofLevel, extract_siblings, reconstruct_children, verify};
pub use store::{MemStore, Store};
