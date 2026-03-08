// created: 2025-03-07
// model: claude-opus-4-6
// driver: Isaac Clayton
// provenance: ai

//! Sync proof primitives for merkle tree diff and verification.
//!
//! When a subscriber is missing entries, the sender walks its tree
//! top-down and emits the internal nodes (blocks of B child hashes)
//! that the subscriber needs. Each block is content-addressed:
//! `hash_parent(children)` determines its position in the tree.
//! No position metadata needed on the wire.

use std::collections::HashMap;

use crate::hash;
use crate::log;
use crate::log::MerkleLog;
use crate::log::subtree_sizes;
use crate::store::Store;

/// A block of child hashes from one internal tree node.
///
/// Content-addressed: `hash_parent(children)` determines where this
/// block belongs in the tree. The subscriber hashes the block and
/// looks up the result in its partial tree — same principle as
/// content-addressed storage.
///
/// Wire format: `[B × 32-byte hash]` (e.g., 512 bytes for B=16).
#[derive(Clone, Debug)]
pub struct NodeBlock {
    pub children: Vec<[u8; 32]>,
}

impl NodeBlock {
    /// The hash of this internal node.
    pub fn hash(&self) -> [u8; 32] {
        hash::hash_parent(&self.children)
    }

    /// Serialize: concatenated 32-byte child hashes.
    pub fn to_bytes(&self) -> Vec<u8> {
        log::serialize_branch(&self.children)
    }

    /// Deserialize from concatenated child hashes.
    pub fn from_bytes(data: &[u8], b: usize) -> Option<Self> {
        let children = log::deserialize_branch(data, b)?;
        Some(NodeBlock { children })
    }
}

/// Collect proof nodes that a subscriber needs to verify the tree.
///
/// Walks the tree top-down, emitting each internal node whose children
/// overlap with missing seqs. Subtrees where the subscriber has all
/// entries are skipped entirely (the subscriber can compute those
/// bottom-up from its leaves).
///
/// Returns nodes in top-down order (parent before children).
///
/// The caller should also send `log.roots()` to the subscriber so it
/// can verify them against the signable hash.
pub fn collect_needed_nodes<S: Store, const B: usize>(
    log: &MerkleLog<S, B>,
    has_seq: impl Fn(u64) -> bool,
) -> Result<Vec<NodeBlock>, S::Error> {
    let sizes = subtree_sizes::<B>(log.length());
    let mut out = Vec::new();
    let mut offset: u64 = 0;

    for (i, &size) in sizes.iter().enumerate() {
        collect_subtree::<S, B>(
            log.store(),
            log.roots()[i],
            size,
            offset,
            &has_seq,
            &mut out,
        )?;
        offset += size;
    }

    Ok(out)
}

fn collect_subtree<S: Store, const B: usize>(
    store: &S,
    node_hash: [u8; 32],
    size: u64,
    offset: u64,
    has_seq: &impl Fn(u64) -> bool,
    out: &mut Vec<NodeBlock>,
) -> Result<(), S::Error> {
    // Leaves produce no proof nodes.
    if size <= 1 {
        return Ok(());
    }

    // If subscriber has all entries under this node, skip.
    if (offset..offset + size).all(|seq| has_seq(seq)) {
        return Ok(());
    }

    // Load this internal node's children from the store.
    let data = store.get(node_hash)?;
    let children = log::deserialize_branch(&data, B)
        .expect("valid branch node");

    out.push(NodeBlock { children: children.clone() });

    // Recurse into children (the all_present check handles skipping).
    let child_size = size / B as u64;
    for (i, &child_hash) in children.iter().enumerate() {
        let child_offset = offset + i as u64 * child_size;
        collect_subtree::<S, B>(
            store,
            child_hash,
            child_size,
            child_offset,
            has_seq,
            out,
        )?;
    }

    Ok(())
}

/// Verify entries against a signable root using proof nodes.
///
/// Walks the tree top-down. At each internal node, if a proof node
/// provides the children, it recurses into them. Otherwise, it
/// computes bottom-up from leaf hashes (for subtrees where the
/// subscriber has all entries).
///
/// - `signable`: the expected `hash_root(roots, length)` from the
///   signed root.
/// - `roots`: the writer's subtree root hashes (sent alongside proof
///   nodes).
/// - `length`: total entries in the writer's log (from the signed root
///   seq).
/// - `nodes`: proof nodes received during sync.
/// - `leaf_hash`: returns the leaf hash for a seq the subscriber holds.
///   Return `None` for seqs the subscriber doesn't have.
///
/// Returns `true` if the tree is consistent with the signable hash.
pub fn verify_partial<const B: usize>(
    signable: [u8; 32],
    roots: &[[u8; 32]],
    length: u64,
    nodes: &[NodeBlock],
    leaf_hash: impl Fn(u64) -> Option<[u8; 32]>,
) -> bool {
    // Verify roots against signable.
    if hash::hash_root(roots, length) != signable {
        return false;
    }

    // Build lookup: node hash → children.
    let mut lookup: HashMap<[u8; 32], &[[u8; 32]]> = HashMap::new();
    for node in nodes {
        lookup.insert(node.hash(), &node.children);
    }

    // Verify each subtree root.
    let sizes = subtree_sizes::<B>(length);
    let mut offset: u64 = 0;
    for (i, &size) in sizes.iter().enumerate() {
        if !verify_node::<B>(&lookup, roots[i], size, offset, &leaf_hash) {
            return false;
        }
        offset += size;
    }

    true
}

/// Verify a single tree node recursively.
///
/// Uses proof nodes (top-down) where available, falls back to
/// bottom-up computation from leaf hashes for complete subtrees.
fn verify_node<const B: usize>(
    lookup: &HashMap<[u8; 32], &[[u8; 32]]>,
    expected: [u8; 32],
    size: u64,
    offset: u64,
    leaf_hash: &impl Fn(u64) -> Option<[u8; 32]>,
) -> bool {
    if size == 1 {
        // Leaf: check hash matches.
        return leaf_hash(offset) == Some(expected);
    }

    let child_size = size / B as u64;

    if let Some(children) = lookup.get(&expected) {
        // Proof node found. Verify each child recursively.
        for (i, &child_hash) in children.iter().enumerate() {
            let child_offset = offset + i as u64 * child_size;
            if !verify_node::<B>(lookup, child_hash, child_size, child_offset, leaf_hash) {
                return false;
            }
        }
        true
    } else {
        // No proof node. Try to compute bottom-up from leaves.
        match compute_bottom_up::<B>(size, offset, leaf_hash) {
            Some(computed) => computed == expected,
            None => false,
        }
    }
}

/// Compute a subtree hash bottom-up from leaf hashes.
///
/// Returns `None` if any leaf hash is missing.
fn compute_bottom_up<const B: usize>(
    size: u64,
    offset: u64,
    leaf_hash: &impl Fn(u64) -> Option<[u8; 32]>,
) -> Option<[u8; 32]> {
    if size == 1 {
        return leaf_hash(offset);
    }

    let child_size = size / B as u64;
    let mut children = Vec::with_capacity(B);
    for i in 0..B {
        let child_offset = offset + i as u64 * child_size;
        children.push(compute_bottom_up::<B>(child_size, child_offset, leaf_hash)?);
    }
    Some(hash::hash_parent(&children))
}
