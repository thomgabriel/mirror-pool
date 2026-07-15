use crate::merkle::{self, MerkleError, TREE_HEIGHT};
use crate::roots::{self, ROOT_HISTORY_SIZE};
use anchor_lang::prelude::*;

// `zero_copy`: this ~3.9 KB struct blows the 4 KB SBF stack frame if Borsh-
// (de)serialized by value (confirmed empirically: `Account<Pool>::try_from_unchecked` /
// `Pool::try_deserialize_unchecked` each report an ~7-8 KB estimated frame, i.e. two
// copies of the struct briefly coexisting — a well-known SBF codegen limit for large
// `#[account]` structs). Zero-copy reinterprets the account's own backing bytes in
// place via `AccountLoader`, so no copy of `Pool` is ever constructed on the stack.
// The (default, safe) `repr(C)` layout is required over `zero_copy(unsafe)`'s
// `repr(packed)`: `merkle::insert`/`roots::push` take `&mut u32` fields, and a
// packed repr makes every multi-byte field potentially misaligned, so the
// compiler rejects `&mut self.next_index` (E0793) outright.
#[account(zero_copy)]
pub struct Pool {
    pub mint: Pubkey,
    pub bump: u8,
    pub vault_bump: u8,
    // `repr(C)` would insert this gap implicitly to 4-byte-align `next_index`; bytemuck's
    // `Pod` derive panics at compile time on any *implicit* padding, so it's named and
    // explicit instead. Keeps the field order (and every other field's relative position)
    // identical to a non-zero-copy layout.
    _reserved: [u8; 2],
    pub next_index: u32,
    pub current_root: [u8; 32],
    pub filled_subtrees: [[u8; 32]; TREE_HEIGHT],
    pub roots: [[u8; 32]; ROOT_HISTORY_SIZE],
    pub current_root_index: u32,
}

impl Pool {
    pub const SPACE: usize = 8 + core::mem::size_of::<Pool>();

    /// Insert a commitment into the embedded tree, mutating fields in place (no large copy).
    ///
    /// `core::result::Result` (not the `Result<T>` alias `anchor_lang::prelude::*` brings
    /// into scope, which fixes the error type to `anchor_lang::error::Error`): this
    /// returns the pure `MerkleError`, same fix as `poseidon::hash2`.
    pub fn insert_commitment(&mut self, leaf: [u8; 32]) -> core::result::Result<u32, MerkleError> {
        merkle::insert(
            &mut self.next_index,
            &mut self.current_root,
            &mut self.filled_subtrees,
            leaf,
        )
    }

    /// Push a root into the embedded ring, in place.
    pub fn push_root(&mut self, root: [u8; 32]) {
        roots::push(&mut self.roots, &mut self.current_root_index, root);
    }
}
