use crate::poseidon::{hash2, is_in_field, PoseidonError};

pub const TREE_HEIGHT: usize = 20;
pub const ZERO_LEAF: [u8; 32] = [0u8; 32];

#[derive(Debug, PartialEq, Eq)]
pub enum MerkleError {
    TreeFull,
    NotInField,
    Hash,
}

impl From<PoseidonError> for MerkleError {
    fn from(e: PoseidonError) -> Self {
        match e {
            PoseidonError::NotInField => MerkleError::NotInField,
            PoseidonError::HashFailed => MerkleError::Hash,
        }
    }
}

/// Precompute zero-subtree roots: zeros[0] = ZERO_LEAF, zeros[i] = H(zeros[i-1], zeros[i-1]).
/// Cheap (TREE_HEIGHT-1 syscalls). NOTE (future opt): these are constant for a given
/// TREE_HEIGHT and could be hardcoded as a `const [[u8;32]; TREE_HEIGHT]` to save the
/// recomputation on every insert — deferred to avoid embedding magic bytes prematurely.
pub fn zeros() -> Result<[[u8; 32]; TREE_HEIGHT], MerkleError> {
    let mut z = [[0u8; 32]; TREE_HEIGHT];
    z[0] = ZERO_LEAF;
    for i in 1..TREE_HEIGHT {
        z[i] = hash2(&z[i - 1], &z[i - 1])?;
    }
    Ok(z)
}

/// Root of a fully-empty tree = H(zeros[H-1], zeros[H-1]).
pub fn empty_root(zeros: &[[u8; 32]; TREE_HEIGHT]) -> Result<[u8; 32], MerkleError> {
    Ok(hash2(&zeros[TREE_HEIGHT - 1], &zeros[TREE_HEIGHT - 1])?)
}

/// Standard Tornado-style incremental insert. Borrows the tree fields in place.
/// Returns the inserted leaf index.
pub fn insert(
    next_index: &mut u32,
    current_root: &mut [u8; 32],
    filled_subtrees: &mut [[u8; 32]; TREE_HEIGHT],
    leaf: [u8; 32],
) -> Result<u32, MerkleError> {
    if !is_in_field(&leaf) {
        return Err(MerkleError::NotInField);
    }
    if (*next_index as u64) >= (1u64 << TREE_HEIGHT) {
        return Err(MerkleError::TreeFull);
    }
    let z = zeros()?;

    let inserted_index = *next_index;
    let mut current_index = inserted_index;
    let mut current_hash = leaf;

    for i in 0..TREE_HEIGHT {
        let (left, right) = if current_index.is_multiple_of(2) {
            filled_subtrees[i] = current_hash;
            (current_hash, z[i])
        } else {
            (filled_subtrees[i], current_hash)
        };
        current_hash = hash2(&left, &right)?;
        current_index /= 2;
    }

    *current_root = current_hash;
    *next_index = inserted_index + 1;
    Ok(inserted_index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_root_is_nonzero_and_deterministic() {
        let z = zeros().unwrap();
        let r1 = empty_root(&z).unwrap();
        let r2 = empty_root(&z).unwrap();
        assert_eq!(r1, r2);
        assert_ne!(r1, [0u8; 32]);
    }

    #[test]
    fn insert_returns_sequential_indices_and_changes_root() {
        let z = zeros().unwrap();
        let mut next_index = 0u32;
        let mut root = empty_root(&z).unwrap();
        let mut filled = z; // empty tree: each level's filled subtree is its zero
        let empty = root;

        let i0 = insert(&mut next_index, &mut root, &mut filled, [7u8; 32]).unwrap();
        let root0 = root;
        let i1 = insert(&mut next_index, &mut root, &mut filled, [9u8; 32]).unwrap();

        assert_eq!(i0, 0);
        assert_eq!(i1, 1);
        assert_ne!(root0, empty, "first insert changes the root");
        assert_ne!(root, root0, "second insert changes the root again");
        assert_eq!(next_index, 2);
    }

    #[test]
    fn same_leaves_same_root_across_two_trees() {
        let z = zeros().unwrap();
        let build = |leaves: &[[u8; 32]]| {
            let mut ni = 0u32;
            let mut root = empty_root(&z).unwrap();
            let mut filled = z;
            for l in leaves {
                insert(&mut ni, &mut root, &mut filled, *l).unwrap();
            }
            root
        };
        let leaves = [[1u8; 32], [2u8; 32], [3u8; 32]];
        assert_eq!(
            build(&leaves),
            build(&leaves),
            "tree is a deterministic function of its leaves"
        );
    }

    #[test]
    fn rejects_out_of_field_leaf() {
        let z = zeros().unwrap();
        let mut ni = 0u32;
        let mut root = empty_root(&z).unwrap();
        let mut filled = z;
        assert!(matches!(
            insert(&mut ni, &mut root, &mut filled, [0xffu8; 32]),
            Err(MerkleError::NotInField)
        ));
    }

    #[test]
    fn insert_into_full_tree_errors() {
        // The TreeFull guard checks next_index before any hashing, so a full tree
        // is reachable in O(1) without 2^20 real inserts.
        let z = zeros().unwrap();
        let mut next_index = 1u32 << TREE_HEIGHT;
        let mut root = empty_root(&z).unwrap();
        let mut filled = z;
        assert!(matches!(
            insert(&mut next_index, &mut root, &mut filled, [1u8; 32]),
            Err(MerkleError::TreeFull)
        ));
    }
}
