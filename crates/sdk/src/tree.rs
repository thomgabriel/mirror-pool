use pool_program::poseidon;

use crate::{FieldBytes, SdkError, TREE_DEPTH};

/// The Merkle authentication path for a deposited note (private circuit
/// inputs `pathElements`/`pathIndices`), depth matching `prover::TREE_DEPTH`
/// (= `programs/pool-program/src/merkle.rs::TREE_HEIGHT`).
#[derive(Debug, Clone)]
pub struct MerklePath {
    pub elements: [FieldBytes; TREE_DEPTH],
    pub indices: [u8; TREE_DEPTH],
}

/// Client-side incremental Merkle tree mirroring `pool_program::merkle`
/// (`TREE_HEIGHT` levels, Poseidon2 nodes, empty positions filled with the
/// same zero-subtree constants). Lets a client compute its note's
/// authentication path from the leaves it has scanned — the private
/// `MerklePath` inputs `prover::prove_membership` needs.
pub struct MerkleTree {
    leaves: Vec<[u8; 32]>,
    zeros: [[u8; 32]; TREE_DEPTH],
}

impl MerkleTree {
    pub fn new() -> Result<Self, SdkError> {
        let zeros = pool_program::merkle::zeros().map_err(|_| SdkError::MerkleHash)?;
        Ok(Self {
            leaves: Vec::new(),
            zeros,
        })
    }

    /// Append a commitment; returns its leaf index. `leaf` must be a canonical
    /// in-field BN254 scalar (e.g. `Note::commitment()`); `root()` /
    /// `authentication_path()` panic in `next_level` otherwise.
    pub fn insert(&mut self, leaf: [u8; 32]) -> usize {
        self.leaves.push(leaf);
        self.leaves.len() - 1
    }

    pub fn root(&self) -> [u8; 32] {
        let mut level = self.leaves.clone();
        for l in 0..TREE_DEPTH {
            level = Self::next_level(&level, self.zeros[l]);
        }
        // After TREE_DEPTH pairings the single remaining node is the root; an
        // empty tree collapses to the empty-root constant.
        level
            .first()
            .copied()
            .unwrap_or_else(|| pool_program::merkle::empty_root(&self.zeros).expect("empty_root"))
    }

    /// The authentication path (sibling per level, and the left/right bit per
    /// level) for `index` against the current root.
    pub fn authentication_path(&self, index: usize) -> MerklePath {
        let mut elements = [[0u8; 32]; TREE_DEPTH];
        let mut indices = [0u8; TREE_DEPTH];
        let mut level = self.leaves.clone();
        let mut pos = index;
        for l in 0..TREE_DEPTH {
            let bit = (pos % 2) as u8;
            indices[l] = bit;
            let sibling = pos ^ 1;
            elements[l] = level.get(sibling).copied().unwrap_or(self.zeros[l]);
            level = Self::next_level(&level, self.zeros[l]);
            pos /= 2;
        }
        MerklePath { elements, indices }
    }

    fn next_level(nodes: &[[u8; 32]], zero: [u8; 32]) -> Vec<[u8; 32]> {
        let mut out = Vec::with_capacity(nodes.len().div_ceil(2));
        let mut i = 0;
        while i < nodes.len() {
            let left = nodes[i];
            let right = nodes.get(i + 1).copied().unwrap_or(zero);
            out.push(poseidon::hash2(&left, &right).expect("node hash in-field"));
            i += 2;
        }
        out
    }
}
