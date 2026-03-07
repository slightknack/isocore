// created: 2026-03-05
// updated: 2026-03-06
// model: claude-opus-4-6
// driver: Isaac Clayton
// provenance: ai

//! Streaming append-only merkle log.
//!
//! Uses positional grouping (base-B digit decomposition) for O(log_B N)
//! incremental append. Each "digit" of the entry count in base B
//! corresponds to a level in the tree. When a digit overflows (reaches B),
//! the B children at that level merge into a parent node via carry
//! propagation — exactly like addition in base B.

use crate::hash;
use crate::proof::{self, Proof, ProofLevel};
use crate::store::Store;

/// Serialize a branch node: concatenated 32-byte child hashes.
fn serialize_branch(children: &[[u8; 32]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(children.len() * 32);
    for child in children {
        out.extend_from_slice(child);
    }
    out
}

/// Deserialize a branch node back into child hashes.
fn deserialize_branch(data: &[u8], b: usize) -> Option<Vec<[u8; 32]>> {
    if data.len() != b * 32 {
        return None;
    }
    let mut children = Vec::with_capacity(b);
    for chunk in data.chunks_exact(32) {
        let mut hash = [0u8; 32];
        hash.copy_from_slice(chunk);
        children.push(hash);
    }
    Some(children)
}

/// A streaming append-only merkle log with branching factor `B`.
///
/// `B` must be a power of 2 and at least 2. The default is 16.
///
/// The log stores entry data and internal nodes in a [`Store`].
/// Only the roots vector and length are kept in memory.
pub struct MerkleLog<S: Store, const B: usize = 16> {
    store: S,
    roots: Vec<[u8; 32]>,
    length: u64,
}

impl<S: Store, const B: usize> MerkleLog<S, B> {
    /// Create a new empty log.
    ///
    /// # Panics
    ///
    /// Panics if `B < 2` or `B` is not a power of 2.
    pub fn new(store: S) -> Self {
        assert!(B >= 2, "branching factor must be at least 2");
        assert!(B.is_power_of_two(), "branching factor must be a power of 2");
        Self {
            store,
            roots: Vec::new(),
            length: 0,
        }
    }

    /// Number of entries appended so far.
    pub fn length(&self) -> u64 {
        self.length
    }

    /// Current root hashes (one per incomplete subtree).
    pub fn roots(&self) -> &[[u8; 32]] {
        &self.roots
    }

    /// A reference to the underlying store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// A mutable reference to the underlying store.
    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    /// Compute a single hash suitable for signing.
    ///
    /// This is `BLAKE3(0x02 || root0 || root1 || ... || length_le)`.
    /// The caller signs this hash with their own crypto.
    pub fn signable(&self) -> [u8; 32] {
        hash::hash_root(&self.roots, self.length)
    }

    /// Append an entry and return its leaf hash.
    ///
    /// The entry data is stored under its leaf hash. Internal parent
    /// nodes are created as needed via carry propagation.
    pub fn append(&mut self, data: &[u8]) -> Result<[u8; 32], S::Error> {
        let leaf_hash = hash::hash_leaf(data);
        self.store.put(leaf_hash, data)?;

        let mut carry = leaf_hash;
        let mut n = self.length;

        loop {
            let digit = (n % B as u64) as usize;
            if digit == B - 1 {
                // This level is full — merge B children into a parent.
                let start = self.roots.len() - (B - 1);
                let mut children: Vec<[u8; 32]> = self.roots.drain(start..).collect();
                children.push(carry);
                let branch_data = serialize_branch(&children);
                carry = hash::hash_parent(&children);
                self.store.put(carry, &branch_data)?;
                n /= B as u64;
            } else {
                self.roots.push(carry);
                break;
            }
        }

        self.length += 1;
        Ok(leaf_hash)
    }

    /// Generate a membership proof for the entry at `index`.
    ///
    /// The proof can be verified against the subtree root that contains
    /// this index. For a complete tree (single root), that's the only root.
    pub fn prove(&self, index: u64) -> Result<Proof, S::Error> {
        if index >= self.length {
            panic!("index {} out of range (length {})", index, self.length);
        }

        // Find which subtree contains this index.
        let sizes = subtree_sizes::<B>(self.length);
        let mut subtree_start: u64 = 0;
        let mut subtree_root_idx = 0;
        let mut subtree_size: u64 = 0;

        for (i, &size) in sizes.iter().enumerate() {
            if index < subtree_start + size {
                subtree_root_idx = i;
                subtree_size = size;
                break;
            }
            subtree_start += size;
        }

        let root_hash = self.roots[subtree_root_idx];
        let relative_index = index - subtree_start;

        // Walk down the tree from root to leaf.
        let mut levels = Vec::new();
        let mut current_hash = root_hash;
        let mut current_size = subtree_size;
        let mut current_index = relative_index;

        while current_size > 1 {
            let data = self.store.get(current_hash)?;
            let children = deserialize_branch(&data, B)
                .expect("corrupt branch node");

            let child_size = current_size / B as u64;
            let position = (current_index / child_size) as usize;

            levels.push(ProofLevel {
                siblings: proof::extract_siblings(&children, position),
                position,
            });

            current_hash = children[position];
            current_index %= child_size;
            current_size = child_size;
        }

        // Reverse: prove walks top-down, but verify expects bottom-up.
        levels.reverse();
        Ok(Proof { levels })
    }

    /// Serialize the log metadata (roots + length).
    ///
    /// Format: `length_le_u64 || root_count_le_u32 || root0 || root1 || ...`
    ///
    /// All tree data lives in the Store; this is just the ~300 byte header.
    pub fn save(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + self.roots.len() * 32);
        out.extend_from_slice(&self.length.to_le_bytes());
        out.extend_from_slice(&(self.roots.len() as u32).to_le_bytes());
        for root in &self.roots {
            out.extend_from_slice(root);
        }
        out
    }

    /// Load log metadata from bytes, attaching to an existing store.
    pub fn load(store: S, data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }
        let length = u64::from_le_bytes(data[..8].try_into().ok()?);
        let root_count = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;
        if data.len() != 12 + root_count * 32 {
            return None;
        }
        let mut roots = Vec::with_capacity(root_count);
        for i in 0..root_count {
            let start = 12 + i * 32;
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&data[start..start + 32]);
            roots.push(hash);
        }
        Some(Self {
            store,
            roots,
            length,
        })
    }
}

/// Decompose `length` into subtree sizes in base B.
///
/// Returns sizes in decreasing order (largest subtree first).
/// Each size is a power of B. The roots of a `MerkleLog` correspond
/// one-to-one with these sizes.
pub fn subtree_sizes<const B: usize>(length: u64) -> Vec<u64> {
    let mut sizes = Vec::new();
    let mut remaining = length;
    let mut power: u64 = 1;

    while power * B as u64 <= remaining {
        power *= B as u64;
    }

    while remaining > 0 {
        while power <= remaining {
            sizes.push(power);
            remaining -= power;
        }
        power /= B as u64;
    }

    sizes
}
