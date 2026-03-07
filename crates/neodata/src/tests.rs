// created: 2026-03-06
// model: claude-opus-4-6
// driver: Isaac Clayton
// provenance: ai

use crate::hash;
use crate::log::{MerkleLog, subtree_sizes};
use crate::proof;
use crate::store::{MemStore, Store};

// ── hash ──

#[test]
fn domain_separation() {
    let data = b"hello";
    let leaf = hash::hash_leaf(data);
    let parent = hash::hash_parent(&[leaf]);
    let root = hash::hash_root(&[leaf], 1);
    assert_ne!(leaf, parent);
    assert_ne!(leaf, root);
    assert_ne!(parent, root);
}

#[test]
fn hash_deterministic() {
    assert_eq!(hash::hash_leaf(b"x"), hash::hash_leaf(b"x"));
    assert_ne!(hash::hash_leaf(b"x"), hash::hash_leaf(b"y"));
}

#[test]
fn hash_leaf_empty() {
    let h = hash::hash_leaf(b"");
    assert_ne!(h, [0u8; 32]);
    assert_eq!(h, hash::hash_leaf(b""));
}

#[test]
fn hash_parent_multiple_children() {
    let a = hash::hash_leaf(b"a");
    let b = hash::hash_leaf(b"b");
    let c = hash::hash_leaf(b"c");
    let d = hash::hash_leaf(b"d");
    let h = hash::hash_parent(&[a, b, c, d]);
    assert_ne!(h, hash::hash_parent(&[d, c, b, a]));
}

#[test]
fn hash_root_multiple_roots() {
    let a = hash::hash_leaf(b"a");
    let b = hash::hash_leaf(b"b");
    let h1 = hash::hash_root(&[a, b], 2);
    let h2 = hash::hash_root(&[a, b], 3);
    assert_ne!(h1, h2, "different lengths must produce different root hashes");
}

// ── store ──

#[test]
fn memstore_put_get() {
    let mut store = MemStore::new();
    let hash = [42u8; 32];
    store.put(hash, b"data").unwrap();
    assert_eq!(store.get(hash).unwrap(), b"data");
    assert!(store.get([0u8; 32]).is_err());
}

#[test]
fn memstore_len_and_is_empty() {
    let mut store = MemStore::new();
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
    store.put([1u8; 32], b"x").unwrap();
    assert!(!store.is_empty());
    assert_eq!(store.len(), 1);
    store.put([2u8; 32], b"y").unwrap();
    assert_eq!(store.len(), 2);
}

#[test]
fn memstore_not_found_display() {
    let err = crate::store::NotFound;
    assert_eq!(format!("{err}"), "not found");
}

#[test]
fn memstore_put_idempotent() {
    let mut store = MemStore::new();
    let hash = [99u8; 32];
    store.put(hash, b"first").unwrap();
    store.put(hash, b"second").unwrap();
    assert_eq!(store.get(hash).unwrap(), b"first");
    assert_eq!(store.len(), 1);
}

// ── proof helpers ──

#[test]
fn extract_siblings_basic() {
    let children: Vec<[u8; 32]> = (0..4u8).map(|i| [i; 32]).collect();
    let sibs = proof::extract_siblings(&children, 1);
    assert_eq!(sibs.len(), 3);
    assert_eq!(sibs[0], [0u8; 32]);
    assert_eq!(sibs[1], [2u8; 32]);
    assert_eq!(sibs[2], [3u8; 32]);
}

#[test]
fn reconstruct_children_basic() {
    let siblings = vec![[0u8; 32], [2u8; 32], [3u8; 32]];
    let entry = [1u8; 32];
    let children = proof::reconstruct_children::<4>(&siblings, entry, 1);
    assert_eq!(children.len(), 4);
    assert_eq!(children[0], [0u8; 32]);
    assert_eq!(children[1], [1u8; 32]);
    assert_eq!(children[2], [2u8; 32]);
    assert_eq!(children[3], [3u8; 32]);
}

#[test]
fn extract_then_reconstruct_roundtrip() {
    let children: Vec<[u8; 32]> = (0..4u8).map(|i| [i; 32]).collect();
    for pos in 0..4 {
        let sibs = proof::extract_siblings(&children, pos);
        let rebuilt = proof::reconstruct_children::<4>(&sibs, children[pos], pos);
        assert_eq!(rebuilt, children, "roundtrip failed for position {pos}");
    }
}

#[test]
fn verify_wrong_sibling_count() {
    let bad = proof::Proof {
        levels: vec![proof::ProofLevel {
            siblings: vec![[0u8; 32]], // only 1 sibling, need B-1=3
            position: 0,
        }],
    };
    assert!(!proof::verify::<4>(&bad, [0u8; 32], [0u8; 32]));
}

#[test]
fn verify_position_out_of_range() {
    let bad = proof::Proof {
        levels: vec![proof::ProofLevel {
            siblings: vec![[0u8; 32]; 3],
            position: 4, // position >= B=4
        }],
    };
    assert!(!proof::verify::<4>(&bad, [0u8; 32], [0u8; 32]));
}

// ── subtree_sizes ──

#[test]
fn subtree_sizes_correctness() {
    assert_eq!(subtree_sizes::<4>(16), vec![16]);
    assert_eq!(subtree_sizes::<4>(6), vec![4, 1, 1]);
    assert_eq!(subtree_sizes::<4>(5), vec![4, 1]);
    assert_eq!(subtree_sizes::<4>(21), vec![16, 4, 1]);
}

#[test]
fn subtree_sizes_zero() {
    assert!(subtree_sizes::<4>(0).is_empty());
    assert!(subtree_sizes::<16>(0).is_empty());
}

#[test]
fn subtree_sizes_one() {
    assert_eq!(subtree_sizes::<4>(1), vec![1]);
    assert_eq!(subtree_sizes::<16>(1), vec![1]);
}

// ── MerkleLog ──

#[test]
fn empty_log() {
    let log = MerkleLog::<MemStore, 16>::new(MemStore::new());
    assert_eq!(log.length(), 0);
    assert!(log.roots().is_empty());
}

#[test]
fn length_and_roots_accessors() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    assert_eq!(log.length(), 0);
    assert_eq!(log.roots().len(), 0);

    log.append(b"a").unwrap();
    assert_eq!(log.length(), 1);
    assert_eq!(log.roots().len(), 1);

    for i in 1..4u8 {
        log.append(&[i]).unwrap();
    }
    assert_eq!(log.length(), 4);
    assert_eq!(log.roots().len(), 1);

    log.append(b"e").unwrap();
    assert_eq!(log.length(), 5);
    assert_eq!(log.roots().len(), 2);
}

#[test]
fn store_and_store_mut_accessors() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    assert!(log.store().is_empty());
    log.append(b"hello").unwrap();
    assert!(!log.store().is_empty());
    assert_eq!(log.store().len(), 1);

    // store_mut allows direct access
    let s = log.store_mut();
    assert!(!s.is_empty());
}

#[test]
fn append_single() {
    let mut log = MerkleLog::<MemStore, 16>::new(MemStore::new());
    let h = log.append(b"hello").unwrap();
    assert_eq!(log.length(), 1);
    assert_eq!(log.roots().len(), 1);
    assert_eq!(log.roots()[0], h);
}

#[test]
fn append_b_entries_single_root() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    for i in 0..4u8 {
        log.append(&[i]).unwrap();
    }
    assert_eq!(log.length(), 4);
    assert_eq!(log.roots().len(), 1);
}

#[test]
fn append_b_plus_one_two_roots() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    for i in 0..5u8 {
        log.append(&[i]).unwrap();
    }
    assert_eq!(log.length(), 5);
    assert_eq!(log.roots().len(), 2);
}

#[test]
fn append_b_squared_single_root() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    for i in 0..16u8 {
        log.append(&[i]).unwrap();
    }
    assert_eq!(log.length(), 16);
    assert_eq!(log.roots().len(), 1);
}

#[test]
fn append_256_with_b16_single_root() {
    let mut log = MerkleLog::<MemStore, 16>::new(MemStore::new());
    for i in 0..256u32 {
        log.append(&i.to_le_bytes()).unwrap();
    }
    assert_eq!(log.length(), 256);
    assert_eq!(log.roots().len(), 1);
}

#[test]
fn deterministic_roots() {
    let mut log1 = MerkleLog::<MemStore, 4>::new(MemStore::new());
    let mut log2 = MerkleLog::<MemStore, 4>::new(MemStore::new());
    for i in 0..10u8 {
        log1.append(&[i]).unwrap();
        log2.append(&[i]).unwrap();
    }
    assert_eq!(log1.roots(), log2.roots());
    assert_eq!(log1.signable(), log2.signable());
}

#[test]
fn different_data_different_roots() {
    let mut log1 = MerkleLog::<MemStore, 4>::new(MemStore::new());
    let mut log2 = MerkleLog::<MemStore, 4>::new(MemStore::new());
    log1.append(b"a").unwrap();
    log2.append(b"b").unwrap();
    assert_ne!(log1.signable(), log2.signable());
}

#[test]
fn signable_changes_with_append() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    log.append(b"first").unwrap();
    let s1 = log.signable();
    log.append(b"second").unwrap();
    let s2 = log.signable();
    assert_ne!(s1, s2);
}

#[test]
fn empty_signable() {
    let log = MerkleLog::<MemStore, 16>::new(MemStore::new());
    let s = log.signable();
    assert_eq!(s, hash::hash_root(&[], 0));
}

#[test]
fn binary_tree() {
    let mut log = MerkleLog::<MemStore, 2>::new(MemStore::new());
    for i in 0..8u8 {
        log.append(&[i]).unwrap();
    }
    assert_eq!(log.length(), 8);
    assert_eq!(log.roots().len(), 1);
}

#[test]
#[should_panic(expected = "branching factor must be at least 2")]
fn new_panics_on_b_one() {
    MerkleLog::<MemStore, 1>::new(MemStore::new());
}

#[test]
#[should_panic(expected = "branching factor must be a power of 2")]
fn new_panics_on_non_power_of_two() {
    MerkleLog::<MemStore, 3>::new(MemStore::new());
}

// ── save / load ──

#[test]
fn save_load_roundtrip() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    for i in 0..10u8 {
        log.append(&[i]).unwrap();
    }

    let saved = log.save();
    let store = log.store().clone();
    let restored = MerkleLog::<MemStore, 4>::load(store, &saved).unwrap();

    assert_eq!(restored.length(), log.length());
    assert_eq!(restored.roots(), log.roots());
    assert_eq!(restored.signable(), log.signable());
}

#[test]
fn append_after_load() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    for i in 0..6u8 {
        log.append(&[i]).unwrap();
    }
    let saved = log.save();
    let store = log.store().clone();

    let mut restored = MerkleLog::<MemStore, 4>::load(store, &saved).unwrap();
    for i in 6..10u8 {
        restored.append(&[i]).unwrap();
    }

    let mut fresh = MerkleLog::<MemStore, 4>::new(MemStore::new());
    for i in 0..10u8 {
        fresh.append(&[i]).unwrap();
    }

    assert_eq!(restored.signable(), fresh.signable());
}

#[test]
fn load_rejects_bad_input() {
    assert!(MerkleLog::<MemStore, 4>::load(MemStore::new(), &[]).is_none());
    assert!(MerkleLog::<MemStore, 4>::load(MemStore::new(), &[0; 8]).is_none());
    let mut bad = vec![0u8; 12];
    bad[8] = 1;
    assert!(MerkleLog::<MemStore, 4>::load(MemStore::new(), &bad).is_none());
}

// ── prove / verify ──

#[test]
fn prove_single_entry() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    let h = log.append(b"only").unwrap();
    let p = log.prove(0).unwrap();
    assert!(p.levels.is_empty());
    assert!(proof::verify::<4>(&p, h, log.roots()[0]));
}

#[test]
fn prove_and_verify_complete_tree() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    let mut hashes = Vec::new();
    for i in 0..4u8 {
        hashes.push(log.append(&[i]).unwrap());
    }
    let root = log.roots()[0];

    for (i, &h) in hashes.iter().enumerate() {
        let p = log.prove(i as u64).unwrap();
        assert!(proof::verify::<4>(&p, h, root), "proof failed for index {i}");
    }
}

#[test]
fn prove_and_verify_two_level_tree() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    let mut hashes = Vec::new();
    for i in 0..16u8 {
        hashes.push(log.append(&[i]).unwrap());
    }
    let root = log.roots()[0];

    for (i, &h) in hashes.iter().enumerate() {
        let p = log.prove(i as u64).unwrap();
        assert!(proof::verify::<4>(&p, h, root), "proof failed for index {i}");
    }
}

#[test]
fn prove_incomplete_tree() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    let mut hashes = Vec::new();
    for i in 0..6u8 {
        hashes.push(log.append(&[i]).unwrap());
    }

    let sizes = subtree_sizes::<4>(6);
    assert_eq!(sizes, vec![4, 1, 1]);
    assert_eq!(log.roots().len(), 3);

    for i in 0..4 {
        let p = log.prove(i).unwrap();
        assert!(proof::verify::<4>(&p, hashes[i as usize], log.roots()[0]));
    }

    let p4 = log.prove(4).unwrap();
    assert!(proof::verify::<4>(&p4, hashes[4], log.roots()[1]));

    let p5 = log.prove(5).unwrap();
    assert!(proof::verify::<4>(&p5, hashes[5], log.roots()[2]));
}

#[test]
fn invalid_proof_rejected() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    for i in 0..4u8 {
        log.append(&[i]).unwrap();
    }
    let root = log.roots()[0];
    let h = hash::hash_leaf(&[0]);
    let p = log.prove(0).unwrap();

    let mut bad_proof = p.clone();
    bad_proof.levels[0].siblings[0] = [0xFF; 32];
    assert!(!proof::verify::<4>(&bad_proof, h, root));

    assert!(!proof::verify::<4>(&p, [0xAA; 32], root));
}

#[test]
#[should_panic(expected = "out of range")]
fn prove_out_of_range_panics() {
    let mut log = MerkleLog::<MemStore, 4>::new(MemStore::new());
    log.append(b"one").unwrap();
    let _ = log.prove(1);
}
