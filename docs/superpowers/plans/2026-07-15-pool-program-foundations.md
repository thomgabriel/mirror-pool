# pool-program Foundations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the on-chain foundations of mirror-pool's `pool-program` — pool initialization, SOL vault custody, an incremental Poseidon Merkle commitment tree, a bounded root-history ring, and a nullifier set — as a fully-tested Anchor program, WITHOUT ZK proof verification (added in a later plan).

**Architecture:** A single Anchor program (`pool-program`) with pure, unit-tested crypto modules (`poseidon`, `merkle`, `roots`, `nullifier`) and thin instruction handlers (`initialize_pool`, `deposit`) that compose them. The commitment tree and root ring use BN254 Poseidon so they stay compatible with the Groth16 circuits added later. This plan is subsystem 1 of Phase 1 (see spec §2); it deliberately stops before proof verification.

**Tech Stack:** Rust 2021 · Anchor 0.31.x · native `solana_program::poseidon` syscall (BN254) · LiteSVM (Rust-native instruction tests).

**Design spec:** [`docs/superpowers/specs/2026-07-15-mirror-pool-design.md`](../specs/2026-07-15-mirror-pool-design.md)

> **Revision note (2026-07-15):** revised after an independent review. Key hardening: no hardcoded program ID (use the Anchor-generated keypair); `Box` the ~4 KB `Pool` account and mutate its fields in place (avoids a >4096-byte SBF stack frame); pure modules take `&mut` field references rather than copying whole structs; `zeros[]` computed on demand (not stored); LiteSVM tests use an absolute `.so` path and reference `pool_program::ID`; root-ring tests use non-zero seed roots; vault funded to rent-exempt minimum at init.

## Global Constraints

- **Language:** Rust only (bounty requirement). No TypeScript tests — use LiteSVM in Rust.
- **Anchor:** `0.31.1`. **Solana/Agave:** `~2.1`. **Rust edition:** `2021`.
- **Poseidon (on-chain):** hashing uses the **native Solana `poseidon` syscall** — `anchor_lang::solana_program::poseidon::hashv(Parameters::Bn254X5, Endianness::BigEndian, &[..])` — **NOT** `light-poseidon` in-BPF. The syscall is cheap and ships a **host implementation**, so `hash2` also runs under `cargo test`. `Bn254X5` is circomlib-compatible and MUST match the later `circuits` plan (same params **and** the same zero-leaf value = field element `0`).
- **Field-element domain:** every 32-byte commitment/nullifier/root is a BN254 field element in **big-endian** bytes and MUST be `< BN254_MODULUS`. Reject out-of-range inputs.
- **Tree:** `TREE_HEIGHT: usize = 20` (≈1.05M leaves; tunable later — spec §7). **Zero leaf:** field element `0` = `[0u8; 32]`.
- **Root ring:** `ROOT_HISTORY_SIZE: usize = 100` (spec / Cloak parity).
- **PDA seeds (this plan):** pool `["pool", mint]`, vault `["vault", pool]`, nullifier `["nullifier", pool, nullifier_hash]`. There is **no separate tree account** — the Merkle state is embedded in the `Pool` account for the SOL MVP. (Spec §3.1 has been reconciled to `["vault", pool]`; a dedicated `["tree", pool]` account may be introduced in a later plan if the tree outgrows `Pool`.)
- **Program ID:** **do not hardcode a vanity string.** Use the Anchor-generated dev keypair (`anchor keys sync`) and reference it via `pool_program::ID` in tests.
- **Compute budget:** `deposit`/`initialize_pool` each perform ~2·`TREE_HEIGHT` Poseidon syscalls plus Borsh (de)serialization of the multi-KB `Pool` account, so **LiteSVM test transactions prepend `ComputeBudgetInstruction::set_compute_unit_limit(400_000)`** (free headroom in tests) and **log `metadata.compute_units_consumed`** to record the real cost. Tune the on-chain expectation from the measured value.
- **Account size / stack:** `Pool` is multi-KB; every handler takes it as `Box<Account<'info, Pool>>` and mutates fields **in place** (never copy the whole tree/ring struct onto the stack).
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
- Produces: an Anchor program crate `pool_program` that builds with a **generated** program ID; a LiteSVM test harness pattern reused by later tasks.

- [ ] **Step 1: Create the workspace manifests**

`Anchor.toml` (the `pool_program` address is a placeholder; Step 4 rewrites it via `anchor keys sync`):
```toml
[toolchain]
anchor_version = "0.31.1"

[features]
resolution = true
skip-lint = false

[programs.localnet]
pool_program = "11111111111111111111111111111111"

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
anchor-lang = "0.31.1"   # re-exports solana_program::poseidon (native syscall + host impl)

[dev-dependencies]
litesvm = "0.6"
solana-sdk = "~2.1"
```

> **Note:** the Poseidon syscall lives in `anchor_lang::solana_program::poseidon` — no `light-poseidon`/`ark-*` direct dependency. If a specific Anchor/solana-program version gates `poseidon` behind a feature, enable it here (verify at implementation time).

- [ ] **Step 2: Write the minimal program entry**

`programs/pool-program/src/lib.rs`:
```rust
use anchor_lang::prelude::*;

// Overwritten by `anchor keys sync` in Step 4 with the generated keypair's pubkey.
declare_id!("11111111111111111111111111111111");

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

- [ ] **Step 3: Write the failing test**

`programs/pool-program/tests/scaffold.rs`:
```rust
#[test]
fn program_id_is_set_to_generated_keypair() {
    // After `anchor keys sync`, declare_id! holds the generated (non-zero) pubkey.
    assert_ne!(
        pool_program::ID.to_bytes(),
        [0u8; 32],
        "run `anchor keys sync` so declare_id! is the generated program keypair"
    );
}
```

- [ ] **Step 4: Generate the program keypair and sync the ID**

Run: `anchor keys sync`
Expected: writes `target/deploy/pool_program-keypair.json` and rewrites `declare_id!(...)` in `lib.rs` **and** the `Anchor.toml` address to the generated pubkey.

- [ ] **Step 5: Build the program SBF artifact**

Run: `anchor build`
Expected: builds `target/deploy/pool_program.so` with no errors.

- [ ] **Step 6: Run the test**

Run: `cargo test -p pool-program --test scaffold`
Expected: PASS (1 test).

- [ ] **Step 7: Commit**

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
- Consumes: `anchor_lang::solana_program::poseidon` (native syscall + host impl).
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
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p pool-program poseidon`
Expected: FAIL — `poseidon` module / `hash2` not found.

- [ ] **Step 3: Implement the module**

Top of `programs/pool-program/src/poseidon.rs`:
```rust
use anchor_lang::solana_program::poseidon::{hashv, Endianness, Parameters};

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
/// Uses the native Solana `poseidon` syscall on-chain; the same call has a host
/// implementation, so this also runs under `cargo test`.
pub fn hash2(left: &[u8; 32], right: &[u8; 32]) -> core::result::Result<[u8; 32], PoseidonError> {
    if !is_in_field(left) || !is_in_field(right) {
        return Err(PoseidonError::NotInField);
    }
    let h = hashv(
        Parameters::Bn254X5,
        Endianness::BigEndian,
        &[left.as_slice(), right.as_slice()],
    )
    .map_err(|_| PoseidonError::HashFailed)?;
    Ok(h.to_bytes())
}
```

Add to `programs/pool-program/src/lib.rs` (after `use anchor_lang::prelude::*;`):
```rust
pub mod poseidon;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p pool-program poseidon`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add programs/pool-program/src/poseidon.rs programs/pool-program/src/lib.rs
git commit -m "feat(pool-program): BN254 Poseidon hash2 via native syscall with field checks"
```

---

### Task 3: Incremental Merkle tree module

**Files:**
- Create: `programs/pool-program/src/merkle.rs`
- Modify: `programs/pool-program/src/lib.rs` (add `pub mod merkle;`)

**Interfaces:**
- Consumes: `poseidon::{hash2, is_in_field, PoseidonError}`.
- Produces:
  - `pub const TREE_HEIGHT: usize = 20;`, `pub const ZERO_LEAF: [u8;32] = [0u8;32];`
  - `pub fn zeros() -> Result<[[u8;32]; TREE_HEIGHT], MerkleError>` — precomputed zero-subtree roots.
  - `pub fn empty_root(zeros: &[[u8;32]; TREE_HEIGHT]) -> Result<[u8;32], MerkleError>`
  - `pub fn insert(next_index: &mut u32, current_root: &mut [u8;32], filled_subtrees: &mut [[u8;32]; TREE_HEIGHT], leaf: [u8;32]) -> Result<u32, MerkleError>` (returns leaf index; **borrows fields, copies nothing large**)
  - `pub enum MerkleError { TreeFull, NotInField, Hash }`

> **Why field-reference APIs:** the `Pool` account is multi-KB and lives boxed on the heap. Passing `&mut` field references (rather than a `MerkleState` value) means the handler never copies the tree onto the 4096-byte SBF stack frame.

- [ ] **Step 1: Write the failing tests**

At the bottom of `programs/pool-program/src/merkle.rs`:
```rust
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
        assert_eq!(build(&leaves), build(&leaves), "tree is a deterministic function of its leaves");
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
        let (left, right) = if current_index % 2 == 0 {
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
  - `pub fn push(roots: &mut [[u8;32]; ROOT_HISTORY_SIZE], current_index: &mut u32, root: [u8;32])`
  - `pub fn is_known(roots: &[[u8;32]; ROOT_HISTORY_SIZE], root: &[u8;32]) -> bool`

- [ ] **Step 1: Write the failing tests**

At the bottom of `programs/pool-program/src/roots.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> ([[u8; 32]; ROOT_HISTORY_SIZE], u32) {
        ([[0u8; 32]; ROOT_HISTORY_SIZE], 0u32)
    }

    #[test]
    fn seed_root_is_known() {
        let (mut roots, _ci) = empty();
        roots[0] = [5u8; 32]; // non-zero seed (the real seed is the non-zero empty-tree root)
        assert!(is_known(&roots, &[5u8; 32]));
        assert!(!is_known(&roots, &[6u8; 32]));
    }

    #[test]
    fn pushed_root_becomes_known_and_recent_history_survives() {
        let (mut roots, mut ci) = empty();
        roots[0] = [9u8; 32]; // non-zero seed
        push(&mut roots, &mut ci, [1u8; 32]);
        assert!(is_known(&roots, &[1u8; 32]));
        assert!(is_known(&roots, &[9u8; 32]), "recent history still valid");
    }

    #[test]
    fn zero_is_never_a_known_root() {
        let (roots, _ci) = empty();
        assert!(!is_known(&roots, &[0u8; 32]), "the zero sentinel is never valid");
    }

    #[test]
    fn old_roots_evicted_after_ring_wraps() {
        let (mut roots, mut ci) = empty();
        roots[0] = [200u8; 32]; // distinct non-zero seed
        // push ROOT_HISTORY_SIZE fresh, distinct, non-zero roots so the seed falls out
        for n in 1..=(ROOT_HISTORY_SIZE as u8) {
            let mut r = [0u8; 32];
            r[0] = n; // 1..=100, all distinct and non-zero, distinct from the seed (200)
            push(&mut roots, &mut ci, r);
        }
        assert!(!is_known(&roots, &[200u8; 32]), "root older than the 100-slot window is rejected");
        let mut newest = [0u8; 32];
        newest[0] = ROOT_HISTORY_SIZE as u8;
        assert!(is_known(&roots, &newest));
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

/// Append a root to the ring (overwriting the oldest slot once full).
pub fn push(roots: &mut [[u8; 32]; ROOT_HISTORY_SIZE], current_index: &mut u32, root: [u8; 32]) {
    let next = (*current_index as usize + 1) % ROOT_HISTORY_SIZE;
    roots[next] = root;
    *current_index = next as u32;
}

/// A root is "known" iff it equals any non-empty slot in the ring.
pub fn is_known(roots: &[[u8; 32]; ROOT_HISTORY_SIZE], root: &[u8; 32]) -> bool {
    if *root == [0u8; 32] {
        return false; // the zero sentinel is never a valid root
    }
    roots.iter().any(|r| r == root)
}
```

Add to `programs/pool-program/src/lib.rs`:
```rust
pub mod roots;
```

- [ ] **Step 4: Run to verify passing**

Run: `cargo test -p pool-program roots`
Expected: PASS (4 tests).

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
- Create: `programs/pool-program/tests/common.rs` (shared test helpers)
- Create: `programs/pool-program/tests/initialize_pool.rs`

**Interfaces:**
- Consumes: `merkle::{zeros, empty_root, insert, TREE_HEIGHT, MerkleError}`, `roots::{push, ROOT_HISTORY_SIZE}`.
- Produces:
  - `#[account] pub struct Pool { mint, bump, vault_bump, next_index, current_root, filled_subtrees[TREE_HEIGHT], roots[ROOT_HISTORY_SIZE], current_root_index }` (**no `zeros` field** — computed on demand).
  - `Pool::SPACE`, `Pool::insert_commitment(&mut self, leaf) -> Result<u32, MerkleError>`, `Pool::push_root(&mut self, root)`.
  - instruction `initialize_pool(ctx)` seeded at `["pool", mint]`, vault at `["vault", pool]`; funds the vault to the rent-exempt minimum.
  - test helpers `program_id()`, `so_path()`, `disc(name)`, `cu_limit_ix()` in `tests/common.rs`.

- [ ] **Step 1: Write the state + in-place helpers**

`programs/pool-program/src/state.rs`:
```rust
use anchor_lang::prelude::*;
use crate::merkle::{self, MerkleError, TREE_HEIGHT};
use crate::roots::{self, ROOT_HISTORY_SIZE};

#[account]
pub struct Pool {
    pub mint: Pubkey,
    pub bump: u8,
    pub vault_bump: u8,
    pub next_index: u32,
    pub current_root: [u8; 32],
    pub filled_subtrees: [[u8; 32]; TREE_HEIGHT],
    pub roots: [[u8; 32]; ROOT_HISTORY_SIZE],
    pub current_root_index: u32,
}

impl Pool {
    // discriminator(8) + mint(32) + bump(1) + vault_bump(1) + next_index(4)
    // + current_root(32) + filled_subtrees(32*H) + roots(32*RING) + current_root_index(4)
    pub const SPACE: usize =
        8 + 32 + 1 + 1 + 4 + 32 + 32 * TREE_HEIGHT + 32 * ROOT_HISTORY_SIZE + 4;

    /// Insert a commitment into the embedded tree, mutating fields in place (no large copy).
    pub fn insert_commitment(&mut self, leaf: [u8; 32]) -> Result<u32, MerkleError> {
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
```

- [ ] **Step 2: Wire the instruction + errors into `lib.rs`**

Replace the body of `programs/pool-program/src/lib.rs` with:
```rust
use anchor_lang::prelude::*;
use anchor_lang::system_program;

pub mod poseidon;
pub mod merkle;
pub mod roots;
pub mod state;

use crate::merkle::{empty_root, zeros};
use crate::state::Pool;

// Overwritten by `anchor keys sync`.
declare_id!("11111111111111111111111111111111");

#[program]
pub mod pool_program {
    use super::*;

    pub fn initialize_pool(ctx: Context<InitializePool>) -> Result<()> {
        let z = zeros().map_err(|_| error!(PoolError::MerkleInit))?;
        let root = empty_root(&z).map_err(|_| error!(PoolError::MerkleInit))?;

        // Fund the vault to the rent-exempt minimum so custody funds are never at rent risk.
        let rent_min = Rent::get()?.minimum_balance(0);
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.payer.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                },
            ),
            rent_min,
        )?;

        // The `init` constraint zero-fills the account; set only the non-zero fields
        // (avoids materializing a multi-KB array on the stack).
        let pool = &mut ctx.accounts.pool;
        pool.mint = ctx.accounts.mint.key();
        pool.bump = ctx.bumps.pool;
        pool.vault_bump = ctx.bumps.vault;
        pool.filled_subtrees = z; // empty tree: filled subtrees == zeros
        pool.current_root = root;
        pool.roots[0] = root;
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
    pub pool: Box<Account<'info, Pool>>,

    /// CHECK: SOL vault PDA (system-owned); only holds lamports.
    #[account(
        mut,
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

- [ ] **Step 3: Write the shared test helpers**

`programs/pool-program/tests/common.rs`:
```rust
#![allow(dead_code)]
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, instruction::Instruction, pubkey::Pubkey,
};

/// The generated program ID (declare_id!), read from the crate under test.
pub fn program_id() -> Pubkey {
    pool_program::ID
}

/// Absolute path to the SBF artifact. `anchor build` writes to the WORKSPACE-root
/// target/, but `cargo test -p pool-program` runs with CWD = the package dir, so a
/// relative path fails. CARGO_MANIFEST_DIR = programs/pool-program → ../../target.
pub fn so_path() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/deploy/pool_program.so").to_string()
}

/// Anchor instruction discriminator = sha256("global:<name>")[..8].
pub fn disc(name: &str) -> [u8; 8] {
    use solana_sdk::hash::hash;
    let h = hash(format!("global:{name}").as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h.to_bytes()[..8]);
    d
}

/// Headroom for the ~20 Poseidon syscalls + multi-KB Borsh (de)serialization.
pub fn cu_limit_ix() -> Instruction {
    ComputeBudgetInstruction::set_compute_unit_limit(400_000)
}
```

- [ ] **Step 4: Write the failing LiteSVM test**

`programs/pool-program/tests/initialize_pool.rs`:
```rust
mod common;
use common::{cu_limit_ix, disc, program_id, so_path};
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount, instruction::{AccountMeta, Instruction},
    message::Message, pubkey::Pubkey, signature::{Keypair, Signer}, system_program,
    transaction::Transaction,
};

#[test]
fn initialize_pool_creates_account_with_nonzero_root() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());

    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false), // writable: receives rent-exempt funding
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: disc("initialize_pool").to_vec(),
    };
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, svm.latest_blockhash());
    let meta = svm.send_transaction(tx).unwrap();
    println!("initialize_pool CU consumed: {}", meta.compute_units_consumed);

    let acct = svm.get_account(&pool).unwrap();
    assert!(acct.data().len() > 8, "pool account allocated");
    // current_root at 8(disc)+32(mint)+1(bump)+1(vault_bump)+4(next_index) = 46..78
    let current_root = &acct.data()[46..78];
    assert_ne!(current_root, &[0u8; 32], "empty-tree root must be nonzero");
}
```

- [ ] **Step 5: Build then run the test**

Run: `anchor build && cargo test -p pool-program --test initialize_pool`
Expected: PASS (1 test). Note the printed CU figure; if it ever nears 400k, raise `cu_limit_ix`.

- [ ] **Step 6: Commit**

```bash
git add programs/pool-program/src/state.rs programs/pool-program/src/lib.rs programs/pool-program/tests/common.rs programs/pool-program/tests/initialize_pool.rs
git commit -m "feat(pool-program): Pool account (boxed) + initialize_pool + test helpers"
```

---

### Task 6: Vault custody + `deposit` instruction

**Files:**
- Modify: `programs/pool-program/src/lib.rs` (add `deposit` handler + accounts + event)
- Create: `programs/pool-program/tests/deposit.rs`

**Interfaces:**
- Consumes: `Pool::{insert_commitment, push_root}`, `poseidon::is_in_field`, `common` test helpers.
- Produces: instruction `deposit(ctx, commitment: [u8;32], amount: u64)` that (a) transfers `amount` lamports payer→vault, (b) inserts `commitment`, (c) pushes the new root, (d) emits `DepositEvent { commitment, leaf_index, new_root }`.

> **DEFERRED — denomination bucketing (spec §5, anti-fingerprinting):** this foundations `deposit` accepts an *arbitrary* `amount`. The spec requires deposits be constrained to **discretized denomination buckets** so amounts don't fingerprint users. That constraint is **intentionally not enforced here** and MUST be added in **Plan 4** (rounds), on-chain in `deposit` and/or at round formation.

- [ ] **Step 1: Add the handler, accounts, and event to `lib.rs`**

Inside `#[program] pub mod pool_program`, add:
```rust
    pub fn deposit(ctx: Context<Deposit>, commitment: [u8; 32], amount: u64) -> Result<()> {
        require!(amount > 0, PoolError::ZeroDeposit);
        require!(crate::poseidon::is_in_field(&commitment), PoolError::CommitmentNotInField);

        // (a) move lamports payer -> vault (vault is a system-owned PDA)
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.payer.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                },
            ),
            amount,
        )?;

        // (b) insert + (c) push root — mutate the boxed account in place, no large stack copy
        let leaf_index = ctx.accounts.pool.insert_commitment(commitment).map_err(|e| match e {
            crate::merkle::MerkleError::TreeFull => error!(PoolError::TreeFull),
            crate::merkle::MerkleError::NotInField => error!(PoolError::CommitmentNotInField),
            crate::merkle::MerkleError::Hash => error!(PoolError::MerkleInit),
        })?;
        let new_root = ctx.accounts.pool.current_root;
        ctx.accounts.pool.push_root(new_root);

        emit!(DepositEvent { commitment, leaf_index, new_root });
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
    pub pool: Box<Account<'info, Pool>>,

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
mod common;
use common::{cu_limit_ix, disc, program_id, so_path};
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount, instruction::{AccountMeta, Instruction},
    message::Message, pubkey::Pubkey, signature::{Keypair, Signer}, system_program,
    transaction::Transaction,
};

fn setup_pool() -> (LiteSVM, Keypair, Pubkey, Pubkey) {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());

    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: disc("initialize_pool").to_vec(),
    };
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    svm.send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash())).unwrap();
    (svm, payer, pool, vault)
}

fn deposit_ix(pool: Pubkey, vault: Pubkey, payer: Pubkey, commitment: [u8; 32], amount: u64) -> Instruction {
    let mut data = disc("deposit").to_vec();
    data.extend_from_slice(&commitment);
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction {
        program_id: program_id(),
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
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    let meta = svm
        .send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .unwrap();
    println!("deposit CU consumed: {}", meta.compute_units_consumed);

    let vault_after = svm.get_account(&vault).unwrap().lamports();
    assert_eq!(vault_after - vault_before, 1_000_000, "vault received the deposit");

    let data_after = svm.get_account(&pool).unwrap().data().to_vec();
    assert_ne!(&data_after[46..78], root_before.as_slice(), "root advanced after deposit");
    let next_index = u32::from_le_bytes(data_after[42..46].try_into().unwrap());
    assert_eq!(next_index, 1, "one leaf inserted");
}

#[test]
fn deposit_rejects_zero_amount() {
    let (mut svm, payer, pool, vault) = setup_pool();
    let commitment = { let mut c = [0u8; 32]; c[31] = 7; c };
    let ix = deposit_ix(pool, vault, payer.pubkey(), commitment, 0);
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    assert!(
        svm.send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash())).is_err(),
        "zero deposit must fail"
    );
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

### Task 7: Nullifier set module + `mark_spent` guard

**Files:**
- Create: `programs/pool-program/src/nullifier.rs`
- Modify: `programs/pool-program/src/lib.rs` (add `pub mod nullifier;` + `NullifierRecord` account + `mark_spent` handler)
- Create: `programs/pool-program/tests/nullifier.rs`

**Interfaces:**
- Consumes: Anchor account model, `common` test helpers.
- Produces:
  - `#[account] pub struct NullifierRecord { pub spent: bool }` at seeds `["nullifier", pool, nullifier_hash]`.
  - instruction `mark_spent(ctx, nullifier_hash: [u8;32])` — `init`s the record PDA (existence == spent); re-marking the same nullifier fails because `init` fails on an existing account.

> **Why a PDA-per-nullifier:** the PDA *existing* is the "spent" marker. `init` (not `init_if_needed`) atomically fails if it already exists, so double-spend protection is free and loophole-free (no close path in this plan).
>
> **DEFERRED — gating:** `mark_spent` is a **standalone** instruction here so the guard can be exercised in isolation. It is intentionally ungated (griefing is limited: nullifiers are secret until reveal, so an attacker cannot pre-burn a victim's specific one). Before ANY deployment, spending a nullifier MUST happen **inside `withdraw`, gated behind Groth16 proof verification** (the `circuits` + wire-ZK plans). A naked public `mark_spent` must not survive into a deployable build.

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
    pub pool: Box<Account<'info, Pool>>,

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
/// nullifier has been spent. `spent` is a readability aid — the security property
/// is the PDA's existence.
#[account]
pub struct NullifierRecord {
    pub spent: bool,
}
```

- [ ] **Step 3: Write the failing test (double-spend rejected)**

`programs/pool-program/tests/nullifier.rs`:
```rust
mod common;
use common::{cu_limit_ix, disc, program_id, so_path};
use litesvm::LiteSVM;
use solana_sdk::{
    instruction::{AccountMeta, Instruction}, message::Message, pubkey::Pubkey,
    signature::{Keypair, Signer}, system_program, transaction::Transaction,
};

fn setup_pool() -> (LiteSVM, Keypair, Pubkey) {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();
    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: disc("initialize_pool").to_vec(),
    };
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    svm.send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash())).unwrap();
    (svm, payer, pool)
}

fn mark_spent_tx(svm: &LiteSVM, payer: &Keypair, pool: Pubkey, nh: [u8; 32]) -> Transaction {
    let (nullifier, _) = Pubkey::find_program_address(
        &[b"nullifier", pool.as_ref(), nh.as_ref()], &program_id());
    let mut data = disc("mark_spent").to_vec();
    data.extend_from_slice(&nh);
    let ix = Instruction {
        program_id: program_id(),
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

    svm.send_transaction(mark_spent_tx(&svm, &payer, pool, nh)).unwrap();
    assert!(
        svm.send_transaction(mark_spent_tx(&svm, &payer, pool, nh)).is_err(),
        "re-spending the same nullifier must fail (PDA already exists)"
    );
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

A deployable Anchor `pool-program` with: pool initialization (vault funded rent-exempt), SOL vault custody, an incremental height-20 Poseidon Merkle commitment tree, a 100-entry root-history ring, and a PDA-based nullifier set with double-spend protection — all covered by Rust unit + LiteSVM integration tests, with the multi-KB account boxed and mutated in place to respect the SBF stack limit.

## Explicitly deferred to later plans

- **ZK proof verification** (`commit_intent` / `withdraw` verifying Groth16, spending nullifiers *inside* `withdraw`) — needs the `circuits` plan first. `mark_spent` is a temporary standalone guard and must be folded into `withdraw` before any deploy.
- **SPL-token pools** — this plan custodies native SOL only; the `mint` seed is a label. Token-2022 / SPL vaults come with the action-adapters.
- **Denomination bucketing** (spec §5, anti-fingerprinting) — `deposit` accepts arbitrary amounts here; the discretized-bucket constraint MUST be added in **Plan 4**.
- **Rounds, `k`-floor, `PooledAction`, incentives, viewing keys** — Phases 2–4.

## Self-review notes

- **Spec coverage (Phase-1-foundations slice):** pool init ✓ (T5), custody ✓ (T5 vault funding + T6), Merkle tree height-20 ✓ (T3), 100-root ring ✓ (T4), nullifier set ✓ (T7), Poseidon/field-range ✓ (T2). Proof verification intentionally out of scope.
- **Review fixes folded in:** generated program ID via `anchor keys sync` (no invalid vanity literal); `Box<Account<Pool>>` + in-place field mutation (SBF stack); field-reference module APIs + on-demand `zeros()` (no per-pool `zeros` storage); absolute `.so` path + `pool_program::ID` in a shared `tests/common.rs`; non-zero-seeded root-ring tests; reconciled `["vault", pool]` seed (spec §3.1 updated); unified compute-budget guidance (tests set 400k + log actual); vault funded rent-exempt at init; `mark_spent` gating called out.
- **Placeholder scan:** none — every step has concrete code/commands.
- **Type consistency:** `Pool::{insert_commitment, push_root}` match `merkle::insert` / `roots::push` field-reference signatures; `insert` returns `u32` consumed by `DepositEvent.leaf_index`; test byte offsets (42..46 next_index, 46..78 current_root) match `Pool` field order (dropping `zeros` did not move fields before `filled_subtrees`).
- **Verify at implementation time:** the `anchor_lang::solana_program::poseidon` API (`hashv`, `Parameters::Bn254X5`, `Endianness::BigEndian`, `PoseidonHash::to_bytes`) and any feature gate; `litesvm` 0.6 API (`add_program_from_file`, `send_transaction`, `TransactionMetadata::compute_units_consumed`); that `anchor keys sync` exists in the pinned Anchor version (else `anchor keys list` + manual `declare_id!`). **Confirm `Bn254X5` + zero-leaf `0` match the circom Poseidon(2) the later `circuits` plan uses.**
