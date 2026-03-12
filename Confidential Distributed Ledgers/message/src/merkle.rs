//! Minimal binary Merkle tree used to commit to a set of `partial_dec_share`
//! values off-chain while storing only the 32-byte root on-chain.
//!
//! # Usage
//!
//! ```rust
//! use message::merkle::MerkleTree;
//!
//! let shares: Vec<Vec<u8>> = vec![b"share0".to_vec(), b"share1".to_vec()];
//! let tree = MerkleTree::build(&shares);
//! let root = tree.root();
//! let proof = tree.proof(0);
//! assert!(proof.verify(&root, &shares[0]));
//! ```
//!
//! # Hashing convention
//! - Leaf hash  : `SHA-256(0x00 || leaf_bytes)`
//! - Inner hash : `SHA-256(0x01 || left_hash || right_hash)`
//!
//! Domain-separation prefixes prevent second-pre-image attacks on the tree.
//!
//! When the number of leaves at a level is odd the last node is duplicated to
//! form an even pair (standard Bitcoin/Merkle convention).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ─── hash helpers ────────────────────────────────────────────────────────────

/// SHA-256 of `0x00 || data`.  Applied to each raw leaf value.
fn leaf_hash(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([0x00]);
    h.update(data);
    h.finalize().into()
}

/// SHA-256 of `0x01 || left || right`.  Applied to pairs of child hashes.
fn inner_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([0x01]);
    h.update(left);
    h.update(right);
    h.finalize().into()
}

// ─── public types ─────────────────────────────────────────────────────────────

/// One step in a Merkle inclusion proof.
///
/// `sibling` is the hash of the node that neighbours the current node at this
/// level.  `is_left` is `true` when the sibling is the **left** child (i.e.
/// the current path node is on the right), so the verifier knows the correct
/// concatenation order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MerkleProofNode {
    pub sibling: [u8; 32],
    /// `true`  → sibling is the LEFT child  (current node is on the right)
    /// `false` → sibling is the RIGHT child (current node is on the left)
    pub is_left: bool,
}

/// Inclusion proof for a single leaf identified by `leaf_index`.
///
/// The `path` vector runs from the **leaf level** upward to the child of the
/// root — i.e. `path[0]` is the sibling at leaf level and `path[last]` is the
/// child of the root.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MerkleProof {
    /// Zero-based index of the leaf this proof covers.
    pub leaf_index: usize,
    /// Sibling hashes from leaf level up to (but not including) the root.
    pub path: Vec<MerkleProofNode>,
}

impl MerkleProof {
    /// Verify this proof against a known 32-byte Merkle root.
    ///
    /// `leaf_data` is the **raw bytes** of the leaf (not its hash).  The
    /// function recomputes `leaf_hash(leaf_data)` and walks `path` toward the
    /// root; returns `true` iff the recomputed root equals `expected_root`.
    pub fn verify(&self, expected_root: &[u8; 32], leaf_data: &[u8]) -> bool {
        let mut current = leaf_hash(leaf_data);
        for node in &self.path {
            current = if node.is_left {
                // sibling is left, current is right
                inner_hash(&node.sibling, &current)
            } else {
                // current is left, sibling is right
                inner_hash(&current, &node.sibling)
            };
        }
        &current == expected_root
    }
}

// ─── MerkleTree ───────────────────────────────────────────────────────────────

/// Complete binary Merkle tree.
///
/// Internally `levels[0]` stores the root (single hash) and
/// `levels[levels.len()-1]` stores the leaf hashes.
pub struct MerkleTree {
    /// `levels[0]` = root level (1 node).
    /// `levels[k]` = one level above the leaves.
    /// `levels[depth]` = leaf hashes.
    levels: Vec<Vec<[u8; 32]>>,
    /// Number of original leaves (before any padding).
    pub leaf_count: usize,
}

impl MerkleTree {
    /// Build a Merkle tree from an ordered slice of raw leaf values.
    ///
    /// Each element of `leaves` is hashed via `leaf_hash()` to form the
    /// leaf level; inner nodes are computed bottom-up.
    ///
    /// Panics if `leaves` is empty.
    pub fn build(leaves: &[impl AsRef<[u8]>]) -> Self {
        assert!(!leaves.is_empty(), "MerkleTree requires at least one leaf");

        let leaf_count = leaves.len();
        let leaf_hashes: Vec<[u8; 32]> = leaves.iter().map(|l| leaf_hash(l.as_ref())).collect();

        // Build bottom-up: collect each level starting from leaves.
        // After the loop `levels_bottom_up[0]` = leaf hashes,
        // `levels_bottom_up[last]` = [root_hash].
        let mut levels_bottom_up: Vec<Vec<[u8; 32]>> = Vec::new();
        let mut current = leaf_hashes;

        loop {
            levels_bottom_up.push(current.clone());
            if current.len() == 1 {
                break; // reached the root
            }
            // Hash pairs; duplicate last node when length is odd.
            let mut next: Vec<[u8; 32]> = Vec::with_capacity((current.len() + 1) / 2);
            let mut i = 0;
            while i < current.len() {
                let left = current[i];
                let right = if i + 1 < current.len() {
                    current[i + 1]
                } else {
                    current[i] // odd leaf → duplicate
                };
                next.push(inner_hash(&left, &right));
                i += 2;
            }
            current = next;
        }

        // Reverse so that levels[0] = root level, levels[last] = leaf level.
        levels_bottom_up.reverse();

        MerkleTree {
            levels: levels_bottom_up,
            leaf_count,
        }
    }

    /// Return the 32-byte root hash.
    pub fn root(&self) -> [u8; 32] {
        self.levels[0][0]
    }

    /// Generate an inclusion proof for the leaf at `leaf_index` (0-based).
    ///
    /// The proof starts at the leaf level and walks up to (but not including)
    /// the root level.
    ///
    /// Panics if `leaf_index >= leaf_count`.
    pub fn proof(&self, leaf_index: usize) -> MerkleProof {
        assert!(
            leaf_index < self.leaf_count,
            "leaf_index {} out of range (leaf_count={})",
            leaf_index,
            self.leaf_count
        );

        // `levels[0]` = root, `levels[depth]` = leaf hashes.
        let depth = self.levels.len() - 1; // number of levels below the root

        let mut path: Vec<MerkleProofNode> = Vec::with_capacity(depth);
        let mut idx = leaf_index;

        // Walk from the leaf level (levels[depth]) up to levels[1] (the level
        // just below the root).
        for level_idx in (1..=depth).rev() {
            let level = &self.levels[level_idx];
            let sibling_idx = if idx % 2 == 0 {
                idx + 1 // current is left child; sibling is right
            } else {
                idx - 1 // current is right child; sibling is left
            };

            // If sibling_idx is out of range (odd node at end of level that was
            // duplicated during build) use the current node itself as sibling.
            let sibling = if sibling_idx < level.len() {
                level[sibling_idx]
            } else {
                level[idx]
            };

            // `is_left` = true means the sibling is the left child (i.e. we
            // are the right child and the sibling precedes us).
            let is_left = idx % 2 == 1;

            path.push(MerkleProofNode { sibling, is_left });
            idx /= 2;
        }

        MerkleProof { leaf_index, path }
    }
}

// ─── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_leaf() {
        let data = b"only_leaf";
        let tree = MerkleTree::build(&[data]);
        assert_eq!(tree.root(), leaf_hash(data));
        let proof = tree.proof(0);
        assert!(proof.verify(&tree.root(), data));
    }

    #[test]
    fn two_leaves() {
        let leaves: &[&[u8]] = &[b"leaf0", b"leaf1"];
        let tree = MerkleTree::build(leaves);
        for i in 0..2 {
            let proof = tree.proof(i);
            assert!(proof.verify(&tree.root(), leaves[i]));
        }
    }

    #[test]
    fn four_leaves_all_proofs_valid() {
        let leaves: &[&[u8]] = &[b"a", b"b", b"c", b"d"];
        let tree = MerkleTree::build(leaves);
        for i in 0..4 {
            let proof = tree.proof(i);
            assert!(proof.verify(&tree.root(), leaves[i]), "proof {} failed", i);
        }
    }

    #[test]
    fn five_leaves_odd_padding() {
        let leaves: Vec<Vec<u8>> = (0..5u8).map(|i| vec![i]).collect();
        let tree = MerkleTree::build(&leaves);
        for i in 0..5 {
            let proof = tree.proof(i);
            assert!(
                proof.verify(&tree.root(), &leaves[i]),
                "proof {} failed",
                i
            );
        }
    }

    #[test]
    fn wrong_data_fails_verification() {
        let leaves: &[&[u8]] = &[b"correct", b"other"];
        let tree = MerkleTree::build(leaves);
        let proof = tree.proof(0);
        // Verify with wrong data — must fail
        assert!(!proof.verify(&tree.root(), b"wrong_data"));
    }

    #[test]
    fn ten_leaves_matches_partial_dec_share_use_case() {
        // Simulate n=10 partial_dec_shares stored as 33-byte arrays.
        let shares: Vec<[u8; 33]> = (0..10u8).map(|i| [i; 33]).collect();
        let shares_as_slices: Vec<&[u8]> = shares.iter().map(|s| s.as_ref()).collect();
        let tree = MerkleTree::build(&shares_as_slices);
        for i in 0..10 {
            let proof = tree.proof(i);
            assert!(proof.verify(&tree.root(), &shares[i]), "share {} proof failed", i);
        }
    }
}
