// created: 2026-03-05
// updated: 2026-03-06
// model: claude-opus-4-6
// driver: Isaac Clayton
// provenance: ai

//! Domain-separated BLAKE3 hashing.

const DOMAIN_LEAF: u8 = 0x00;
const DOMAIN_PARENT: u8 = 0x01;
const DOMAIN_ROOT: u8 = 0x02;

/// Hash a leaf: `BLAKE3(0x00 || data)`.
pub fn hash_leaf(data: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[DOMAIN_LEAF]);
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

/// Hash a parent node: `BLAKE3(0x01 || child0 || child1 || ...)`.
pub fn hash_parent(children: &[[u8; 32]]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[DOMAIN_PARENT]);
    for child in children {
        hasher.update(child);
    }
    *hasher.finalize().as_bytes()
}

/// Hash for signing: `BLAKE3(0x02 || root0 || root1 || ... || length_le)`.
pub fn hash_root(roots: &[[u8; 32]], length: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&[DOMAIN_ROOT]);
    for root in roots {
        hasher.update(root);
    }
    hasher.update(&length.to_le_bytes());
    *hasher.finalize().as_bytes()
}
