// created: 2026-03-05
// updated: 2026-03-05
// model: claude-opus-4-6
// driver: Isaac Clayton
// provenance: ai

//! Compact RLE-encoded event sets.
//!
//! An [`EventSet`] tracks which events (by index) a peer has seen,
//! using a hybrid run-length / literal encoding for efficient storage
//! across sparse, dense, and random data patterns.
//!
//! # Encoding
//!
//! Each 64-bit block uses a 2-bit tag in the MSBs:
//!
//! | Tag  | Meaning          | Payload           |
//! |------|------------------|-------------------|
//! | `00` | Run of N zeros   | N (62-bit count)  |
//! | `10` | Run of N ones    | N (62-bit count)  |
//! | `D1` | Literal          | 63 raw bits       |
//!
//! A literal block packs 63 bits into 64: bit 62 is the literal tag,
//! bit 63 is the MSB of the 63-bit value, and bits 61..0 hold the
//! lower 62 bits. Runs compress homogeneous regions; literals compress
//! mixed/random regions.
//!
//! # Examples
//!
//! ```
//! use neoset::{EventSet, EventSetBuilder};
//!
//! // Dense: "I have all events 0..999"
//! let dense = EventSet::ones(1000);
//! assert_eq!(dense.count_ones(), 1000);
//!
//! // Sparse: "I have events 5, 100, 5000 out of 10000"
//! let sparse = EventSet::from_ones(&[5, 100, 5000], 10_000);
//! assert_eq!(sparse.count_ones(), 3);
//!
//! // Sync diff: what does peer A have that peer B doesn't?
//! let peer_a = EventSet::ones(1000);
//! let mut b = EventSetBuilder::new();
//! b.push_ones(800);
//! b.push_zeros(50);
//! b.push_ones(150);
//! let peer_b = b.finish();
//! let need: Vec<u64> = peer_a.difference(&peer_b).iter_ones().collect();
//! assert_eq!(need.len(), 50); // events 800..850
//! ```

// ── Block encoding ──

const TAG_SHIFT: u32 = 62;
const TAG_ONES: u64 = 0b10;
const RUN_MAX: u64 = (1u64 << TAG_SHIFT) - 1;
const LIT_BITS: u32 = 63;
const LIT_MASK: u64 = (1u64 << LIT_BITS) - 1;

fn is_literal(block: u64) -> bool {
    (block >> TAG_SHIFT) & 1 == 1
}

fn is_run(block: u64) -> bool {
    !is_literal(block)
}

fn run_is_ones(block: u64) -> bool {
    (block >> TAG_SHIFT) == TAG_ONES
}

fn run_count(block: u64) -> u64 {
    block & RUN_MAX
}

fn block_width(block: u64) -> u64 {
    if is_literal(block) { LIT_BITS as u64 } else { run_count(block) }
}

fn make_zeros(n: u64) -> u64 {
    debug_assert!(n > 0 && n <= RUN_MAX);
    n
}

fn make_ones(n: u64) -> u64 {
    debug_assert!(n > 0 && n <= RUN_MAX);
    (TAG_ONES << TAG_SHIFT) | n
}

/// Encode a 63-bit value as a literal block.
fn make_literal(val: u64) -> u64 {
    debug_assert!(val <= LIT_MASK);
    let d = (val >> 62) & 1;
    let lower = val & RUN_MAX;
    (d << 63) | (1u64 << TAG_SHIFT) | lower
}

/// Decode a literal block into its 63-bit value.
fn literal_value(block: u64) -> u64 {
    let d = (block >> 63) & 1;
    let lower = block & RUN_MAX;
    (d << 62) | lower
}

// ── EventSet ──

/// A compact, RLE-encoded bitset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventSet {
    blocks: Vec<u64>,
    len: u64,
}

impl EventSet {
    /// Empty event set.
    pub fn new() -> Self {
        Self { blocks: Vec::new(), len: 0 }
    }

    /// Set of `n` consecutive ones.
    pub fn ones(n: u64) -> Self {
        if n == 0 { return Self::new(); }
        let mut b = EventSetBuilder::new();
        b.push_ones(n);
        b.finish()
    }

    /// Set of `n` consecutive zeros.
    pub fn zeros(n: u64) -> Self {
        if n == 0 { return Self::new(); }
        let mut b = EventSetBuilder::new();
        b.push_zeros(n);
        b.finish()
    }

    /// Build from sorted indices of set bits within a universe of `len` bits.
    ///
    /// # Panics
    ///
    /// Panics if `indices` is not sorted or contains values >= `len`.
    pub fn from_ones(indices: &[u64], len: u64) -> Self {
        let mut b = EventSetBuilder::new();
        let mut pos = 0u64;
        for &idx in indices {
            assert!(idx < len, "index {idx} out of range (len {len})");
            assert!(idx >= pos, "indices must be sorted");
            if idx > pos {
                b.push_zeros(idx - pos);
            }
            b.push(true);
            pos = idx + 1;
        }
        if pos < len {
            b.push_zeros(len - pos);
        }
        b.finish()
    }

    /// Build from an iterator of bools.
    pub fn from_bools(iter: impl IntoIterator<Item = bool>) -> Self {
        let mut b = EventSetBuilder::new();
        for bit in iter {
            b.push(bit);
        }
        b.finish()
    }

    /// Total number of bit positions.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether the set represents zero bits.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Check if the bit at `index` is set.
    pub fn contains(&self, index: u64) -> bool {
        if index >= self.len {
            return false;
        }
        let mut pos: u64 = 0;
        for &block in &self.blocks {
            let w = block_width(block);
            if index < pos + w {
                if is_run(block) {
                    return run_is_ones(block);
                } else {
                    let offset = (index - pos) as u32;
                    return (literal_value(block) >> offset) & 1 != 0;
                }
            }
            pos += w;
        }
        false
    }

    /// Count set bits.
    pub fn count_ones(&self) -> u64 {
        let mut total = 0u64;
        let mut remaining = self.len;
        for &block in &self.blocks {
            if remaining == 0 { break; }
            if is_run(block) {
                let c = run_count(block).min(remaining);
                if run_is_ones(block) {
                    total += c;
                }
                remaining -= c;
            } else {
                let valid = remaining.min(LIT_BITS as u64);
                let val = literal_value(block);
                // Count only the valid bits.
                let mask = if valid >= 64 { u64::MAX } else { (1u64 << valid) - 1 };
                total += (val & mask).count_ones() as u64;
                remaining -= valid;
            }
        }
        total
    }

    /// Count unset bits.
    pub fn count_zeros(&self) -> u64 {
        self.len - self.count_ones()
    }

    /// Iterate over indices of set bits.
    pub fn iter_ones(&self) -> Iter<'_> {
        Iter { blocks: &self.blocks, block_idx: 0, pos: 0, offset: 0, len: self.len, want: true }
    }

    /// Iterate over indices of unset bits.
    pub fn iter_zeros(&self) -> Iter<'_> {
        Iter { blocks: &self.blocks, block_idx: 0, pos: 0, offset: 0, len: self.len, want: false }
    }

    /// Flip all bits.
    pub fn complement(&self) -> EventSet {
        let mut blocks = Vec::with_capacity(self.blocks.len());
        for &block in &self.blocks {
            if is_literal(block) {
                let flipped = (!literal_value(block)) & LIT_MASK;
                blocks.push(make_literal(flipped));
            } else if run_is_ones(block) {
                blocks.push(make_zeros(run_count(block)));
            } else {
                blocks.push(make_ones(run_count(block)));
            }
        }
        EventSet { blocks, len: self.len }
    }

    /// Set union: bits set in either operand.
    pub fn union(&self, other: &EventSet) -> EventSet {
        bitwise_op(self, other, |a, b| a | b)
    }

    /// Set intersection: bits set in both operands.
    pub fn intersection(&self, other: &EventSet) -> EventSet {
        bitwise_op(self, other, |a, b| a & b)
    }

    /// Set difference: bits set in `self` but not `other`.
    pub fn difference(&self, other: &EventSet) -> EventSet {
        bitwise_op(self, other, |a, b| a & !b)
    }

    /// Symmetric difference: bits set in exactly one operand.
    pub fn symmetric_difference(&self, other: &EventSet) -> EventSet {
        bitwise_op(self, other, |a, b| a ^ b)
    }

    /// Whether every bit set in `self` is also set in `other`.
    pub fn is_subset(&self, other: &EventSet) -> bool {
        self.difference(other).count_ones() == 0
    }

    /// Whether every bit set in `other` is also set in `self`.
    pub fn is_superset(&self, other: &EventSet) -> bool {
        other.is_subset(self)
    }

    /// Number of encoded blocks (for compression analysis).
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Encoded size in bytes (8 per block + 8 for length header).
    pub fn encoded_size(&self) -> usize {
        8 + self.blocks.len() * 8
    }

    /// Serialize to bytes. Format: `len_le_u64 || block0_le_u64 || ...`
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.encoded_size());
        out.extend_from_slice(&self.len.to_le_bytes());
        for &block in &self.blocks {
            out.extend_from_slice(&block.to_le_bytes());
        }
        out
    }

    /// Deserialize from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 || (bytes.len() - 8) % 8 != 0 {
            return None;
        }
        let len = u64::from_le_bytes(bytes[..8].try_into().ok()?);
        let blocks: Vec<u64> = bytes[8..]
            .chunks_exact(8)
            .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
            .collect();

        // Validate: sum of block widths must cover len exactly,
        // or exceed it by < 63 (trailing literal padding).
        let mut total: u64 = 0;
        for &block in &blocks {
            if is_run(block) && run_count(block) == 0 {
                return None;
            }
            total = total.checked_add(block_width(block))?;
        }
        if total < len || total - len >= LIT_BITS as u64 {
            return None;
        }
        Some(Self { blocks, len })
    }
}

impl Default for EventSet {
    fn default() -> Self { Self::new() }
}

// ── Builder ──

/// Incremental builder for [`EventSet`].
///
/// Accumulates bits into a 63-bit buffer. Full buffers are emitted as
/// literal blocks (mixed) or run blocks (uniform). Large homogeneous
/// pushes bypass the buffer for O(1) emission.
pub struct EventSetBuilder {
    blocks: Vec<u64>,
    len: u64,
    buf: u64,
    buf_len: u32,
}

impl EventSetBuilder {
    pub fn new() -> Self {
        Self { blocks: Vec::new(), len: 0, buf: 0, buf_len: 0 }
    }

    /// Push a single bit.
    pub fn push(&mut self, value: bool) {
        if value {
            self.buf |= 1u64 << self.buf_len;
        }
        self.buf_len += 1;
        self.len += 1;
        if self.buf_len == LIT_BITS {
            self.flush_buf();
        }
    }

    /// Push `count` consecutive ones.
    pub fn push_ones(&mut self, mut count: u64) {
        if count == 0 { return; }
        self.len += count;

        // 1. Fill current buffer.
        let space = (LIT_BITS - self.buf_len) as u64;
        let fill = count.min(space);
        if fill > 0 {
            let mask = ((1u64 << fill) - 1) << self.buf_len;
            self.buf |= mask;
            self.buf_len += fill as u32;
            count -= fill;
        }
        if self.buf_len == LIT_BITS {
            self.flush_buf();
        }
        if count == 0 { return; }

        // 2. Buffer is empty. Emit runs for large counts.
        debug_assert!(self.buf_len == 0);
        if count >= LIT_BITS as u64 {
            while count > RUN_MAX {
                self.blocks.push(make_ones(RUN_MAX));
                count -= RUN_MAX;
            }
            if count >= LIT_BITS as u64 {
                self.blocks.push(make_ones(count));
                return;
            }
        }

        // 3. Buffer remainder (< 63).
        if count > 0 {
            self.buf = (1u64 << count) - 1;
            self.buf_len = count as u32;
        }
    }

    /// Push `count` consecutive zeros.
    pub fn push_zeros(&mut self, mut count: u64) {
        if count == 0 { return; }
        self.len += count;

        // 1. Fill current buffer (bits default to 0).
        let space = (LIT_BITS - self.buf_len) as u64;
        let fill = count.min(space);
        self.buf_len += fill as u32;
        count -= fill;
        if self.buf_len == LIT_BITS {
            self.flush_buf();
        }
        if count == 0 { return; }

        // 2. Buffer is empty. Emit runs for large counts.
        debug_assert!(self.buf_len == 0);
        if count >= LIT_BITS as u64 {
            while count > RUN_MAX {
                self.blocks.push(make_zeros(RUN_MAX));
                count -= RUN_MAX;
            }
            if count >= LIT_BITS as u64 {
                self.blocks.push(make_zeros(count));
                return;
            }
        }

        // 3. Buffer remainder.
        if count > 0 {
            self.buf_len = count as u32;
            // buf already 0
        }
    }

    /// Finish building.
    pub fn finish(mut self) -> EventSet {
        if self.buf_len > 0 {
            let mask = (1u64 << self.buf_len) - 1;
            let val = self.buf & mask;
            if val == 0 {
                self.blocks.push(make_zeros(self.buf_len as u64));
            } else if val == mask {
                self.blocks.push(make_ones(self.buf_len as u64));
            } else {
                // Mixed — emit as literal (upper bits are zero padding).
                self.blocks.push(make_literal(val));
            }
            self.buf = 0;
            self.buf_len = 0;
        }
        compact_runs(&mut self.blocks);
        EventSet { blocks: self.blocks, len: self.len }
    }

    fn flush_buf(&mut self) {
        debug_assert!(self.buf_len == LIT_BITS);
        let val = self.buf & LIT_MASK;
        if val == 0 {
            self.blocks.push(make_zeros(LIT_BITS as u64));
        } else if val == LIT_MASK {
            self.blocks.push(make_ones(LIT_BITS as u64));
        } else {
            self.blocks.push(make_literal(val));
        }
        self.buf = 0;
        self.buf_len = 0;
    }
}

impl Default for EventSetBuilder {
    fn default() -> Self { Self::new() }
}

/// Merge adjacent same-type runs.
fn compact_runs(blocks: &mut Vec<u64>) {
    if blocks.len() < 2 { return; }
    let mut write = 0;
    for read in 1..blocks.len() {
        let a = blocks[write];
        let b = blocks[read];
        if is_run(a) && is_run(b) && run_is_ones(a) == run_is_ones(b) {
            let sum = run_count(a) + run_count(b);
            if sum <= RUN_MAX {
                blocks[write] = if run_is_ones(a) { make_ones(sum) } else { make_zeros(sum) };
                continue;
            }
        }
        write += 1;
        blocks[write] = blocks[read];
    }
    blocks.truncate(write + 1);
}

// ── Iterator ──

/// Iterator over bit indices matching a target value.
pub struct Iter<'a> {
    blocks: &'a [u64],
    block_idx: usize,
    pos: u64,
    offset: u64,
    len: u64,
    want: bool,
}

impl Iterator for Iter<'_> {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        while self.block_idx < self.blocks.len() {
            let block = self.blocks[self.block_idx];

            if is_run(block) {
                let count = run_count(block);
                if run_is_ones(block) == self.want {
                    if self.offset < count {
                        let idx = self.pos + self.offset;
                        if idx >= self.len { return None; }
                        self.offset += 1;
                        return Some(idx);
                    }
                }
                self.pos += count;
            } else {
                let val = literal_value(block);
                while self.offset < LIT_BITS as u64 {
                    let idx = self.pos + self.offset;
                    if idx >= self.len { return None; }
                    let bit = (val >> self.offset) & 1 != 0;
                    self.offset += 1;
                    if bit == self.want {
                        return Some(idx);
                    }
                }
                self.pos += LIT_BITS as u64;
            }

            self.block_idx += 1;
            self.offset = 0;
        }
        None
    }
}

// ── Set operations ──

fn expand(set: &EventSet) -> Vec<bool> {
    let mut bits = Vec::with_capacity(set.len as usize);
    for &block in &set.blocks {
        if is_run(block) {
            let val = run_is_ones(block);
            for _ in 0..run_count(block) {
                bits.push(val);
            }
        } else {
            let val = literal_value(block);
            for i in 0..LIT_BITS {
                bits.push((val >> i) & 1 != 0);
            }
        }
    }
    bits.truncate(set.len as usize);
    bits
}

fn bitwise_op(a: &EventSet, b: &EventSet, op: impl Fn(bool, bool) -> bool) -> EventSet {
    let bits_a = expand(a);
    let bits_b = expand(b);
    let max_len = bits_a.len().max(bits_b.len());
    let mut builder = EventSetBuilder::new();
    for i in 0..max_len {
        let va = bits_a.get(i).copied().unwrap_or(false);
        let vb = bits_b.get(i).copied().unwrap_or(false);
        builder.push(op(va, vb));
    }
    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Constructors ──

    #[test]
    fn empty_set() {
        let set = EventSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert_eq!(set.count_ones(), 0);
        assert_eq!(set.count_zeros(), 0);
        assert!(!set.contains(0));
    }

    #[test]
    fn ones_constructor() {
        let set = EventSet::ones(1000);
        assert_eq!(set.len(), 1000);
        assert_eq!(set.count_ones(), 1000);
        assert!(set.contains(0));
        assert!(set.contains(999));
        assert!(!set.contains(1000));
    }

    #[test]
    fn zeros_constructor() {
        let set = EventSet::zeros(500);
        assert_eq!(set.len(), 500);
        assert_eq!(set.count_zeros(), 500);
        assert!(!set.contains(0));
    }

    #[test]
    fn from_ones_sparse() {
        let set = EventSet::from_ones(&[5, 100, 5000], 10_000);
        assert_eq!(set.len(), 10_000);
        assert_eq!(set.count_ones(), 3);
        assert!(set.contains(5));
        assert!(set.contains(100));
        assert!(set.contains(5000));
        assert!(!set.contains(0));
        assert!(!set.contains(99));
    }

    #[test]
    fn from_ones_dense() {
        let indices: Vec<u64> = (0..100).collect();
        let set = EventSet::from_ones(&indices, 100);
        assert_eq!(set.count_ones(), 100);
    }

    #[test]
    fn from_ones_empty() {
        let set = EventSet::from_ones(&[], 100);
        assert_eq!(set.count_ones(), 0);
        assert_eq!(set.len(), 100);
    }

    #[test]
    fn from_bools() {
        let bits = vec![true, false, true, true, false];
        let set = EventSet::from_bools(bits);
        assert_eq!(set.len(), 5);
        assert!(set.contains(0));
        assert!(!set.contains(1));
        assert!(set.contains(2));
    }

    // ── Builder ──

    #[test]
    fn push_individual_bits() {
        let mut b = EventSetBuilder::new();
        b.push(true);
        b.push(false);
        b.push(true);
        b.push(true);
        b.push(false);
        let set = b.finish();
        assert_eq!(set.len(), 5);
        assert!(set.contains(0));
        assert!(!set.contains(1));
        assert!(set.contains(2));
        assert!(set.contains(3));
        assert!(!set.contains(4));
        assert_eq!(set.count_ones(), 3);
    }

    #[test]
    fn push_mixed_then_run() {
        let mut b = EventSetBuilder::new();
        b.push(true);
        b.push(false);
        b.push(true);
        b.push_ones(1000);
        let set = b.finish();
        assert_eq!(set.len(), 1003);
        assert_eq!(set.count_ones(), 1002);
        assert!(set.contains(0));
        assert!(!set.contains(1));
        assert!(set.contains(2));
        assert!(set.contains(3));
        assert!(set.contains(1002));
    }

    #[test]
    fn push_large_zeros_then_ones() {
        let mut b = EventSetBuilder::new();
        b.push_zeros(10_000);
        b.push_ones(10_000);
        let set = b.finish();
        assert_eq!(set.len(), 20_000);
        assert_eq!(set.count_ones(), 10_000);
        assert!(!set.contains(9_999));
        assert!(set.contains(10_000));
    }

    // ── Literal blocks ──

    #[test]
    fn alternating_bits_use_literals() {
        // 126 alternating bits should use 2 literal blocks, not 126 run blocks.
        let bits: Vec<bool> = (0..126).map(|i| i % 2 == 0).collect();
        let set = EventSet::from_bools(bits);
        assert_eq!(set.len(), 126);
        assert_eq!(set.count_ones(), 63);
        // 126 bits = 2 full 63-bit literal blocks.
        assert_eq!(set.block_count(), 2);
    }

    #[test]
    fn random_pattern_uses_literals() {
        // A pseudo-random pattern should use literals, not one-bit runs.
        let bits: Vec<bool> = (0..630).map(|i| (i * 7 + 3) % 5 < 2).collect();
        let set = EventSet::from_bools(bits);
        assert_eq!(set.len(), 630);
        // 630 / 63 = 10 blocks (all literals, since the pattern is mixed).
        assert!(set.block_count() <= 10);
    }

    #[test]
    fn literal_contains_works() {
        // Build a set where the first 63 bits are mixed (literal block).
        let bits: Vec<bool> = (0..63).map(|i| i % 3 == 0).collect();
        let set = EventSet::from_bools(bits.clone());
        for (i, &expected) in bits.iter().enumerate() {
            assert_eq!(set.contains(i as u64), expected, "mismatch at index {i}");
        }
    }

    #[test]
    fn partial_literal_at_end() {
        // 30 mixed bits — should emit a partial literal (padded to 63).
        let bits: Vec<bool> = (0..30).map(|i| i % 2 == 0).collect();
        let set = EventSet::from_bools(bits.clone());
        assert_eq!(set.len(), 30);
        for (i, &expected) in bits.iter().enumerate() {
            assert_eq!(set.contains(i as u64), expected, "mismatch at index {i}");
        }
        // Should not read padding bits.
        assert!(!set.contains(30));
    }

    // ── Compression ──

    #[test]
    fn dense_run_compression() {
        let set = EventSet::ones(10_000);
        assert_eq!(set.block_count(), 1);
        assert_eq!(set.encoded_size(), 16); // 8 (len) + 8 (1 block)
    }

    #[test]
    fn sparse_compression() {
        // 1 set bit out of 1_000_000 — should be ~3 blocks (zeros, ones(1), zeros).
        let set = EventSet::from_ones(&[500_000], 1_000_000);
        assert!(set.block_count() <= 3);
    }

    #[test]
    fn mixed_compression_ratio() {
        // 6300 random bits → ~100 literal blocks → ~808 bytes.
        // Without literals (runs only), alternating would be ~6300 blocks → ~50408 bytes.
        let bits: Vec<bool> = (0..6300).map(|i| (i * 13 + 7) % 11 < 5).collect();
        let set = EventSet::from_bools(bits);
        // Should be around 100 blocks, not thousands.
        assert!(set.block_count() <= 110, "got {} blocks", set.block_count());
    }

    // ── Serialization ──

    #[test]
    fn serialization_roundtrip_runs() {
        let mut b = EventSetBuilder::new();
        b.push_ones(100);
        b.push_zeros(200);
        b.push_ones(50);
        let set = b.finish();
        let bytes = set.to_bytes();
        let restored = EventSet::from_bytes(&bytes).unwrap();
        assert_eq!(set, restored);
    }

    #[test]
    fn serialization_roundtrip_literals() {
        let bits: Vec<bool> = (0..200).map(|i| i % 3 == 0).collect();
        let set = EventSet::from_bools(bits.clone());
        let bytes = set.to_bytes();
        let restored = EventSet::from_bytes(&bytes).unwrap();
        assert_eq!(set, restored);
        for (i, &expected) in bits.iter().enumerate() {
            assert_eq!(restored.contains(i as u64), expected);
        }
    }

    #[test]
    fn serialization_roundtrip_mixed() {
        let mut b = EventSetBuilder::new();
        b.push_ones(100);
        b.push(false);
        b.push(true);
        b.push(false);
        b.push_zeros(500);
        b.push_ones(50);
        let set = b.finish();
        let bytes = set.to_bytes();
        let restored = EventSet::from_bytes(&bytes).unwrap();
        assert_eq!(set, restored);
    }

    #[test]
    fn from_bytes_rejects_bad_input() {
        assert!(EventSet::from_bytes(&[]).is_none());
        assert!(EventSet::from_bytes(&[1, 2, 3]).is_none());
        let mut bad = 100u64.to_le_bytes().to_vec();
        assert!(EventSet::from_bytes(&bad).is_none());
        bad.push(0);
        assert!(EventSet::from_bytes(&bad).is_none());
    }

    // ── Iterators ──

    #[test]
    fn ones_iterator() {
        let mut b = EventSetBuilder::new();
        b.push(false);
        b.push(true);
        b.push(false);
        b.push(false);
        b.push(true);
        let set = b.finish();
        let ones: Vec<u64> = set.iter_ones().collect();
        assert_eq!(ones, vec![1, 4]);
    }

    #[test]
    fn zeros_iterator() {
        let mut b = EventSetBuilder::new();
        b.push(true);
        b.push(false);
        b.push(true);
        let set = b.finish();
        let zeros: Vec<u64> = set.iter_zeros().collect();
        assert_eq!(zeros, vec![1]);
    }

    #[test]
    fn ones_iterator_with_runs() {
        let mut b = EventSetBuilder::new();
        b.push_zeros(5);
        b.push_ones(3);
        b.push_zeros(2);
        let set = b.finish();
        let ones: Vec<u64> = set.iter_ones().collect();
        assert_eq!(ones, vec![5, 6, 7]);
    }

    #[test]
    fn ones_iterator_with_literals() {
        let bits: Vec<bool> = (0..63).map(|i| i == 10 || i == 30 || i == 62).collect();
        let set = EventSet::from_bools(bits);
        let ones: Vec<u64> = set.iter_ones().collect();
        assert_eq!(ones, vec![10, 30, 62]);
    }

    #[test]
    fn iterator_respects_len_boundary() {
        // Partial literal at end — iterator must not yield padding bits.
        let bits: Vec<bool> = (0..20).map(|i| i % 2 == 0).collect();
        let set = EventSet::from_bools(bits);
        let ones: Vec<u64> = set.iter_ones().collect();
        assert_eq!(ones.len(), 10);
        assert_eq!(*ones.last().unwrap(), 18);
    }

    // ── Set operations ──

    #[test]
    fn union_operation() {
        let a = EventSet::from_bools(vec![true, false, true, false]);
        let b = EventSet::from_bools(vec![false, true, true, false]);
        let u = a.union(&b);
        assert_eq!(u.len(), 4);
        assert!(u.contains(0));
        assert!(u.contains(1));
        assert!(u.contains(2));
        assert!(!u.contains(3));
    }

    #[test]
    fn intersection_operation() {
        let a = EventSet::from_bools(vec![true, false, true]);
        let b = EventSet::from_bools(vec![true, true, false]);
        let i = a.intersection(&b);
        assert!(i.contains(0));
        assert!(!i.contains(1));
        assert!(!i.contains(2));
    }

    #[test]
    fn difference_operation() {
        let a = EventSet::from_bools(vec![true, true, false]);
        let b = EventSet::from_bools(vec![true, false, true]);
        let d = a.difference(&b);
        assert!(!d.contains(0));
        assert!(d.contains(1));
        assert!(!d.contains(2));
    }

    #[test]
    fn symmetric_difference_operation() {
        let a = EventSet::from_bools(vec![true, true, false, false]);
        let b = EventSet::from_bools(vec![true, false, true, false]);
        let s = a.symmetric_difference(&b);
        assert!(!s.contains(0)); // both
        assert!(s.contains(1));  // only a
        assert!(s.contains(2));  // only b
        assert!(!s.contains(3)); // neither
    }

    #[test]
    fn complement_operation() {
        let set = EventSet::ones(100);
        let comp = set.complement();
        assert_eq!(comp.len(), 100);
        assert_eq!(comp.count_ones(), 0);
        assert_eq!(comp.count_zeros(), 100);
    }

    #[test]
    fn complement_mixed() {
        let bits = vec![true, false, true, false, true];
        let set = EventSet::from_bools(bits);
        let comp = set.complement();
        assert!(!comp.contains(0));
        assert!(comp.contains(1));
        assert!(!comp.contains(2));
        assert!(comp.contains(3));
        assert!(!comp.contains(4));
    }

    #[test]
    fn complement_involution() {
        let bits: Vec<bool> = (0..200).map(|i| i % 3 == 0).collect();
        let set = EventSet::from_bools(bits);
        let double = set.complement().complement();
        // Bit-level equivalence.
        for i in 0..set.len() {
            assert_eq!(set.contains(i), double.contains(i), "mismatch at {i}");
        }
    }

    #[test]
    fn different_length_union() {
        let a = EventSet::ones(3);
        let mut bb = EventSetBuilder::new();
        bb.push_zeros(2);
        bb.push_ones(5);
        let b = bb.finish();
        let u = a.union(&b);
        assert_eq!(u.len(), 7);
        assert!(u.contains(0));
        assert!(u.contains(6));
    }

    // ── Subset / superset ──

    #[test]
    fn subset_superset() {
        let small = EventSet::from_ones(&[1, 3, 5], 10);
        let big = EventSet::from_ones(&[0, 1, 2, 3, 4, 5, 6], 10);
        assert!(small.is_subset(&big));
        assert!(big.is_superset(&small));
        assert!(!big.is_subset(&small));
    }

    #[test]
    fn empty_is_subset_of_everything() {
        let empty = EventSet::new();
        let some = EventSet::ones(10);
        assert!(empty.is_subset(&some));
        assert!(empty.is_subset(&empty));
    }

    // ── Sync scenario ──

    #[test]
    fn sync_diff_scenario() {
        // Peer A has events 0..999.
        let peer_a = EventSet::ones(1000);
        // Peer B has 0..799 and 850..999.
        let mut b = EventSetBuilder::new();
        b.push_ones(800);
        b.push_zeros(50);
        b.push_ones(150);
        let peer_b = b.finish();
        // What does A need to send B?
        let need = peer_a.difference(&peer_b);
        let indices: Vec<u64> = need.iter_ones().collect();
        assert_eq!(indices.len(), 50);
        assert_eq!(indices[0], 800);
        assert_eq!(indices[49], 849);
    }

    // ── Edge cases ──

    #[test]
    fn single_bit_one() {
        let set = EventSet::ones(1);
        assert_eq!(set.len(), 1);
        assert!(set.contains(0));
    }

    #[test]
    fn single_bit_zero() {
        let set = EventSet::zeros(1);
        assert_eq!(set.len(), 1);
        assert!(!set.contains(0));
    }

    #[test]
    fn exactly_63_bits() {
        let bits: Vec<bool> = (0..63).map(|i| i % 2 == 0).collect();
        let set = EventSet::from_bools(bits.clone());
        assert_eq!(set.len(), 63);
        for (i, &expected) in bits.iter().enumerate() {
            assert_eq!(set.contains(i as u64), expected, "at {i}");
        }
    }

    #[test]
    fn exactly_64_bits() {
        let bits: Vec<bool> = (0..64).map(|i| i % 2 == 0).collect();
        let set = EventSet::from_bools(bits.clone());
        assert_eq!(set.len(), 64);
        for (i, &expected) in bits.iter().enumerate() {
            assert_eq!(set.contains(i as u64), expected, "at {i}");
        }
    }

    #[test]
    fn count_ones_respects_len_boundary() {
        // A partial literal at the end — padding bits must not be counted.
        let mut b = EventSetBuilder::new();
        b.push_ones(10);
        b.push(false);
        b.push_ones(5);
        let set = b.finish();
        assert_eq!(set.len(), 16);
        assert_eq!(set.count_ones(), 15);
    }
}
