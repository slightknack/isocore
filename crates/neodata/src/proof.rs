// created: 2026-03-05
// updated: 2026-03-06
// model: claude-opus-4-6
// driver: Isaac Clayton
// provenance: ai

//! Membership proofs for merkle logs.

use crate::hash;

/// A membership proof for a single entry in a merkle log.
#[derive(Clone, Debug)]
pub struct Proof {
    pub levels: Vec<ProofLevel>,
}

/// One level of a membership proof.
#[derive(Clone, Debug)]
pub struct ProofLevel {
    /// Sibling hashes at this level (B-1 of them).
    pub siblings: Vec<[u8; 32]>,
    /// Which child slot the proven entry occupies (0..B).
    pub position: usize,
}

/// Extract B-1 siblings from a full children array, excluding `position`.
pub fn extract_siblings(children: &[[u8; 32]], position: usize) -> Vec<[u8; 32]> {
    children.iter()
        .enumerate()
        .filter(|&(i, _)| i != position)
        .map(|(_, h)| *h)
        .collect()
}

/// Rebuild the full B-element children array from B-1 siblings and the entry at `position`.
pub fn reconstruct_children<const B: usize>(
    siblings: &[[u8; 32]],
    entry: [u8; 32],
    position: usize,
) -> Vec<[u8; 32]> {
    let mut children = Vec::with_capacity(B);
    let mut sib_idx = 0;
    for i in 0..B {
        if i == position {
            children.push(entry);
        } else {
            children.push(siblings[sib_idx]);
            sib_idx += 1;
        }
    }
    children
}

/// Verify a membership proof against a known root.
///
/// Recomputes the root from `leaf_hash` upward using the proof siblings,
/// then checks it matches `expected_root`.
pub fn verify<const B: usize>(
    proof: &Proof,
    leaf_hash: [u8; 32],
    expected_root: [u8; 32],
) -> bool {
    let mut current = leaf_hash;

    for level in &proof.levels {
        if level.siblings.len() != B - 1 || level.position >= B {
            return false;
        }
        let children = reconstruct_children::<B>(&level.siblings, current, level.position);
        current = hash::hash_parent(&children);
    }

    current == expected_root
}
