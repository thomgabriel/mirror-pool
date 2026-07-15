# pool-program Foundations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the on-chain foundations of mirror-pool's `pool-program` — pool initialization, SOL vault custody, an incremental Poseidon Merkle commitment tree, a bounded root-history ring, and a nullifier set — as a fully-tested Anchor program, WITHOUT ZK proof verification (added in a later plan).

**Architecture:** A single Anchor program (`pool-program`) with pure, unit-tested crypto modules (`poseidon`, `merkle`, `roots`, `nullifier`) and thin instruction handlers (`initialize_pool`, `deposit`) that compose them. The commitment tree and root ring use BN254 Poseidon so they stay compatible with the Groth16 circuits added later. This plan is subsystem 1 of Phase 1 (see spec §2); it deliberately stops before proof verification.

**Tech Stack:** Rust 2021 · Anchor 0.31.x · `light-poseidon` (BN254 Poseidon) · `ark-bn254` · LiteSVM (Rust-native instruction tests).

**Design spec:** [`docs/superpowers/specs/2026-07-15-mirror-pool-design.md`](../specs/2026-07-15-mirror-pool-design.md)

## Global Constraints

- **Language:** Rust only (bounty requirement). No TypeScript tests — use LiteSVM in Rust.
- **Anchor:** `0.31.1`. **Solana/Agave:** `~2.1`. **Rust edition:** `2021`.
- **Poseidon:** `light-poseidon = "0.3"` with `ark-bn254 = "0.5"`, `ark-ff = "0.5"`. Hash is BN254 Poseidon, circom-compatible (`new_circom`), big-endian byte I/O (`hash_bytes_be`). This MUST match the circuits in the later `circuits` plan.
- **Field-element domain:** every 32-byte commitment/nullifier/root is a BN254 field element in **big-endian** bytes, and MUST be `< BN254_MODULUS`. Reject out-of-range inputs.
- **Tree:** `TREE_HEIGHT: usize = 20` (≈1.05M leaves; tunable later — spec §7). **Zero leaf:** `[0u8; 32]`.
- **Root ring:** `ROOT_HISTORY_SIZE: usize = 100` (spec / Cloak parity).
- **PDA seeds (verbatim):** pool `["pool", mint]`, vault `["vault", pool]`, tree `["tree", pool]`, nullifier `["nullifier", pool, nullifier_hash]`.
- **Program ID:** use the Anchor-generated dev keypair; do not hardcode a vanity ID in this plan.
- Every task ends green (`cargo test -p pool-program`) and is committed.

---

### Task 1: Anchor workspace scaffold

**Files:**
- Create: `Anchor.toml`
- Create: `Cargo.toml` (workspace)
- Create: `programs/pool-program/Cargo.toml`
- Create: `programs/pool-program/src/lib.rs`
- Create: `programs/pool-program/tests/scaffold.rs`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: an Anchor program crate `pool_program` that builds; a LiteSVM test harness pattern reused by later tasks.

- [ ] **Step 1: Create the workspace manifests**

`Anchor.toml`:
```toml
[toolchain]
anchor_version = "0.31.1"

[features]
resolution = true
skip-lint = false

[programs.localnet]
pool_program = "Poo11111111111111111111111111111111111111111"

[provider]
cluster = "Localnet"
wallet = "~/.config/solana/id.json"

[scripts]
test = "cargo test -p pool-program"
```

Workspace `Cargo.toml`:
```toml
[workspace]
members = ["programs/*"]
resolver = "2"

[profile.release]
overflow-checks = true
lto = "fat"
codegen-units = 1
```

`programs/pool-program/Cargo.toml`:
```toml
[package]
name = "pool-program"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "lib"]
name = "pool_program"

[features]
default = []
cpi = ["no-entrypoint"]
no-entrypoint = []
idl-build = ["anchor-lang/idl-build"]

[dependencies]
anchor-lang = "0.31.1"
light-poseidon = "0.3"
ark-bn254 = "0.5"
ark-ff = "0.5"

[dev-dependencies]
litesvm = "0.6"
solana-sdk = "~2.1"
```

- [ ] **Step 2: Write the minimal program entry**

`programs/pool-program/src/lib.rs`:
```rust
use anchor_lang::prelude::*;

declare_id!("Poo11111111111111111111111111111111111111111");

#[program]
pub mod pool_program {
    use super::*;

    pub fn ping(_ctx: Context<Ping>) -> Result<()> {
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Ping<'info> {
    pub signer: Signer<'info>,
}
```

- [ ] **Step 3: Write the failing build/test**

`programs/pool-program/tests/scaffold.rs`:
```rust
#[test]
fn program_crate_builds_and_id_is_stable() {
    // Compiles only if the program crate and its ID macro are wired correctly.
    let id = pool_program::ID;
    assert_eq!(id.to_string(), "Poo11111111111111111111111111111111111111111");
}
```

- [ ] **Step 4: Build the program SBF artifact**

Run: `anchor build`
Expected: builds `target/deploy/pool_program.so` with no errors.

- [ ] **Step 5: Run the test**

Run: `cargo test -p pool-program --test scaffold`
Expected: PASS (1 test).

- [ ] **Step 6: Commit**

```bash
git add Anchor.toml Cargo.toml programs/
git commit -m "feat(pool-program): scaffold Anchor workspace"
```

---

### Task 2: Poseidon hash module

**Files:**
- Create: `programs/pool-program/src/poseidon.rs`
- Modify: `programs/pool-program/src/lib.rs` (add `pub mod poseidon;`)

**Interfaces:**
- Consumes: `light_poseidon`, `ark_bn254::Fr`.
- Produces:
  - `pub const BN254_MODULUS_BE: [u8; 32]` — field modulus, big-endian.
  - `pub fn is_in_field(bytes: &[u8; 32]) -> bool`
  - `pub fn hash2(left: &[u8; 32], right: &[u8; 32]) -> core::result::Result<[u8; 32], PoseidonError>`
  - `pub enum PoseidonError { NotInField, HashFailed }`

- [ ] **Step 1: Write the failing test**

At the bottom of `programs/pool-program/src/poseidon.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash2_is_deterministic_and_nonzero() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let h1 = hash2(&a, &b).unwrap();
        let h2 = hash2(&a, &b).unwrap();
        assert_eq!(h1, h2, "Poseidon must be deterministic");
        assert_ne!(h1, [0u8; 32], "hash of nonzero inputs must be nonzero");
    }

    #[test]
    fn hash2_is_order_sensitive() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        assert_ne!(hash2(&a, &b).unwrap(), hash2(&b, &a).unwrap());
    }

    #[test]
    fn rejects_out_of_field_input() {
        let too_big = [0xffu8; 32]; // > BN254 modulus
        assert!(matches!(hash2(&too_big, &[0u8; 32]), Err(PoseidonError::NotInField)));
    }

    #[test]
    fn zero_subtree_matches_reference() {
        // zeros[1] = hash2(zeros[0], zeros[0]) where zeros[0] = [0u8;32]
        let z0 = [0u8; 32];
        let z1 = hash2(&z0, &z0).unwrap();
        assert_ne!(z1, z0);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p pool-program poseidon`
Expected: FAIL — `poseidon` module / `hash2` not found.

- [ ] **Step 3: Implement the module**

Top of `programs/pool-program/src/poseidon.rs`:
```rust
use ark_bn254::Fr;
use ark_ff::{BigInteger, PrimeField};
use light_poseidon::{Poseidon, PoseidonHasher};

/// BN254 scalar field modulus, big-endian.
pub const BN254_MODULUS_BE: [u8; 32] = [
    0x30, 0x64, 0x4e, 0x72, 0xe1, 0x31, 0xa0, 0x29, 0xb8, 0x50, 0x45, 0xb6, 0x81, 0x81, 0x58, 0x5d,
    0x28, 0x33, 0xe8, 0x48, 0x79, 0xb9, 0x70, 0x91, 0x43, 0xe1, 0xf5, 0x93, 0xf0, 0x00, 0x00, 0x01,
];

#[derive(Debug, PartialEq, Eq)]
pub enum PoseidonError {
    NotInField,
    HashFailed,
}

/// True iff `bytes` (big-endian) is a canonical BN254 field element (< modulus).
pub fn is_in_field(bytes: &[u8; 32]) -> bool {
    // Lexicographic big-endian comparison against the modulus.
    for i in 0..32 {
        if bytes[i] < BN254_MODULUS_BE[i] {
            return true;
        }
        if bytes[i] > BN254_MODULUS_BE[i] {
            return false;
        }
    }
    false // equal to modulus is NOT in field
}

/// Circom-compatible BN254 Poseidon over two field elements, big-endian I/O.
pub fn hash2(left: &[u8; 32], right: &[u8; 32]) -> core::result::Result<[u8; 32], PoseidonError> {
    if !is_in_field(left) || !is_in_field(right) {
        return Err(PoseidonError::NotInField);
    }
    let mut hasher = Poseidon::<Fr>::new_circom(2).map_err(|_| PoseidonError::HashFailed)?;
    let hash = hasher
        .hash_bytes_be(&[left.as_slice(), right.as_slice()])
        .map_err(|_| PoseidonError::HashFailed)?;
    Ok(hash)
}

// keep the arkworks trait imports used above from being flagged if light-poseidon changes
#[allow(unused_imports)]
use ark_ff::Field as _;
#[allow(unused)]
fn _fr_roundtrip_marker(_f: Fr) -> Option<()> {
    let _ = <Fr as PrimeField>::MODULUS;
    let _ = BigInteger::to_bytes_be;
    None
}
```

Add to `programs/pool-program/src/lib.rs` (after `use anchor_lang::prelude::*;`):
```rust
pub mod poseidon;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p pool-program poseidon`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add programs/pool-program/src/poseidon.rs programs/pool-program/src/lib.rs
git commit -m "feat(pool-program): BN254 Poseidon hash2 with field-range checks"
```

---

### Task 3: Incremental Merkle tree module

**Files:**
- Create: `programs/pool-program/src/merkle.rs`
- Modify: `programs/pool-program/src/lib.rs` (add `pub mod merkle;`)

**Interfaces:**
- Consumes: `poseidon::{hash2, PoseidonError}`.
- Produces:
  - `pub const TREE_HEIGHT: usize = 20;`
  - `pub struct MerkleState { pub next_index: u32, pub current_root: [u8;32], pub filled_subtrees: [[u8;32]; TREE_HEIGHT], pub zeros: [[u8;32]; TREE_HEIGHT] }`
  - `pub fn init_state() -> Result<MerkleState, MerkleError>`
  - `pub fn insert(state: &mut MerkleState, leaf: [u8;32]) -> Result<u32, MerkleError>` (returns leaf index)
  - `pub enum MerkleError { TreeFull, NotInField, Hash }`

- [ ] **Step 1: Write the failing tests**

At the bottom of `programs/pool-program/src/merkle.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_root_is_all_zeros_subtree() {
        let s = init_state().unwrap();
        // Root of an empty tree = zeros[TREE_HEIGHT-1] hashed once more == top zero.
        // It must be deterministic and nonzero (since it hashes zero leaves upward).
        assert_ne!(s.current_root, [0u8; 32]);
        assert_eq!(s.next_index, 0);
    }

    #[test]
    fn insert_returns_sequential_indices_and_changes_root() {
        let mut s = init_state().unwrap();
        let empty_root = s.current_root;
        let i0 = insert(&mut s, [7u8; 32]).unwrap();
        let root0 = s.current_root;
        let i1 = insert(&mut s, [9u8; 32]).unwrap();
        assert_eq!(i0, 0);
        assert_eq!(i1, 1);
        assert_ne!(root0, empty_root, "first insert must change the root");
        assert_ne!(s.current_root, root0, "second insert must change the root again");
        assert_eq!(s.next_index, 2);
    }

    #[test]
    fn same_leaves_same_root_across_two_trees() {
        let mut a = init_state().unwrap();
        let mut b = init_state().unwrap();
        for leaf in [[1u8;32],[2u8;32],[3u8;32]] {
            insert(&mut a, leaf).unwrap();
            insert(&mut b, leaf).unwrap();
        }
        assert_eq!(a.current_root, b.current_root, "tree is a deterministic function of its leaves");
    }

    #[test]
    fn rejects_out_of_field_leaf() {
        let mut s = init_state().unwrap();
        assert!(matches!(insert(&mut s, [0xffu8; 32]), Err(MerkleError::NotInField)));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p pool-program merkle`
Expected: FAIL — `merkle` module not found.

- [ ] **Step 3: Implement the module**

Top of `programs/pool-program/src/merkle.rs`:
```rust
use crate::poseidon::{hash2, is_in_field, PoseidonError};

pub const TREE_HEIGHT: usize = 20;
pub const ZERO_LEAF: [u8; 32] = [0u8; 32];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MerkleState {
    pub next_index: u32,
    pub current_root: [u8; 32],
    pub filled_subtrees: [[u8; 32]; TREE_HEIGHT],
    pub zeros: [[u8; 32]; TREE_HEIGHT],
}

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
/// Initial tree root and filled_subtrees both start from these zeros.
pub fn init_state() -> Result<MerkleState, MerkleError> {
    let mut zeros = [[0u8; 32]; TREE_HEIGHT];
    zeros[0] = ZERO_LEAF;
    for i in 1..TREE_HEIGHT {
        zeros[i] = hash2(&zeros[i - 1], &zeros[i - 1])?;
    }
    // Root of a fully-empty tree = H(zeros[H-1], zeros[H-1]).
    let current_root = hash2(&zeros[TREE_HEIGHT - 1], &zeros[TREE_HEIGHT - 1])?;
    Ok(MerkleState {
        next_index: 0,
        current_root,
        filled_subtrees: zeros, // empty tree: each level's filled subtree is its zero
        zeros,
    })
}

/// Standard Tornado-style incremental insert. Returns the inserted leaf index.
pub fn insert(state: &mut MerkleState, leaf: [u8; 32]) -> Result<u32, MerkleError> {
    if !is_in_field(&leaf) {
        return Err(MerkleError::NotInField);
    }
    if (state.next_index as u64) >= (1u64 << TREE_HEIGHT) {
        return Err(MerkleError::TreeFull);
    }

    let inserted_index = state.next_index;
    let mut current_index = inserted_index;
    let mut current_hash = leaf;

    for i in 0..TREE_HEIGHT {
        let (left, right) = if current_index % 2 == 0 {
            // left child on this level: right sibling is the level's zero, and we
            // record this node as the new filled subtree for the level.
            state.filled_subtrees[i] = current_hash;
            (current_hash, state.zeros[i])
        } else {
            (state.filled_subtrees[i], current_hash)
        };
        current_hash = hash2(&left, &right)?;
        current_index /= 2;
    }

    state.current_root = current_hash;
    state.next_index = inserted_index + 1;
    Ok(inserted_index)
}
```

Add to `programs/pool-program/src/lib.rs`:
```rust
pub mod merkle;
```

- [ ] **Step 4: Run to verify passing**

Run: `cargo test -p pool-program merkle`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add programs/pool-program/src/merkle.rs programs/pool-program/src/lib.rs
git commit -m "feat(pool-program): incremental Poseidon Merkle tree (height 20)"
```

---

### Task 4: Root-history ring buffer module

**Files:**
- Create: `programs/pool-program/src/roots.rs`
- Modify: `programs/pool-program/src/lib.rs` (add `pub mod roots;`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub const ROOT_HISTORY_SIZE: usize = 100;`
  - `pub struct RootRing { pub roots: [[u8;32]; ROOT_HISTORY_SIZE], pub current_index: u32 }`
  - `pub fn new_ring(initial_root: [u8;32]) -> RootRing`
  - `pub fn push(ring: &mut RootRing, root: [u8;32])`
  - `pub fn is_known(ring: &RootRing, root: &[u8;32]) -> bool`

- [ ] **Step 1: Write the failing tests**

At the bottom of `programs/pool-program/src/roots.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_root_is_known() {
        let ring = new_ring([5u8; 32]);
        assert!(is_known(&ring, &[5u8; 32]));
        assert!(!is_known(&ring, &[6u8; 32]));
    }

    #[test]
    fn pushed_root_becomes_known() {
        let mut ring = new_ring([0u8; 32]);
        push(&mut ring, [1u8; 32]);
        assert!(is_known(&ring, &[1u8; 32]));
        assert!(is_known(&ring, &[0u8; 32]), "recent history still valid");
    }

    #[test]
    fn old_roots_evicted_after_ring_wraps() {
        let mut ring = new_ring([0u8; 32]);
        // push ROOT_HISTORY_SIZE fresh roots so [0u8;32] falls out of the window
        for n in 1..=(ROOT_HISTORY_SIZE as u8) {
            let mut r = [0u8; 32];
            r[0] = n;
            push(&mut ring, r);
        }
        assert!(!is_known(&ring, &[0u8; 32]), "root older than the window is rejected");
        let mut newest = [0u8; 32];
        newest[0] = ROOT_HISTORY_SIZE as u8;
        assert!(is_known(&ring, &newest));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p pool-program roots`
Expected: FAIL — `roots` module not found.

- [ ] **Step 3: Implement the module**

Top of `programs/pool-program/src/roots.rs`:
```rust
pub const ROOT_HISTORY_SIZE: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootRing {
    pub roots: [[u8; 32]; ROOT_HISTORY_SIZE],
    pub current_index: u32,
}

pub fn new_ring(initial_root: [u8; 32]) -> RootRing {
    let mut roots = [[0u8; 32]; ROOT_HISTORY_SIZE];
    roots[0] = initial_root;
    RootRing { roots, current_index: 0 }
}

pub fn push(ring: &mut RootRing, root: [u8; 32]) {
    let next = (ring.current_index as usize + 1) % ROOT_HISTORY_SIZE;
    ring.roots[next] = root;
    ring.current_index = next as u32;
}

/// A root is "known" iff it equals any non-empty slot in the ring.
pub fn is_known(ring: &RootRing, root: &[u8; 32]) -> bool {
    if *root == [0u8; 32] {
        return false; // the zero sentinel is never a valid root
    }
    ring.roots.iter().any(|r| r == root)
}
```

Add to `programs/pool-program/src/lib.rs`:
```rust
pub mod roots;
```

- [ ] **Step 4: Run to verify passing**

Run: `cargo test -p pool-program roots`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add programs/pool-program/src/roots.rs programs/pool-program/src/lib.rs
git commit -m "feat(pool-program): 100-entry root-history ring buffer"
```

---

### Task 5: Pool state + `initialize_pool` instruction

**Files:**
- Create: `programs/pool-program/src/state.rs`
- Modify: `programs/pool-program/src/lib.rs` (add state, `initialize_pool` handler)
- Create: `programs/pool-program/tests/initialize_pool.rs`

**Interfaces:**
- Consumes: `merkle::{init_state, MerkleState, TREE_HEIGHT}`, `roots::{new_ring, RootRing, ROOT_HISTORY_SIZE}`.
- Produces:
  - `#[account] pub struct Pool { pub mint: Pubkey, pub bump: u8, pub vault_bump: u8, pub next_index: u32, pub current_root: [u8;32], pub filled_subtrees: [[u8;32]; TREE_HEIGHT], pub zeros: [[u8;32]; TREE_HEIGHT], pub roots: [[u8;32]; ROOT_HISTORY_SIZE], pub current_root_index: u32 }`
  - instruction `initialize_pool(ctx)` seeded at `["pool", mint]`.
  - Helpers on `Pool`: `fn merkle(&self) -> MerkleState`, `fn store_merkle(&mut self, m: &MerkleState)`, `fn ring(&self) -> RootRing`, `fn store_ring(&mut self, r: &RootRing)`.

> **Note on layout:** we flatten `MerkleState`/`RootRing` fields into the `Pool` account (rather than nest the pure structs) so Anchor can derive `#[account]` space directly. The helpers convert between the flat account and the pure module structs.

- [ ] **Step 1: Write the state + helpers**

`programs/pool-program/src/state.rs`:
```rust
use anchor_lang::prelude::*;
use crate::merkle::{MerkleState, TREE_HEIGHT};
use crate::roots::{RootRing, ROOT_HISTORY_SIZE};

#[account]
pub struct Pool {
    pub mint: Pubkey,
    pub bump: u8,
    pub vault_bump: u8,
    pub next_index: u32,
    pub current_root: [u8; 32],
    pub filled_subtrees: [[u8; 32]; TREE_HEIGHT],
    pub zeros: [[u8; 32]; TREE_HEIGHT],
    pub roots: [[u8; 32]; ROOT_HISTORY_SIZE],
    pub current_root_index: u32,
}

impl Pool {
    // discriminator(8) + mint(32) + bump(1) + vault_bump(1) + next_index(4)
    // + current_root(32) + filled_subtrees(32*H) + zeros(32*H)
    // + roots(32*RING) + current_root_index(4)
    pub const SPACE: usize = 8 + 32 + 1 + 1 + 4 + 32
        + 32 * TREE_HEIGHT
        + 32 * TREE_HEIGHT
        + 32 * ROOT_HISTORY_SIZE
        + 4;

    pub fn merkle(&self) -> MerkleState {
        MerkleState {
            next_index: self.next_index,
            current_root: self.current_root,
            filled_subtrees: self.filled_subtrees,
            zeros: self.zeros,
        }
    }

    pub fn store_merkle(&mut self, m: &MerkleState) {
        self.next_index = m.next_index;
        self.current_root = m.current_root;
        self.filled_subtrees = m.filled_subtrees;
        self.zeros = m.zeros;
    }

    pub fn ring(&self) -> RootRing {
        RootRing { roots: self.roots, current_index: self.current_root_index }
    }

    pub fn store_ring(&mut self, r: &RootRing) {
        self.roots = r.roots;
        self.current_root_index = r.current_index;
    }
}
```

- [ ] **Step 2: Wire the instruction + errors into `lib.rs`**

Replace the body of `programs/pool-program/src/lib.rs` with:
```rust
use anchor_lang::prelude::*;

pub mod poseidon;
pub mod merkle;
pub mod roots;
pub mod state;

use crate::merkle::init_state;
use crate::roots::new_ring;
use crate::state::Pool;

declare_id!("Poo11111111111111111111111111111111111111111");

#[program]
pub mod pool_program {
    use super::*;

    pub fn initialize_pool(ctx: Context<InitializePool>) -> Result<()> {
        let m = init_state().map_err(|_| error!(PoolError::MerkleInit))?;
        let ring = new_ring(m.current_root);

        let pool = &mut ctx.accounts.pool;
        pool.mint = ctx.accounts.mint.key();
        pool.bump = ctx.bumps.pool;
        pool.vault_bump = ctx.bumps.vault;
        pool.store_merkle(&m);
        pool.store_ring(&ring);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializePool<'info> {
    #[account(
        init,
        payer = payer,
        space = Pool::SPACE,
        seeds = [b"pool", mint.key().as_ref()],
        bump
    )]
    pub pool: Account<'info, Pool>,

    /// CHECK: SOL vault PDA, owned by the system program; only lamports are held here.
    #[account(
        seeds = [b"vault", pool.key().as_ref()],
        bump
    )]
    pub vault: UncheckedAccount<'info>,

    /// CHECK: mint is used only as a PDA seed / label in this plan (no SPL logic yet).
    pub mint: UncheckedAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[error_code]
pub enum PoolError {
    #[msg("failed to initialize the merkle tree")]
    MerkleInit,
    #[msg("deposit amount must be greater than zero")]
    ZeroDeposit,
    #[msg("commitment is not a valid field element")]
    CommitmentNotInField,
    #[msg("merkle tree is full")]
    TreeFull,
}
```

- [ ] **Step 3: Write the failing LiteSVM test**

`programs/pool-program/tests/initialize_pool.rs`:
```rust
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount, instruction::{AccountMeta, Instruction},
    message::Message, pubkey::Pubkey, signature::{Keypair, Signer}, system_program,
    transaction::Transaction,
};

const PROGRAM_ID: Pubkey = solana_sdk::pubkey!("Poo11111111111111111111111111111111111111111");

// Anchor discriminator = first 8 bytes of sha256("global:initialize_pool")
fn init_pool_discriminator() -> [u8; 8] {
    use solana_sdk::hash::hash;
    let h = hash(b"global:initialize_pool");
    let mut d = [0u8; 8];
    d.copy_from_slice(&h.to_bytes()[..8]);
    d
}

fn setup() -> (LiteSVM, Keypair) {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(PROGRAM_ID, "target/deploy/pool_program.so").unwrap();
    (svm, payer)
}

#[test]
fn initialize_pool_creates_account_with_nonzero_root() {
    let (mut svm, payer) = setup();
    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &PROGRAM_ID);
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &PROGRAM_ID);

    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: init_pool_discriminator().to_vec(),
    };
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, svm.latest_blockhash());
    svm.send_transaction(tx).unwrap();

    let acct = svm.get_account(&pool).unwrap();
    assert!(acct.data().len() > 8, "pool account allocated");
    // current_root sits at offset 8(disc)+32(mint)+1(bump)+1(vault_bump)+4(next_index) = 46
    let current_root = &acct.data()[46..78];
    assert_ne!(current_root, &[0u8; 32], "empty-tree root must be nonzero");
}
```

- [ ] **Step 4: Build then run the test to verify it fails, then passes**

Run: `anchor build && cargo test -p pool-program --test initialize_pool`
Expected first run while implementing incrementally: FAIL if handler absent; after Steps 1–2 are in place: PASS (1 test).

- [ ] **Step 5: Commit**

```bash
git add programs/pool-program/src/state.rs programs/pool-program/src/lib.rs programs/pool-program/tests/initialize_pool.rs
git commit -m "feat(pool-program): Pool account + initialize_pool instruction"
```

---

### Task 6: Vault custody + `deposit` instruction

**Files:**
- Modify: `programs/pool-program/src/lib.rs` (add `deposit` handler + accounts)
- Create: `programs/pool-program/tests/deposit.rs`

**Interfaces:**
- Consumes: `Pool` helpers, `merkle::insert`, `roots::push`, `poseidon::is_in_field`.
- Produces: instruction `deposit(ctx, commitment: [u8;32], amount: u64)` that (a) transfers `amount` lamports payer→vault, (b) inserts `commitment` into the tree, (c) pushes the new root to the ring, (d) emits `DepositEvent { commitment, leaf_index, new_root }`.

- [ ] **Step 1: Add the handler, accounts, and event to `lib.rs`**

Inside `#[program] pub mod pool_program`, add:
```rust
    pub fn deposit(ctx: Context<Deposit>, commitment: [u8; 32], amount: u64) -> Result<()> {
        require!(amount > 0, PoolError::ZeroDeposit);
        require!(crate::poseidon::is_in_field(&commitment), PoolError::CommitmentNotInField);

        // (a) move lamports payer -> vault (system CPI; vault is a system-owned PDA)
        let cpi = anchor_lang::system_program::Transfer {
            from: ctx.accounts.payer.to_account_info(),
            to: ctx.accounts.vault.to_account_info(),
        };
        anchor_lang::system_program::transfer(
            CpiContext::new(ctx.accounts.system_program.to_account_info(), cpi),
            amount,
        )?;

        // (b) insert commitment into the merkle tree
        let pool = &mut ctx.accounts.pool;
        let mut m = pool.merkle();
        let leaf_index = crate::merkle::insert(&mut m, commitment).map_err(|e| match e {
            crate::merkle::MerkleError::TreeFull => error!(PoolError::TreeFull),
            crate::merkle::MerkleError::NotInField => error!(PoolError::CommitmentNotInField),
            crate::merkle::MerkleError::Hash => error!(PoolError::MerkleInit),
        })?;
        pool.store_merkle(&m);

        // (c) push new root to the ring
        let mut ring = pool.ring();
        crate::roots::push(&mut ring, m.current_root);
        pool.store_ring(&ring);

        // (d) event
        emit!(DepositEvent { commitment, leaf_index, new_root: m.current_root });
        Ok(())
    }
```

Add the accounts + event below `InitializePool`:
```rust
#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(
        mut,
        seeds = [b"pool", pool.mint.as_ref()],
        bump = pool.bump
    )]
    pub pool: Account<'info, Pool>,

    /// CHECK: SOL vault PDA (system-owned); receives lamports.
    #[account(
        mut,
        seeds = [b"vault", pool.key().as_ref()],
        bump = pool.vault_bump
    )]
    pub vault: UncheckedAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[event]
pub struct DepositEvent {
    pub commitment: [u8; 32],
    pub leaf_index: u32,
    pub new_root: [u8; 32],
}
```

- [ ] **Step 2: Write the failing test**

`programs/pool-program/tests/deposit.rs`:
```rust
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount, instruction::{AccountMeta, Instruction},
    message::Message, pubkey::Pubkey, signature::{Keypair, Signer}, system_program,
    transaction::Transaction,
};

const PROGRAM_ID: Pubkey = solana_sdk::pubkey!("Poo11111111111111111111111111111111111111111");

fn disc(name: &str) -> [u8; 8] {
    use solana_sdk::hash::hash;
    let h = hash(format!("global:{name}").as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h.to_bytes()[..8]);
    d
}

fn setup_pool() -> (LiteSVM, Keypair, Pubkey, Pubkey) {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(PROGRAM_ID, "target/deploy/pool_program.so").unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &PROGRAM_ID);
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &PROGRAM_ID);

    let mut data = disc("initialize_pool").to_vec();
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: std::mem::take(&mut data),
    };
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, svm.latest_blockhash());
    svm.send_transaction(tx).unwrap();
    (svm, payer, pool, vault)
}

fn deposit_ix(pool: Pubkey, vault: Pubkey, payer: Pubkey, commitment: [u8; 32], amount: u64) -> Instruction {
    let mut data = disc("deposit").to_vec();
    data.extend_from_slice(&commitment);
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    }
}

#[test]
fn deposit_moves_lamports_and_advances_tree() {
    let (mut svm, payer, pool, vault) = setup_pool();

    let root_before = svm.get_account(&pool).unwrap().data()[46..78].to_vec();
    let vault_before = svm.get_account(&vault).map(|a| a.lamports()).unwrap_or(0);

    let commitment = { let mut c = [0u8; 32]; c[31] = 42; c };
    let ix = deposit_ix(pool, vault, payer.pubkey(), commitment, 1_000_000);
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, svm.latest_blockhash());
    svm.send_transaction(tx).unwrap();

    let vault_after = svm.get_account(&vault).unwrap().lamports();
    assert_eq!(vault_after - vault_before, 1_000_000, "vault received the deposit");

    let data_after = svm.get_account(&pool).unwrap().data().to_vec();
    let root_after = &data_after[46..78];
    assert_ne!(root_after, root_before.as_slice(), "root advanced after deposit");
    // next_index sits at offset 8+32+1+1 = 42 (u32 LE)
    let next_index = u32::from_le_bytes(data_after[42..46].try_into().unwrap());
    assert_eq!(next_index, 1, "one leaf inserted");
}

#[test]
fn deposit_rejects_zero_amount() {
    let (mut svm, payer, pool, vault) = setup_pool();
    let commitment = { let mut c = [0u8; 32]; c[31] = 7; c };
    let ix = deposit_ix(pool, vault, payer.pubkey(), commitment, 0);
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, svm.latest_blockhash());
    assert!(svm.send_transaction(tx).is_err(), "zero deposit must fail");
}
```

- [ ] **Step 3: Build + run to verify pass**

Run: `anchor build && cargo test -p pool-program --test deposit`
Expected: PASS (2 tests).

- [ ] **Step 4: Commit**

```bash
git add programs/pool-program/src/lib.rs programs/pool-program/tests/deposit.rs
git commit -m "feat(pool-program): deposit — vault custody + tree insert + root push"
```

---

### Task 7: Nullifier set module + `is_spent` helper

**Files:**
- Create: `programs/pool-program/src/nullifier.rs`
- Modify: `programs/pool-program/src/lib.rs` (add `pub mod nullifier;` + `NullifierRecord` account + `mark_spent` handler used later by withdraw)
- Create: `programs/pool-program/tests/nullifier.rs`

**Interfaces:**
- Consumes: Anchor account model.
- Produces:
  - `#[account] pub struct NullifierRecord { pub spent: bool }` at seeds `["nullifier", pool, nullifier_hash]`.
  - instruction `mark_spent(ctx, nullifier_hash: [u8;32])` — `init`s the record PDA (its existence == spent); re-marking the same nullifier fails because `init` fails on an existing account. (Withdraw will call this pattern in a later plan; here it is exercised standalone.)
  - `fn nullifier_seeds(pool, hash)` documented for later consumers.

> **Why a PDA-per-nullifier:** a nullifier's PDA *existing* is the "spent" marker — the classic double-spend guard. `init` atomically fails if the PDA already exists, so double-spend protection is free.

- [ ] **Step 1: Add the account + handler to `lib.rs`**

Add `pub mod nullifier;` near the other module declarations, then inside the `#[program]` module:
```rust
    pub fn mark_spent(ctx: Context<MarkSpent>, _nullifier_hash: [u8; 32]) -> Result<()> {
        ctx.accounts.nullifier.spent = true;
        Ok(())
    }
```

Add below the `Deposit` accounts:
```rust
#[derive(Accounts)]
#[instruction(nullifier_hash: [u8; 32])]
pub struct MarkSpent<'info> {
    #[account(
        seeds = [b"pool", pool.mint.as_ref()],
        bump = pool.bump
    )]
    pub pool: Account<'info, Pool>,

    #[account(
        init,
        payer = payer,
        space = 8 + 1,
        seeds = [b"nullifier", pool.key().as_ref(), nullifier_hash.as_ref()],
        bump
    )]
    pub nullifier: Account<'info, crate::nullifier::NullifierRecord>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}
```

- [ ] **Step 2: Create the nullifier module**

`programs/pool-program/src/nullifier.rs`:
```rust
use anchor_lang::prelude::*;

/// Existence of this PDA at seeds ["nullifier", pool, nullifier_hash] means the
/// nullifier has been spent. `spent` is always true once created; the flag is a
/// readability aid — the security property is the PDA's existence.
#[account]
pub struct NullifierRecord {
    pub spent: bool,
}
```

- [ ] **Step 3: Write the failing test (double-spend rejected)**

`programs/pool-program/tests/nullifier.rs`:
```rust
use litesvm::LiteSVM;
use solana_sdk::{
    instruction::{AccountMeta, Instruction}, message::Message, pubkey::Pubkey,
    signature::{Keypair, Signer}, system_program, transaction::Transaction,
};

const PROGRAM_ID: Pubkey = solana_sdk::pubkey!("Poo11111111111111111111111111111111111111111");

fn disc(name: &str) -> [u8; 8] {
    use solana_sdk::hash::hash;
    let h = hash(format!("global:{name}").as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h.to_bytes()[..8]);
    d
}

fn setup_pool() -> (LiteSVM, Keypair, Pubkey) {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(PROGRAM_ID, "target/deploy/pool_program.so").unwrap();
    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &PROGRAM_ID);
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &PROGRAM_ID);
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new_readonly(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: disc("initialize_pool").to_vec(),
    };
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    svm.send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash())).unwrap();
    (svm, payer, pool)
}

fn mark_spent_tx(svm: &LiteSVM, payer: &Keypair, pool: Pubkey, nh: [u8; 32]) -> Transaction {
    let (nullifier, _) = Pubkey::find_program_address(
        &[b"nullifier", pool.as_ref(), nh.as_ref()], &PROGRAM_ID);
    let mut data = disc("mark_spent").to_vec();
    data.extend_from_slice(&nh);
    let ix = Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new_readonly(pool, false),
            AccountMeta::new(nullifier, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    Transaction::new(&[payer], msg, svm.latest_blockhash())
}

#[test]
fn first_mark_succeeds_second_fails() {
    let (mut svm, payer, pool) = setup_pool();
    let nh = { let mut n = [0u8; 32]; n[31] = 99; n };

    let tx1 = mark_spent_tx(&svm, &payer, pool, nh);
    svm.send_transaction(tx1).unwrap();

    let tx2 = mark_spent_tx(&svm, &payer, pool, nh);
    assert!(svm.send_transaction(tx2).is_err(), "re-spending the same nullifier must fail (PDA already exists)");
}
```

- [ ] **Step 4: Build + run to verify pass**

Run: `anchor build && cargo test -p pool-program --test nullifier`
Expected: PASS (1 test).

- [ ] **Step 5: Run the whole suite**

Run: `cargo test -p pool-program`
Expected: PASS (all unit + integration tests green).

- [ ] **Step 6: Commit**

```bash
git add programs/pool-program/src/nullifier.rs programs/pool-program/src/lib.rs programs/pool-program/tests/nullifier.rs
git commit -m "feat(pool-program): nullifier PDA set with double-spend guard"
```

---

## What this plan delivers

A deployable Anchor `pool-program` with: pool initialization, SOL vault custody, an incremental height-20 Poseidon Merkle commitment tree, a 100-entry root-history ring, and a PDA-based nullifier set with double-spend protection — all covered by Rust unit + LiteSVM integration tests.

## Explicitly deferred to later plans

- **ZK proof verification** (`commit_intent` / `withdraw` verifying Groth16) — needs the `circuits` plan first.
- **SPL-token pools** — this plan custodies native SOL only; the `mint` seed is a label. Token-2022 / SPL vaults come with the action-adapters.
- **Rounds, `k`-floor, `PooledAction`, incentives, viewing keys** — Phases 2–4.

## Self-review notes

- **Spec coverage (Phase-1-foundations slice):** pool init ✓ (T5), custody ✓ (T6), Merkle tree height-20 ✓ (T3), 100-root ring ✓ (T4), nullifier set ✓ (T7), Poseidon/field-range ✓ (T2). Proof verification intentionally out of scope (stated above).
- **Placeholder scan:** none — every step has concrete code/commands.
- **Type consistency:** `Pool` helpers (`merkle`/`store_merkle`/`ring`/`store_ring`) match `MerkleState`/`RootRing` field names; `insert` returns `u32` leaf index consumed by `DepositEvent.leaf_index`; discriminator offsets in tests (42=next_index, 46..78=current_root) match `Pool::SPACE` layout.
- **Verify at implementation time:** exact `light-poseidon` 0.3 API (`new_circom`, `hash_bytes_be`) and `litesvm` 0.6 API (`add_program_from_file`, `send_transaction`) against installed crate versions; adjust import paths if the crates moved them.
