// created: 2026-03-05
// updated: 2026-03-05
// model: claude-opus-4-6
// driver: Isaac Clayton
// provenance: ai

use std::collections::HashMap;
use std::fmt;

/// Backend-agnostic content-addressed storage.
///
/// Maps directly to freight's `write_chunk_raw` / `read_chunk_raw`.
pub trait Store {
    type Error: fmt::Debug;

    /// Store data under its hash. Idempotent — storing the same hash twice is a no-op.
    fn put(&mut self, hash: [u8; 32], data: &[u8]) -> Result<(), Self::Error>;

    /// Retrieve data by hash.
    fn get(&self, hash: [u8; 32]) -> Result<Vec<u8>, Self::Error>;
}

/// In-memory store for testing.
#[derive(Clone, Debug, Default)]
pub struct MemStore {
    map: HashMap<[u8; 32], Vec<u8>>,
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Error type for [`MemStore`].
#[derive(Debug, Clone)]
pub struct NotFound;

impl fmt::Display for NotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not found")
    }
}

impl Store for MemStore {
    type Error = NotFound;

    fn put(&mut self, hash: [u8; 32], data: &[u8]) -> Result<(), NotFound> {
        self.map.entry(hash).or_insert_with(|| data.to_vec());
        Ok(())
    }

    fn get(&self, hash: [u8; 32]) -> Result<Vec<u8>, NotFound> {
        self.map.get(&hash).cloned().ok_or(NotFound)
    }
}
