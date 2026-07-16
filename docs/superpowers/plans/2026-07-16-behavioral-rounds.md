# Behavioral Rounds (on-chain core) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single-user `withdraw` path with a k-anonymous **round** engine — many participants `commit_intent`, the pool executes them as one vault-signed batch (`execute_round`) that an on-chain **k-floor** refuses to fire below `k`, with a coordinator-independent `cancel_intent` escape hatch — so exiting the pool is only ever possible inside a crowd of ≥ k.

**Architecture:** A `Round` PDA accumulates per-intent PDAs (`["intent", pool, nullifier_hash]`) until anyone calls `execute_round` with all ≥ k of them as `remaining_accounts`; the pool then pays every intent from its vault in a single signed transaction (the "uniform actor" property), enforcing `intent_count ≥ k_floor` on-chain. No circuit changes: the existing withdraw proof already binds `extDataHash(recipient, relayer, fee)`, so we verify it at **commit** time and store the intent instead of paying immediately. Extensibility is the sanctioned `PooledAction` trait with one `WithdrawAction` impl, dispatched by an `ActionKind` enum.

**Tech Stack:** Rust 2021, Anchor 0.31.1, `zero_copy` accounts (`AccountLoader`/bytemuck `Pod`), `solana-poseidon` (BN254), `groth16-solana` 0.2, LiteSVM 0.6.1 (Rust-native instruction tests), `ark-circom` 0.5 client proving (pinned — do NOT bump), `proptest`/host unit tests for invariants.

## Global Constraints

Every task's requirements implicitly include this section. Copy exact values verbatim.

- **Anchor 0.31.1 / Rust 2021.** `cargo fmt` + `cargo clippy --all-targets -- -D warnings` clean before every commit. `overflow-checks` stays on.
- **Conventional commits** (`feat:`, `fix:`, `test:`, `refactor:`, `docs:`, `chore:`), one logical change per commit.
- **Custody fail-closed.** On-chain paths return typed `PoolError`s. No `unwrap()`/`expect()`/`panic!` on attacker-influenced input. Use checked arithmetic / `require!`-guarded math for every amount, index, and count.
- **`k`-floor is enforced ON-CHAIN**, not just off-chain. `execute_round` MUST reject any batch with `intent_count < k_floor`. `MIN_K_FLOOR = 2` — a pool with `k_floor < 2` is rejected at `initialize_pool`.
- **Uniform actor.** All payouts of a round happen in ONE transaction signed only by the vault PDA (`invoke_signed`). No per-intent standalone payout instruction may exist.
- **No standalone `withdraw`.** The single-user `withdraw` instruction is removed; a k=1 exit would bypass the anonymity set. Exit is only via a round.
- **Never log, emit, or return secrets** — no note secret, nullifier preimage, or member→action mapping. Events carry only already-public data.
- **Front-run safety is preserved.** The payout accounts ARE the keys hashed into the proof's `extDataHash` (recipient/relayer), recorded in the `Intent` and paid verbatim at execute — never re-derived from separate args.
- **Invariant logic lives in pure `pub fn`s with host unit tests** (`cargo-llvm-cov` cannot see SBF in-VM lines): `meets_k_floor`, `split_payout`, and the `MerkleTree` path/root math.
- **Large accounts are `Box`ed / `zero_copy` and mutated in place** — never copy the ~3.9 KB `Pool` by value onto the 4 KB SBF stack. `Pool` stays `#[account(zero_copy)]`, `repr(C)`, no implicit padding (bytemuck `Pod` rejects it at compile time).
- **Tests are Rust-native**: host unit tests + LiteSVM. No TypeScript, no `solana-test-validator` in the inner loop. Adversarial/negative cases are mandatory for anything security-relevant (sub-k, replay, double-commit, cross-pool intent, fund redirection).
- **`TREE_HEIGHT = 20`, `ROOT_HISTORY_SIZE = 100`** (unchanged). `ark-circom` pinned at 0.5.
- **Error variants are APPEND-ONLY.** Anchor assigns `PoolError` codes from 6000 in
  declaration order, and existing tests hardcode them (`deposit.rs` → 6001/6002;
  `withdraw.rs` until Task 6 → 6005/6007). Every new variant (`KFloorTooLow`,
  `WrongRound`, `RoundClosed`, `RoundOverflow`, `KFloorNotMet`,
  `IntentAccountsMismatch`, `IntentInvalid`, `IntentAccountMismatch`,
  `DuplicateIntent`) MUST be appended after `FeeExceedsDenomination` — never
  inserted or reordered — or those hardcoded codes silently shift and break.
- A change isn't done until `cargo test -p <crate>` is green and you've said so with the output.

---

## File Structure

**Program (`programs/pool-program/src/`):**
- `state.rs` — *modify.* `Pool` gains `k_floor: u16` and `current_round_id: u64` (with explicit padding preserving the no-implicit-padding invariant).
- `round.rs` — *new.* Round/intent **data**: `RoundState` enum, `Round` account, `Intent` account, `ActionKind` enum, `MIN_K_FLOOR`.
- `invariants.rs` — *new.* Pure, host-tested security math: `meets_k_floor`, `split_payout`.
- `action.rs` — *new.* The `PooledAction` **behavior** seam: `PooledAction` trait + `WithdrawAction` impl (the two vault→recipient/relayer transfers).
- `lib.rs` — *modify.* `initialize_pool` (adds `k_floor` + creates `Round(0)`); new `commit_intent`, `execute_round`, `cancel_intent`; remove `withdraw` + `Withdraw` accounts; new error variants; `pub mod round/invariants/action`.
- `verifier.rs`, `nullifier.rs`, `merkle.rs`, `roots.rs`, `poseidon.rs`, `vk.rs` — *unchanged.*

**SDK (`crates/sdk/src/lib.rs`):** *modify.* Add `MerkleTree` builder (root + authentication path); `build_initialize_pool_ix` gains `k_floor` + round account; remove `build_withdraw_ix`, add `build_commit_intent_ix`, `build_execute_round_ix`, `build_cancel_intent_ix`.

**Tests:**
- `crates/sdk/src/lib.rs` `#[cfg(test)]` — *modify.* `MerkleTree` host tests (root parity + path-matches-committed-bundle).
- `programs/pool-program/tests/common.rs` — *modify.* Add round-PDA / disc helpers as needed (see tasks).
- `programs/pool-program/tests/round_support.rs` — *new.* Shared LiteSVM fixture: build a k-note tree via `sdk::MerkleTree`, deposit all leaves, prove each note; returns per-intent proof material.
- `programs/pool-program/tests/commit_intent.rs`, `execute_round.rs`, `cancel_intent.rs` — *new.*
- `programs/pool-program/tests/initialize_pool.rs`, `deposit.rs` — *modify* (k_floor arg + round account).
- `programs/pool-program/tests/withdraw.rs` — *delete* in Task 6 (superseded).
- `crates/sdk/tests/e2e.rs`, `crates/sdk/tests/sdk.rs` — *modify* in Task 6 (round flow; drop `build_withdraw_ix`).

**Interface names used across tasks (verbatim):**
- `Pool.k_floor: u16`, `Pool.current_round_id: u64`.
- `round::MIN_K_FLOOR: u16 = 2`, `round::RoundState::{Open, Executed}`, `round::Round { state, intent_count }`, `round::Round::SPACE`, `round::Intent { pool, round_id, recipient, relayer, fee, action }`, `round::Intent::SPACE`, `round::ActionKind::Withdraw`.
- `invariants::meets_k_floor(intent_count: u32, k_floor: u16) -> bool`, `invariants::split_payout(denomination: u64, fee: u64) -> Result<(u64, u64)>`.
- `action::PooledAction` (method `execute(&self) -> Result<()>`), `action::WithdrawAction`.
- Instructions: `initialize_pool(denomination: u64, k_floor: u16)`, `commit_intent(proof, root, nullifier_hash, fee, round_id)`, `execute_round(round_id: u64)`, `cancel_intent(round_id: u64, nullifier_hash: [u8;32])`.
- PDA seeds: pool `["pool", mint]`, vault `["vault", pool]`, round `["round", pool, round_id_le]`, intent `["intent", pool, nullifier_hash]`, nullifier `["nullifier", pool, nullifier_hash]`.
- SDK: `sdk::MerkleTree`, `sdk::MerklePath { elements, indices }`.

---

## Task 1: SDK `MerkleTree` builder (root + authentication path)

Client-side incremental Merkle tree matching `pool_program::merkle`: accumulate leaves, compute the current root, and produce any leaf's authentication path against that root. This is the enabler for k≥2 real-proof tests and closes a real SDK gap (today `build_withdraw_ix` demands a `MerklePath` the caller cannot compute). No program change.

**Files:**
- Modify: `crates/sdk/src/lib.rs`
- Test: `crates/sdk/src/lib.rs` `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `pool_program::merkle::{zeros, TREE_HEIGHT}`, `pool_program::poseidon::hash2`, existing `MerklePath { elements: [FieldBytes; TREE_DEPTH], indices: [u8; TREE_DEPTH] }`, `TREE_DEPTH`.
- Produces: `sdk::MerkleTree` with `new() -> Result<Self, SdkError>`, `insert(&mut self, leaf: [u8;32]) -> usize`, `root(&self) -> [u8;32]`, `authentication_path(&self, index: usize) -> MerklePath`. New `SdkError::MerkleHash`.

- [ ] **Step 1: Write the failing tests**

Add to `crates/sdk/src/lib.rs`'s `#[cfg(test)] mod tests` (the `feb`/`decode_be_hex` helpers reconstruct the committed bundle so the path is validated against what the circuit actually accepts):

```rust
    fn tf(n: u8) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[31] = n;
        b
    }

    fn tdecode_be_hex(s: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
        }
        out
    }

    #[test]
    fn merkle_root_matches_pool_program_incremental_insert() {
        // Same two leaves the committed bundle uses, same order.
        let decoy = poseidon::hash2(&tf(111), &tf(222)).unwrap();
        let note = poseidon::hash2(&tf(7), &tf(9)).unwrap();

        let mut tree = MerkleTree::new().unwrap();
        tree.insert(decoy);
        tree.insert(note);

        // Independent reference: the on-chain program's own incremental insert.
        let z = pool_program::merkle::zeros().unwrap();
        let mut next_index = 0u32;
        let mut root = pool_program::merkle::empty_root(&z).unwrap();
        let mut filled = z;
        pool_program::merkle::insert(&mut next_index, &mut root, &mut filled, decoy).unwrap();
        pool_program::merkle::insert(&mut next_index, &mut root, &mut filled, note).unwrap();

        assert_eq!(tree.root(), root, "SDK tree root must match on-chain incremental insert");
    }

    #[test]
    fn merkle_path_matches_committed_bundle() {
        // The committed circuit-validated bundle: reconstruct its 2-leaf tree and
        // assert the SDK computes the SAME root/path the circuit signed off on.
        let raw = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(std::path::Path::parent)
                .unwrap()
                .join("circuits/test/withdraw_vectors.json"),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();

        let decoy = poseidon::hash2(&tf(111), &tf(222)).unwrap();
        let note = poseidon::hash2(&tf(7), &tf(9)).unwrap();
        let mut tree = MerkleTree::new().unwrap();
        tree.insert(decoy);
        let note_index = tree.insert(note); // 1

        assert_eq!(tree.root(), tdecode_be_hex(v["root"].as_str().unwrap()), "root");
        let path = tree.authentication_path(note_index);
        let want_elems: Vec<[u8; 32]> = v["pathElements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| tdecode_be_hex(e.as_str().unwrap()))
            .collect();
        let want_idx: Vec<u8> = v["pathIndices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.as_u64().unwrap() as u8)
            .collect();
        assert_eq!(path.elements.to_vec(), want_elems, "pathElements must match circuit bundle");
        assert_eq!(path.indices.to_vec(), want_idx, "pathIndices must match circuit bundle");
    }
```

Add `serde_json` to `crates/sdk/Cargo.toml` `[dev-dependencies]` if absent:
```toml
serde_json = "1"
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sdk --lib merkle`
Expected: FAIL — `MerkleTree` not defined.

- [ ] **Step 3: Implement `MerkleTree`**

Add near the top of `crates/sdk/src/lib.rs` (after the `SdkError` enum add the `MerkleHash` variant):

```rust
#[derive(Debug, PartialEq, Eq)]
pub enum SdkError {
    /// A note field is not a canonical, in-field BN254 scalar.
    NotInField,
    /// The Poseidon hash used to derive an empty-subtree constant failed.
    MerkleHash,
}
```

Then add the builder:

```rust
/// Client-side incremental Merkle tree mirroring `pool_program::merkle`
/// (`TREE_HEIGHT` levels, Poseidon2 nodes, empty positions filled with the
/// same zero-subtree constants). Lets a client compute its note's
/// authentication path from the leaves it has scanned — the private
/// `MerklePath` inputs `prover::prove_withdraw` needs.
pub struct MerkleTree {
    leaves: Vec<[u8; 32]>,
    zeros: [[u8; 32]; TREE_DEPTH],
}

impl MerkleTree {
    pub fn new() -> Result<Self, SdkError> {
        let zeros = pool_program::merkle::zeros().map_err(|_| SdkError::MerkleHash)?;
        Ok(Self { leaves: Vec::new(), zeros })
    }

    /// Append a commitment; returns its leaf index.
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
        level.first().copied().unwrap_or_else(|| {
            pool_program::merkle::empty_root(&self.zeros).expect("empty_root")
        })
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
```

Ensure `pool_program::merkle::empty_root` and `zeros` are reachable (they are `pub`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p sdk --lib merkle`
Expected: PASS (both tests). Then `cargo test -p sdk` to confirm nothing else regressed.

- [ ] **Step 5: Lint + commit**

```bash
cargo fmt
cargo clippy -p sdk --all-targets -- -D warnings
git add crates/sdk/src/lib.rs crates/sdk/Cargo.toml
git commit -m "feat(sdk): incremental MerkleTree builder (root + authentication path)"
```

---

## Task 2: Pool round-state fields, invariants, and `initialize_pool(k_floor)` + `Round(0)`

Extend `Pool` with `k_floor`/`current_round_id`, add the pure invariant functions, define the round/intent data types, and make `initialize_pool` take `k_floor` (rejecting `< MIN_K_FLOOR`) and create the pool's first `Round`. Sweep every `initialize_pool` caller so the whole workspace stays green.

**Files:**
- Modify: `programs/pool-program/src/state.rs`, `programs/pool-program/src/lib.rs`
- Create: `programs/pool-program/src/round.rs`, `programs/pool-program/src/invariants.rs`
- Modify (callers): `crates/sdk/src/lib.rs` (`build_initialize_pool_ix` + its unit test), `programs/pool-program/tests/initialize_pool.rs`, `programs/pool-program/tests/deposit.rs`, `programs/pool-program/tests/withdraw.rs`, `crates/sdk/tests/e2e.rs`, `crates/sdk/tests/sdk.rs`
- Test: `programs/pool-program/src/invariants.rs` `#[cfg(test)]`, `programs/pool-program/tests/initialize_pool.rs`

**Interfaces:**
- Consumes: existing `Pool` layout (`state.rs`), `PoolError`, `merkle::{zeros, empty_root}`.
- Produces: `Pool.k_floor`, `Pool.current_round_id`; `round::{MIN_K_FLOOR, RoundState, Round, Intent, ActionKind}`; `invariants::{meets_k_floor, split_payout}`; `initialize_pool(denomination: u64, k_floor: u16)` creating `Round(0)`; `InitializePool` context with a `round` account at position 3.

- [ ] **Step 1: Write the failing host tests for invariants**

Create `programs/pool-program/src/invariants.rs`:

```rust
use crate::PoolError;
use anchor_lang::prelude::*;

/// The k-floor: a round may only execute when it holds at least `k_floor`
/// intents. This is THE behavioral-anonymity invariant, enforced on-chain.
pub fn meets_k_floor(intent_count: u32, k_floor: u16) -> bool {
    intent_count >= k_floor as u32
}

/// Split a denomination into `(payout_to_recipient, fee_to_relayer)`, failing
/// closed if the fee exceeds the denomination (never underflows).
pub fn split_payout(denomination: u64, fee: u64) -> Result<(u64, u64)> {
    require!(fee <= denomination, PoolError::FeeExceedsDenomination);
    let payout = denomination
        .checked_sub(fee)
        .ok_or(error!(PoolError::FeeExceedsDenomination))?;
    Ok((payout, fee))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k_floor_boundary() {
        assert!(!meets_k_floor(1, 2), "below floor rejected");
        assert!(meets_k_floor(2, 2), "exactly floor accepted");
        assert!(meets_k_floor(3, 2), "above floor accepted");
        assert!(!meets_k_floor(0, 2), "empty round rejected");
    }

    #[test]
    fn split_payout_conserves_value() {
        assert_eq!(split_payout(1_000, 10).unwrap(), (990, 10));
        assert_eq!(split_payout(1_000, 0).unwrap(), (1_000, 0));
        assert_eq!(split_payout(1_000, 1_000).unwrap(), (0, 1_000));
    }

    #[test]
    fn split_payout_rejects_fee_over_denomination() {
        assert!(split_payout(1_000, 1_001).is_err(), "fee > denomination fails closed");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p pool-program --lib invariants`
Expected: FAIL — module `invariants` not declared / not found.

- [ ] **Step 3: Declare the modules and define round data types**

In `programs/pool-program/src/lib.rs`, add module declarations alongside the existing ones:

```rust
pub mod action;
pub mod invariants;
pub mod round;
```

Create `programs/pool-program/src/round.rs`:

```rust
use anchor_lang::prelude::*;

/// A pool with `k_floor < 2` provides no anonymity; reject it at init.
pub const MIN_K_FLOOR: u16 = 2;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoundState {
    Open,
    Executed,
}

/// One accumulation window. `intent_count` is the authoritative number of
/// live intents committed to this round (incremented at commit, decremented
/// at cancel); `execute_round` checks it against the k-floor.
#[account]
pub struct Round {
    pub state: RoundState,
    pub intent_count: u32,
}

impl Round {
    pub const SPACE: usize = 8 + 1 + 4;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActionKind {
    Withdraw,
}

/// A committed intent: the note is already spent (its nullifier PDA exists);
/// `recipient`/`relayer` were bound into the proof via `extDataHash`, so
/// `execute_round` pays exactly these keys. `pool`/`round_id` bind the intent
/// to its pool and round, closing cross-pool / cross-round reuse.
#[account]
pub struct Intent {
    pub pool: Pubkey,
    pub round_id: u64,
    pub recipient: Pubkey,
    pub relayer: Pubkey,
    pub fee: u64,
    pub action: ActionKind,
}

impl Intent {
    pub const SPACE: usize = 8 + 32 + 8 + 32 + 32 + 8 + 1;
}
```

- [ ] **Step 4: Run invariant tests to verify they pass**

Run: `cargo test -p pool-program --lib invariants`
Expected: PASS (3 tests).

- [ ] **Step 5: Extend `Pool` with `k_floor` + `current_round_id`**

In `programs/pool-program/src/state.rs`, replace the trailing `current_root_index` / `_reserved2` fields so the new fields land with explicit (never implicit) padding. The struct alignment is already 8 (from `denomination: u64`); this layout keeps size a multiple of 8 with no implicit gap:

```rust
    pub current_root_index: u32,
    // k-floor and the current open round id. `k_floor` (u16) sits right after
    // the u32 (offset stays 2-aligned); an explicit 2-byte pad then 8-aligns
    // `current_round_id` (u64). Every byte of padding is named so bytemuck's
    // `Pod` derive — which rejects *implicit* padding — stays satisfied.
    pub k_floor: u16,
    _reserved2: [u8; 2],
    pub current_round_id: u64,
}
```

(Remove the old `_reserved2: [u8; 4]`. The `assert!(core::mem::size_of::<Pool>().is_multiple_of(8))` const below the struct must still hold — the new tail is `u32(4) + u16(2) + pad(2) + u64(8) = 16` bytes, size grows by 8 to 3936, still a multiple of 8.)

- [ ] **Step 6: Add `k_floor` + `Round(0)` to `initialize_pool` and the error variant**

In `programs/pool-program/src/lib.rs`, change the handler signature and body:

```rust
    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        denomination: u64,
        k_floor: u16,
    ) -> Result<()> {
        require!(k_floor >= crate::round::MIN_K_FLOOR, PoolError::KFloorTooLow);

        let z = zeros().map_err(|_| error!(PoolError::MerkleInit))?;
        let root = empty_root(&z).map_err(|_| error!(PoolError::MerkleInit))?;

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

        {
            let mut pool = ctx.accounts.pool.load_init()?;
            pool.mint = ctx.accounts.mint.key();
            pool.denomination = denomination;
            pool.k_floor = k_floor;
            pool.current_round_id = 0;
            pool.bump = ctx.bumps.pool;
            pool.vault_bump = ctx.bumps.vault;
            pool.filled_subtrees = z;
            pool.current_root = root;
            pool.roots[0] = root;
        }

        let round = &mut ctx.accounts.round;
        round.state = crate::round::RoundState::Open;
        round.intent_count = 0;
        Ok(())
    }
```

Add the `round` account to `InitializePool` (position 3, right after `vault`):

```rust
    #[account(
        init,
        payer = payer,
        space = crate::round::Round::SPACE,
        seeds = [b"round", pool.key().as_ref(), &0u64.to_le_bytes()],
        bump
    )]
    pub round: Account<'info, crate::round::Round>,
```

Add the error variant to `PoolError`:

```rust
    #[msg("k_floor must be at least MIN_K_FLOOR")]
    KFloorTooLow,
```

- [ ] **Step 7: Sweep every `initialize_pool` caller**

The signature (adds `k_floor`) and account list (adds `round` at index 3) changed. Update:

**`crates/sdk/src/lib.rs` — `build_initialize_pool_ix`:**
```rust
pub fn build_initialize_pool_ix(
    pool: Pubkey,
    vault: Pubkey,
    round: Pubkey,
    mint: Pubkey,
    payer: Pubkey,
    denomination: u64,
    k_floor: u16,
) -> Instruction {
    let mut data = discriminator("initialize_pool").to_vec();
    data.extend_from_slice(&denomination.to_le_bytes());
    data.extend_from_slice(&k_floor.to_le_bytes());
    Instruction {
        program_id: pool_program::ID,
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(round, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    }
}
```
Add a helper for the round-0 PDA (used by the sweep + later tasks):
```rust
/// The PDA for a pool's round `round_id` (`["round", pool, round_id_le]`).
pub fn round_pda(pool: Pubkey, round_id: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &round_id.to_le_bytes()],
        &pool_program::ID,
    )
    .0
}
```
Update the `initialize_pool_ix_encodes_denomination` unit test in the same file to pass a `round` pubkey + `k_floor`, and assert `ix.accounts.len() == 6` and `&ix.data[16..18] == &k_floor.to_le_bytes()`.

**`programs/pool-program/tests/initialize_pool.rs`:** in the hand-built ix, insert `AccountMeta::new(round, false)` after `vault` (derive `let (round, _) = Pubkey::find_program_address(&[b"round", pool.as_ref(), &0u64.to_le_bytes()], &program_id());`) and append `d.extend_from_slice(&2u16.to_le_bytes());` after the denomination bytes.

**`programs/pool-program/tests/deposit.rs` (`setup_pool`) and `programs/pool-program/tests/withdraw.rs` (`setup_pool`):** same two edits — add the round account meta after `vault`, append `&2u16.to_le_bytes()` after the denomination in the `initialize_pool` data. (Change the `setup_pool` fn to derive the round PDA and include it.)

**`crates/sdk/tests/e2e.rs` and `crates/sdk/tests/sdk.rs`:** update the `build_initialize_pool_ix(...)` call sites to pass `sdk::round_pda(pool, 0)` and a `k_floor` of `2` (add the round param in the correct position).

- [ ] **Step 8: Add the `Round(0)` assertion to `initialize_pool.rs`**

Append to `programs/pool-program/tests/initialize_pool.rs`:

```rust
#[test]
fn initialize_pool_opens_round_zero() {
    use pool_program::round::{Round, RoundState};
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round, _) =
        Pubkey::find_program_address(&[b"round", pool.as_ref(), &0u64.to_le_bytes()], &program_id());

    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(round, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: {
            let mut d = disc("initialize_pool").to_vec();
            d.extend_from_slice(&1_000_000u64.to_le_bytes());
            d.extend_from_slice(&2u16.to_le_bytes());
            d
        },
    };
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    svm.send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .unwrap();

    let acct = svm.get_account(&round).unwrap();
    let parsed = Round::try_deserialize(&mut acct.data()).unwrap();
    assert_eq!(parsed.state, RoundState::Open, "round 0 opens");
    assert_eq!(parsed.intent_count, 0, "round 0 starts empty");
}
```
Add `use anchor_lang::AccountDeserialize;` and (if not already imported) the account-data import at the top of the test file.

- [ ] **Step 9: Reject-below-floor assertion**

Also append a negative test asserting `initialize_pool` with `k_floor = 1` fails (custom error `KFloorTooLow`). Compute its code: `PoolError` variants are numbered from 6000 in declaration order; `KFloorTooLow` is appended last, so its code is `6000 + (index)`. Rather than hardcode, assert it is an `InstructionError::Custom(_)` and that the logs contain `"KFloorTooLow"`:

```rust
#[test]
fn initialize_pool_rejects_k_floor_below_min() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round, _) =
        Pubkey::find_program_address(&[b"round", pool.as_ref(), &0u64.to_le_bytes()], &program_id());

    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(round, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: {
            let mut d = disc("initialize_pool").to_vec();
            d.extend_from_slice(&1_000_000u64.to_le_bytes());
            d.extend_from_slice(&1u16.to_le_bytes());
            d
        },
    };
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    let outcome = svm
        .send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .expect_err("k_floor below MIN_K_FLOOR must be rejected");
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("KFloorTooLow")),
        "expected KFloorTooLow; logs: {:?}",
        outcome.meta.logs
    );
}
```

- [ ] **Step 10: Run the affected suites**

Run: `anchor build` (rebuild the `.so` — the account layout + ix changed), then
`cargo test -p pool-program` and `cargo test -p sdk`.
Expected: all green (existing deposit/withdraw/initialize tests updated for the new signature, new Round(0) + reject-below-floor tests pass, SDK unit tests pass). Note: `cargo test -p pool-program` requires `circuits/build/*` artifacts for the withdraw fixture — the fixture regenerates them via `setup.sh` if missing.

- [ ] **Step 11: Lint + commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
git add programs/pool-program/src crates/sdk/src/lib.rs programs/pool-program/tests crates/sdk/tests
git commit -m "feat(pool-program): pool round-state fields, k-floor invariants, initialize_pool opens Round(0)"
```

---

## Task 3: `commit_intent`

The front half of the old `withdraw`, minus payout: verify the proof against a known root, atomically burn the nullifier (single-commit), record the `Intent` under the current open round, and increment `intent_count`. Introduces the shared LiteSVM round fixture (used by Tasks 4–5).

**Files:**
- Modify: `programs/pool-program/src/lib.rs`
- Create: `programs/pool-program/tests/round_support.rs`, `programs/pool-program/tests/commit_intent.rs`
- Test: `programs/pool-program/tests/commit_intent.rs`

**Interfaces:**
- Consumes: `verifier::{WithdrawProof, verify_withdraw}`, `ext_data::ext_data_hash`, `roots::is_known`, `nullifier::NullifierRecord`, `round::{Round, RoundState, Intent, ActionKind}`, `Pool`.
- Produces: `commit_intent(proof, root, nullifier_hash, fee, round_id)`, `CommitIntent` accounts, error variants `WrongRound`, `RoundClosed`, `RoundOverflow`. Fixture `round_support::build_round_fixture(k_floor, n) -> RoundFixture` and `round_support::commit_intent_tx(...)`.

- [ ] **Step 1: Write the shared fixture**

Create `programs/pool-program/tests/round_support.rs`. It initializes a pool, deposits `n` fresh notes, and generates a real Groth16 proof per note against the common final root using `sdk::MerkleTree`. (This is the multi-proof generator that makes k≥2 tests honest.)

```rust
#![allow(dead_code, deprecated)]
mod common;
pub use common::{disc, program_id, so_path};

use litesvm::LiteSVM;
use pool_program::verifier::WithdrawProof;
use sdk::{MerkleTree, Note};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::Transaction,
};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const DENOMINATION: u64 = 2_000_000;
pub const FEE: u64 = 1_000;

pub struct IntentMaterial {
    pub note: Note,
    pub proof: WithdrawProof,
    pub root: [u8; 32],
    pub nullifier_hash: [u8; 32],
    pub recipient: Pubkey,
    pub relayer: Pubkey,
    pub fee: u64,
    pub intent_pda: Pubkey,
    pub nullifier_pda: Pubkey,
}

pub struct RoundFixture {
    pub svm: LiteSVM,
    pub payer: Keypair,
    pub pool: Pubkey,
    pub vault: Pubkey,
    pub k_floor: u16,
    pub intents: Vec<IntentMaterial>,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("two levels below workspace root")
        .to_path_buf()
}

/// Mirrors the other tests' build guard: the `circuits/build/*` artifacts are
/// gitignored outputs of `circuits/scripts/setup.sh`; generate them if absent
/// rather than skip the real prove/verify.
fn ensure_build_artifacts() -> PathBuf {
    static BUILD_DIR: OnceLock<PathBuf> = OnceLock::new();
    BUILD_DIR
        .get_or_init(|| {
            let circuits_dir = workspace_root().join("circuits");
            let build_dir = circuits_dir.join("build");
            let required = [
                build_dir.join("withdraw_js").join("withdraw.wasm"),
                build_dir.join("withdraw.r1cs"),
                build_dir.join("withdraw.zkey"),
                build_dir.join("verification_key.json"),
            ];
            if !required.iter().all(|p| p.exists()) {
                let status = std::process::Command::new("bash")
                    .arg(circuits_dir.join("scripts").join("setup.sh"))
                    .status();
                let ok = matches!(status, Ok(s) if s.success());
                if !ok || !required.iter().all(|p| p.exists()) {
                    panic!(
                        "circuits/build artifacts missing and setup.sh did not produce them \
                         (needs circom + npx/snarkjs on PATH)."
                    );
                }
            }
            build_dir
        })
        .clone()
}

fn send(svm: &mut LiteSVM, payer: &Keypair, signers: &[&Keypair], ix: Instruction) {
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&payer.pubkey()),
    );
    svm.send_transaction(Transaction::new(signers, msg, svm.latest_blockhash()))
        .unwrap();
}

/// Initialize a pool with `k_floor`, deposit `n` fresh notes, and build a real
/// proof for each against the common final root.
pub fn build_round_fixture(k_floor: u16, n: usize) -> RoundFixture {
    let build_dir = ensure_build_artifacts();

    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round0, _) =
        Pubkey::find_program_address(&[b"round", pool.as_ref(), &0u64.to_le_bytes()], &program_id());

    // initialize_pool(DENOMINATION, k_floor)
    let mut data = disc("initialize_pool").to_vec();
    data.extend_from_slice(&DENOMINATION.to_le_bytes());
    data.extend_from_slice(&k_floor.to_le_bytes());
    send(
        &mut svm,
        &payer,
        &[&payer],
        Instruction {
            program_id: program_id(),
            accounts: vec![
                AccountMeta::new(pool, false),
                AccountMeta::new(vault, false),
                AccountMeta::new(round0, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            data,
        },
    );

    // Build the client tree + deposit each commitment on-chain (roots agree by
    // the MerkleTree<->pool_program parity proven in Task 1).
    let notes: Vec<Note> = (0..n).map(|_| Note::new()).collect();
    let mut tree = MerkleTree::new().unwrap();
    for note in &notes {
        let commitment = note.commitment();
        tree.insert(commitment);
        let mut d = disc("deposit").to_vec();
        d.extend_from_slice(&commitment);
        d.extend_from_slice(&DENOMINATION.to_le_bytes());
        send(
            &mut svm,
            &payer,
            &[&payer],
            Instruction {
                program_id: program_id(),
                accounts: vec![
                    AccountMeta::new(pool, false),
                    AccountMeta::new(vault, false),
                    AccountMeta::new(payer.pubkey(), true),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: d,
            },
        );
    }

    let root = tree.root();
    let mut intents = Vec::with_capacity(n);
    for (i, note) in notes.iter().enumerate() {
        let recipient = Pubkey::new_unique();
        let relayer = Pubkey::new_unique();
        let path = tree.authentication_path(i);
        let ext = sdk::compute_ext_data_hash(&recipient.to_bytes(), &relayer.to_bytes(), FEE);
        let inputs = sdk::WithdrawInputs {
            root,
            nullifier_hash: note.nullifier_hash(),
            ext_data_hash: ext,
            nullifier: note.nullifier(),
            secret: note.secret(),
            path_elements: path.elements,
            path_indices: path.indices,
        };
        let (proof, public_inputs) = prover::prove_withdraw(
            build_dir.join("withdraw_js").join("withdraw.wasm"),
            build_dir.join("withdraw.r1cs"),
            build_dir.join("withdraw.zkey"),
            &inputs,
        )
        .expect("proving a fresh note must succeed");
        let withdraw_proof = WithdrawProof {
            a: prover::proof_a_to_solana_be(&proof.a).unwrap(),
            b: prover::g2_to_solana_be(&proof.b).unwrap(),
            c: prover::g1_to_solana_be(&proof.c).unwrap(),
        };
        let (intent_pda, _) = Pubkey::find_program_address(
            &[b"intent", pool.as_ref(), public_inputs.nullifier_hash.as_ref()],
            &program_id(),
        );
        let (nullifier_pda, _) = Pubkey::find_program_address(
            &[b"nullifier", pool.as_ref(), public_inputs.nullifier_hash.as_ref()],
            &program_id(),
        );
        intents.push(IntentMaterial {
            note: *note,
            proof: withdraw_proof,
            root: public_inputs.root,
            nullifier_hash: public_inputs.nullifier_hash,
            recipient,
            relayer,
            fee: FEE,
            intent_pda,
            nullifier_pda,
        });
    }

    RoundFixture { svm, payer, pool, vault, k_floor, intents }
}

/// Build a `commit_intent` transaction for intent `i` against round `round_id`.
pub fn commit_intent_tx(fx: &RoundFixture, i: usize, round_id: u64) -> Transaction {
    let m = &fx.intents[i];
    let (round, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &round_id.to_le_bytes()],
        &program_id(),
    );
    let mut data = disc("commit_intent").to_vec();
    data.extend_from_slice(&m.proof.a);
    data.extend_from_slice(&m.proof.b);
    data.extend_from_slice(&m.proof.c);
    data.extend_from_slice(&m.root);
    data.extend_from_slice(&m.nullifier_hash);
    data.extend_from_slice(&m.fee.to_le_bytes());
    data.extend_from_slice(&round_id.to_le_bytes());
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new_readonly(fx.pool, false),
            AccountMeta::new(round, false),
            AccountMeta::new(m.intent_pda, false),
            AccountMeta::new(m.nullifier_pda, false),
            AccountMeta::new_readonly(m.recipient, false),
            AccountMeta::new_readonly(m.relayer, false),
            AccountMeta::new(fx.payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&fx.payer.pubkey()),
    );
    Transaction::new(&[&fx.payer], msg, fx.svm.latest_blockhash())
}
```

Ensure `crates/sdk` and `prover` are `dev-dependencies` of `pool-program` (they already are — `withdraw.rs` uses `prover`; add `sdk` to `programs/pool-program/Cargo.toml` `[dev-dependencies]` if absent: `sdk = { path = "../../crates/sdk" }`).

- [ ] **Step 2: Write the failing `commit_intent` test**

Create `programs/pool-program/tests/commit_intent.rs`:

```rust
#![allow(deprecated)]
mod round_support;
use round_support::{build_round_fixture, commit_intent_tx, program_id};

use pool_program::round::{Intent, Round, RoundState};
use solana_sdk::{
    account::ReadableAccount,
    instruction::{AccountMeta, Instruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    signature::Signer,
    transaction::{Transaction, TransactionError},
    compute_budget::ComputeBudgetInstruction,
    system_program,
};
use anchor_lang::AccountDeserialize;

#[test]
fn commit_intent_records_intent_and_burns_nullifier() {
    let mut fx = build_round_fixture(2, 1);
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    let tx = commit_intent_tx(&fx, 0, 0);
    fx.svm.send_transaction(tx).expect("valid commit must succeed");

    // Intent recorded with the bound payout keys, under round 0.
    let m0_recipient = fx.intents[0].recipient;
    let m0_relayer = fx.intents[0].relayer;
    let intent_acct = fx.svm.get_account(&fx.intents[0].intent_pda).unwrap();
    let intent = Intent::try_deserialize(&mut intent_acct.data()).unwrap();
    assert_eq!(intent.pool, fx.pool);
    assert_eq!(intent.round_id, 0);
    assert_eq!(intent.recipient, m0_recipient);
    assert_eq!(intent.relayer, m0_relayer);

    // Round count incremented; nullifier PDA now exists.
    let round_acct = fx.svm.get_account(&round0).unwrap();
    let round = Round::try_deserialize(&mut round_acct.data()).unwrap();
    assert_eq!(round.state, RoundState::Open);
    assert_eq!(round.intent_count, 1);
    assert!(fx.svm.get_account(&fx.intents[0].nullifier_pda).is_some());
}

#[test]
fn commit_intent_rejects_double_commit() {
    let mut fx = build_round_fixture(2, 1);
    fx.svm.send_transaction(commit_intent_tx(&fx, 0, 0)).unwrap();
    fx.svm.expire_blockhash();
    let outcome = fx
        .svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .expect_err("re-committing the same nullifier must fail");
    assert_ne!(outcome.err, TransactionError::AlreadyProcessed);
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("already in use")),
        "nullifier/intent PDA init must reject the second commit; logs: {:?}",
        outcome.meta.logs
    );
}

#[test]
fn commit_intent_rejects_unknown_root() {
    let mut fx = build_round_fixture(2, 1);
    // Corrupt the root in the tx data (byte at the proof-length offset).
    let m = &fx.intents[0];
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );
    let mut bad_root = m.root;
    bad_root[0] ^= 0x01;
    let mut data = round_support::disc("commit_intent").to_vec();
    data.extend_from_slice(&m.proof.a);
    data.extend_from_slice(&m.proof.b);
    data.extend_from_slice(&m.proof.c);
    data.extend_from_slice(&bad_root);
    data.extend_from_slice(&m.nullifier_hash);
    data.extend_from_slice(&m.fee.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new_readonly(fx.pool, false),
            AccountMeta::new(round0, false),
            AccountMeta::new(m.intent_pda, false),
            AccountMeta::new(m.nullifier_pda, false),
            AccountMeta::new_readonly(m.recipient, false),
            AccountMeta::new_readonly(m.relayer, false),
            AccountMeta::new(fx.payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&fx.payer.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(&[&fx.payer], msg, fx.svm.latest_blockhash()))
        .expect_err("unknown root must fail");
    // M4: assert the SPECIFIC guard fired (UnknownRoot), not just "some error" —
    // `UnknownRoot` stays a stable variant (new errors are appended, never
    // reordered — see Global Constraints), so a log-substring check is non-tautological.
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("UnknownRoot")),
        "expected UnknownRoot; logs: {:?}",
        outcome.meta.logs
    );
}

// I4: `fee > denomination` is a reachable value guard on the commit path (it
// fires BEFORE proof verification), and must fail closed. Reuse intent 0's real
// proof but set an out-of-range fee in the instruction data.
#[test]
fn commit_intent_rejects_fee_over_denomination() {
    let mut fx = build_round_fixture(2, 1);
    let m = &fx.intents[0];
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );
    let bad_fee = round_support::DENOMINATION + 1;
    let mut data = round_support::disc("commit_intent").to_vec();
    data.extend_from_slice(&m.proof.a);
    data.extend_from_slice(&m.proof.b);
    data.extend_from_slice(&m.proof.c);
    data.extend_from_slice(&m.root);
    data.extend_from_slice(&m.nullifier_hash);
    data.extend_from_slice(&bad_fee.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new_readonly(fx.pool, false),
            AccountMeta::new(round0, false),
            AccountMeta::new(m.intent_pda, false),
            AccountMeta::new(m.nullifier_pda, false),
            AccountMeta::new_readonly(m.recipient, false),
            AccountMeta::new_readonly(m.relayer, false),
            AccountMeta::new(fx.payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&fx.payer.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(&[&fx.payer], msg, fx.svm.latest_blockhash()))
        .expect_err("fee exceeding the denomination must fail closed");
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("FeeExceedsDenomination")),
        "expected FeeExceedsDenomination; logs: {:?}",
        outcome.meta.logs
    );
}
```
(Expose `disc` from `round_support` — it already re-exports `common::disc`.)

**Also add to `execute_round.rs` (Task 4) a `commit_to_executed_round_rejects` test** covering the reachable `WrongRound` guard on the commit path (the one round-guard that survives after I3): build `build_round_fixture(2, 3)`, commit intents 0 and 1 to round 0, run `execute_round(0)`, then `commit_intent_tx(&fx, 2, 0)` — committing note 2 to the now-**executed** round 0 while `current_round_id == 1`. Assert the logs contain `"WrongRound"` (round_id 0 != current 1; fires before any PDA init). This is the only place `WrongRound` is reachable, so it belongs with the executed-round setup that `execute_round.rs` already has.

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p pool-program --test commit_intent`
Expected: FAIL — `commit_intent` instruction not defined (discriminator resolves to nothing / program panics on unknown ix).

- [ ] **Step 4: Implement `commit_intent`**

Add to the `#[program]` module in `programs/pool-program/src/lib.rs`:

```rust
    pub fn commit_intent(
        ctx: Context<CommitIntent>,
        proof: crate::verifier::WithdrawProof,
        root: [u8; 32],
        nullifier_hash: [u8; 32],
        fee: u64,
        round_id: u64,
    ) -> Result<()> {
        {
            let pool = ctx.accounts.pool.load()?;
            require!(round_id == pool.current_round_id, PoolError::WrongRound);
            require!(crate::roots::is_known(&pool.roots, &root), PoolError::UnknownRoot);
            require!(fee <= pool.denomination, PoolError::FeeExceedsDenomination);
        }
        require!(
            ctx.accounts.round.state == crate::round::RoundState::Open,
            PoolError::RoundClosed
        );

        // extDataHash is computed from the recorded payout KEYS (the accounts
        // whose pubkeys `execute_round` pays), so the proof binds exactly the
        // keys stored in the Intent — no redirection possible.
        let ext = ext_data::ext_data_hash(
            &ctx.accounts.recipient.key().to_bytes(),
            &ctx.accounts.relayer.key().to_bytes(),
            fee,
        );
        crate::verifier::verify_withdraw(&proof, &[root, nullifier_hash, ext])?;

        // The nullifier PDA's `init` already enforced single-commit atomically.
        ctx.accounts.nullifier.spent = true;

        let intent = &mut ctx.accounts.intent;
        intent.pool = ctx.accounts.pool.key();
        intent.round_id = round_id;
        intent.recipient = ctx.accounts.recipient.key();
        intent.relayer = ctx.accounts.relayer.key();
        intent.fee = fee;
        intent.action = crate::round::ActionKind::Withdraw;

        let round = &mut ctx.accounts.round;
        round.intent_count = round
            .intent_count
            .checked_add(1)
            .ok_or(error!(PoolError::RoundOverflow))?;
        Ok(())
    }
```

Add the `CommitIntent` accounts struct (after `Deposit`):

```rust
#[derive(Accounts)]
#[instruction(proof: crate::verifier::WithdrawProof, root: [u8; 32], nullifier_hash: [u8; 32], fee: u64, round_id: u64)]
pub struct CommitIntent<'info> {
    #[account(
        seeds = [b"pool", pool.load()?.mint.as_ref()],
        bump = pool.load()?.bump
    )]
    pub pool: AccountLoader<'info, Pool>,

    #[account(
        mut,
        seeds = [b"round", pool.key().as_ref(), &round_id.to_le_bytes()],
        bump
    )]
    pub round: Account<'info, crate::round::Round>,

    #[account(
        init,
        payer = payer,
        space = crate::round::Intent::SPACE,
        seeds = [b"intent", pool.key().as_ref(), nullifier_hash.as_ref()],
        bump
    )]
    pub intent: Account<'info, crate::round::Intent>,

    #[account(
        init,
        payer = payer,
        space = 8 + 1,
        seeds = [b"nullifier", pool.key().as_ref(), nullifier_hash.as_ref()],
        bump
    )]
    pub nullifier: Account<'info, crate::nullifier::NullifierRecord>,

    /// CHECK: payout recipient; bound into the proof via extDataHash, recorded in the Intent.
    pub recipient: SystemAccount<'info>,
    /// CHECK: relayer; bound into the proof via extDataHash, recorded in the Intent.
    pub relayer: SystemAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}
```

Add error variants to `PoolError`:

```rust
    #[msg("round_id does not match the pool's current round")]
    WrongRound,
    #[msg("round is not open")]
    RoundClosed,
    #[msg("round intent count overflow")]
    RoundOverflow,
```

- [ ] **Step 5: Run to verify it passes**

Run: `anchor build` then `cargo test -p pool-program --test commit_intent`
Expected: PASS (3 tests). Proving runs the real Groth16 prover, so this is slower than a pure-logic test.

- [ ] **Step 6: Lint + commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
git add programs/pool-program/src/lib.rs programs/pool-program/tests/round_support.rs programs/pool-program/tests/commit_intent.rs programs/pool-program/Cargo.toml
git commit -m "feat(pool-program): commit_intent (verify + burn nullifier + record intent under open round)"
```

---

## Task 4: `execute_round` + `PooledAction` seam (the k-anonymous batch)

The differentiator: enforce the k-floor on-chain, then pay every intent from the vault in ONE signed transaction (uniform actor), dispatching each through the `PooledAction` seam. Bind each intent to this pool and round, require the complete set with no duplicates, and open the next round.

**Files:**
- Create: `programs/pool-program/src/action.rs`
- Modify: `programs/pool-program/src/lib.rs`
- Create: `programs/pool-program/tests/execute_round.rs`
- Test: `programs/pool-program/tests/execute_round.rs`

**Interfaces:**
- Consumes: `invariants::{meets_k_floor, split_payout}`, `round::{Round, RoundState, Intent, ActionKind}`, `Pool`, fixture `round_support`.
- Produces: `action::{PooledAction, WithdrawAction}`; `execute_round(round_id: u64)`, `ExecuteRound` accounts (with `remaining_accounts = [intent, recipient, relayer] × intent_count`), error variants `KFloorNotMet`, `IntentAccountsMismatch`, `IntentInvalid`, `IntentAccountMismatch`, `DuplicateIntent`.

- [ ] **Step 1: Write the `PooledAction` seam**

Create `programs/pool-program/src/action.rs`:

```rust
use anchor_lang::prelude::*;
use anchor_lang::system_program;

/// The one sanctioned extension seam (CLAUDE.md): every pooled action, when a
/// round executes, produces its effect through `execute`. Adding a protocol =
/// one new impl + one new `ActionKind` variant + one dispatch arm in
/// `execute_round`. Action-specific validation happens at commit time (the ZK
/// proof), so `execute` only performs the effect.
pub trait PooledAction {
    fn execute(&self) -> Result<()>;
}

/// Pay a single withdraw intent from the vault: `denomination - fee` to the
/// recipient, `fee` to the relayer, both signed by the vault PDA.
pub struct WithdrawAction<'a, 'info> {
    pub vault: AccountInfo<'info>,
    pub recipient: AccountInfo<'info>,
    pub relayer: AccountInfo<'info>,
    pub system_program: AccountInfo<'info>,
    pub signer_seeds: &'a [&'a [&'a [u8]]],
    pub denomination: u64,
    pub fee: u64,
}

impl PooledAction for WithdrawAction<'_, '_> {
    fn execute(&self) -> Result<()> {
        let (payout, fee) = crate::invariants::split_payout(self.denomination, self.fee)?;
        if payout > 0 {
            system_program::transfer(
                CpiContext::new_with_signer(
                    self.system_program.clone(),
                    system_program::Transfer {
                        from: self.vault.clone(),
                        to: self.recipient.clone(),
                    },
                    self.signer_seeds,
                ),
                payout,
            )?;
        }
        if fee > 0 {
            system_program::transfer(
                CpiContext::new_with_signer(
                    self.system_program.clone(),
                    system_program::Transfer {
                        from: self.vault.clone(),
                        to: self.relayer.clone(),
                    },
                    self.signer_seeds,
                ),
                fee,
            )?;
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Write the failing `execute_round` test**

Create `programs/pool-program/tests/execute_round.rs`:

```rust
#![allow(deprecated)]
mod round_support;
use round_support::{build_round_fixture, commit_intent_tx, program_id, DENOMINATION, FEE};

use pool_program::round::{Round, RoundState};
use solana_sdk::{
    account::ReadableAccount,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::{Transaction, TransactionError},
};
use anchor_lang::AccountDeserialize;

fn execute_round_ix(
    fx: &round_support::RoundFixture,
    round_id: u64,
    cranker: Pubkey,
    intent_triples: &[(Pubkey, Pubkey, Pubkey)], // (intent, recipient, relayer)
) -> Instruction {
    let (round, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &round_id.to_le_bytes()],
        &program_id(),
    );
    let (next_round, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &(round_id + 1).to_le_bytes()],
        &program_id(),
    );
    let mut accounts = vec![
        AccountMeta::new(fx.pool, false),
        AccountMeta::new(round, false),
        AccountMeta::new(next_round, false),
        AccountMeta::new(fx.vault, false),
        AccountMeta::new(cranker, true),
        AccountMeta::new_readonly(system_program::ID, false),
    ];
    for (intent, recipient, relayer) in intent_triples {
        accounts.push(AccountMeta::new(*intent, false));
        accounts.push(AccountMeta::new(*recipient, false));
        accounts.push(AccountMeta::new(*relayer, false));
    }
    let mut data = round_support::disc("execute_round").to_vec();
    data.extend_from_slice(&round_id.to_le_bytes());
    Instruction { program_id: program_id(), accounts, data }
}

#[test]
fn execute_round_pays_the_batch_and_enforces_k_floor() {
    // k_floor = 2, two committed intents.
    let mut fx = build_round_fixture(2, 2);
    fx.svm.send_transaction(commit_intent_tx(&fx, 0, 0)).unwrap();
    fx.svm.expire_blockhash();
    fx.svm.send_transaction(commit_intent_tx(&fx, 1, 0)).unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();

    let triples: Vec<(Pubkey, Pubkey, Pubkey)> = fx
        .intents
        .iter()
        .map(|m| (m.intent_pda, m.recipient, m.relayer))
        .collect();

    let vault_before = fx.svm.get_account(&fx.vault).unwrap().lamports();
    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&cranker.pubkey()),
    );
    let meta = fx
        .svm
        .send_transaction(Transaction::new(&[&cranker], msg, fx.svm.latest_blockhash()))
        .expect("a full k-round must execute");
    println!("execute_round CU consumed: {}", meta.compute_units_consumed);

    // Every recipient/relayer paid; vault debited exactly k * denomination.
    for m in &fx.intents {
        assert_eq!(
            fx.svm.get_account(&m.recipient).unwrap().lamports(),
            DENOMINATION - FEE,
            "recipient paid denomination - fee"
        );
        assert_eq!(
            fx.svm.get_account(&m.relayer).unwrap().lamports(),
            FEE,
            "relayer paid fee"
        );
    }
    let vault_after = fx.svm.get_account(&fx.vault).unwrap().lamports();
    assert_eq!(
        vault_before - vault_after,
        DENOMINATION * fx.intents.len() as u64,
        "vault debited exactly k * denomination (value conserved)"
    );

    // Round closed; next round opened.
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );
    let (round1, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &1u64.to_le_bytes()],
        &program_id(),
    );
    let r0 = Round::try_deserialize(&mut fx.svm.get_account(&round0).unwrap().data()).unwrap();
    let r1 = Round::try_deserialize(&mut fx.svm.get_account(&round1).unwrap().data()).unwrap();
    assert_eq!(r0.state, RoundState::Executed);
    assert_eq!(r1.state, RoundState::Open);
    assert_eq!(r1.intent_count, 0);

    // Re-executing the same (now Executed) round must fail.
    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&cranker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(&[&cranker], msg, fx.svm.latest_blockhash()))
        .expect_err("re-executing an Executed round must fail");
    assert!(matches!(
        outcome.err,
        TransactionError::InstructionError(_, InstructionError::Custom(_))
    ));
}

#[test]
fn execute_round_rejects_sub_k() {
    // k_floor = 2, only one committed intent.
    let mut fx = build_round_fixture(2, 2);
    fx.svm.send_transaction(commit_intent_tx(&fx, 0, 0)).unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    let triples = vec![(
        fx.intents[0].intent_pda,
        fx.intents[0].recipient,
        fx.intents[0].relayer,
    )];

    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&cranker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(&[&cranker], msg, fx.svm.latest_blockhash()))
        .expect_err("a sub-k round must not fire");
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("KFloorNotMet")),
        "expected KFloorNotMet; logs: {:?}",
        outcome.meta.logs
    );
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p pool-program --test execute_round`
Expected: FAIL — `execute_round` not defined.

- [ ] **Step 4: Implement `execute_round`**

Add `pub mod action;` is already declared (Task 2). Add the handler to `#[program]` in `lib.rs`:

```rust
    pub fn execute_round(ctx: Context<ExecuteRound>, round_id: u64) -> Result<()> {
        let (denomination, vault_bump, k_floor, current_round_id) = {
            let pool = ctx.accounts.pool.load()?;
            (pool.denomination, pool.vault_bump, pool.k_floor, pool.current_round_id)
        };
        // Re-execution and stale/future round_id are impossible by construction —
        // an explicit `round.state`/`round_id` check here would be UNREACHABLE dead
        // code (CLAUDE.md forbids it), because the account constraints ARE the guard:
        //   * `next_round` is `init` at seeds ["round", pool, round_id+1]; once this
        //     round has executed, round_id+1 already exists → its init fails "already
        //     in use" atomically, so a round executes at most once.
        //   * a future/non-existent `round_id` fails Anchor's `round` account load
        //     before the handler body runs.
        // Rounds are created strictly sequentially (Round(0) at init, Round(N+1) at
        // execute(N)), so the ONLY reachable path here has `round_id == current_round_id`;
        // the `current_round_id + 1` bump below is therefore correct.
        let count = ctx.accounts.round.intent_count;
        require!(
            crate::invariants::meets_k_floor(count, k_floor),
            PoolError::KFloorNotMet
        );

        let rem = ctx.remaining_accounts;
        require!(
            rem.len() == (count as usize) * 3,
            PoolError::IntentAccountsMismatch
        );

        let pool_key = ctx.accounts.pool.key();
        let vault_bump_arr = [vault_bump];
        let seeds: &[&[u8]] = &[b"vault", pool_key.as_ref(), &vault_bump_arr];
        let signer_seeds: &[&[&[u8]]] = &[seeds];

        let mut seen: Vec<Pubkey> = Vec::with_capacity(count as usize);
        for i in 0..(count as usize) {
            let intent_ai = &rem[i * 3];
            let recipient_ai = &rem[i * 3 + 1];
            let relayer_ai = &rem[i * 3 + 2];

            // Owner + discriminator checked by `try_from`; `pool`/`round_id`
            // bind it to THIS pool and round (closes cross-pool / cross-round
            // reuse); uniqueness closes duplicate-padding.
            let intent: Account<crate::round::Intent> =
                Account::try_from(intent_ai).map_err(|_| error!(PoolError::IntentInvalid))?;
            require_keys_eq!(intent.pool, pool_key, PoolError::IntentInvalid);
            require!(intent.round_id == round_id, PoolError::IntentInvalid);
            require!(!seen.contains(intent_ai.key), PoolError::DuplicateIntent);
            seen.push(*intent_ai.key);
            require_keys_eq!(*recipient_ai.key, intent.recipient, PoolError::IntentAccountMismatch);
            require_keys_eq!(*relayer_ai.key, intent.relayer, PoolError::IntentAccountMismatch);

            let action = crate::action::WithdrawAction {
                vault: ctx.accounts.vault.to_account_info(),
                recipient: recipient_ai.clone(),
                relayer: relayer_ai.clone(),
                system_program: ctx.accounts.system_program.to_account_info(),
                signer_seeds,
                denomination,
                fee: intent.fee,
            };
            match intent.action {
                crate::round::ActionKind::Withdraw => {
                    crate::action::PooledAction::execute(&action)?;
                }
            }
        }

        ctx.accounts.round.state = crate::round::RoundState::Executed;
        {
            let mut pool = ctx.accounts.pool.load_mut()?;
            pool.current_round_id = current_round_id
                .checked_add(1)
                .ok_or(error!(PoolError::RoundOverflow))?;
        }
        let next = &mut ctx.accounts.next_round;
        next.state = crate::round::RoundState::Open;
        next.intent_count = 0;
        Ok(())
    }
```

Add the `ExecuteRound` accounts struct:

```rust
#[derive(Accounts)]
#[instruction(round_id: u64)]
pub struct ExecuteRound<'info> {
    #[account(
        mut,
        seeds = [b"pool", pool.load()?.mint.as_ref()],
        bump = pool.load()?.bump
    )]
    pub pool: AccountLoader<'info, Pool>,

    #[account(
        mut,
        seeds = [b"round", pool.key().as_ref(), &round_id.to_le_bytes()],
        bump
    )]
    pub round: Account<'info, crate::round::Round>,

    #[account(
        init,
        payer = cranker,
        space = crate::round::Round::SPACE,
        seeds = [b"round", pool.key().as_ref(), &(round_id + 1).to_le_bytes()],
        bump
    )]
    pub next_round: Account<'info, crate::round::Round>,

    /// CHECK: SOL vault PDA (system-owned); pays out the batch via invoke_signed.
    #[account(
        mut,
        seeds = [b"vault", pool.key().as_ref()],
        bump = pool.load()?.vault_bump
    )]
    pub vault: UncheckedAccount<'info>,

    #[account(mut)]
    pub cranker: Signer<'info>,
    pub system_program: Program<'info, System>,
    // remaining_accounts: [intent, recipient, relayer] × intent_count
}
```

Add error variants to `PoolError`:

```rust
    #[msg("round has fewer intents than the k-floor")]
    KFloorNotMet,
    #[msg("wrong number of intent accounts for this round")]
    IntentAccountsMismatch,
    #[msg("intent account does not belong to this pool/round")]
    IntentInvalid,
    #[msg("payout account does not match the recorded intent")]
    IntentAccountMismatch,
    #[msg("duplicate intent account in the batch")]
    DuplicateIntent,
```

- [ ] **Step 5: Run to verify it passes**

Run: `anchor build` then `cargo test -p pool-program --test execute_round`
Expected: PASS (2 tests). Print line shows CU consumed for a k=2 batch.

- [ ] **Step 6: Add adversarial coverage (duplicate-padding + cross-pool)**

Append two negative tests to `execute_round.rs`:

```rust
#[test]
fn execute_round_rejects_duplicate_padding() {
    // k_floor = 2, ONE real intent duplicated to fake a full round.
    let mut fx = build_round_fixture(2, 2);
    fx.svm.send_transaction(commit_intent_tx(&fx, 0, 0)).unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    // Force intent_count to 2 by committing a second real intent, then pass the
    // FIRST intent twice (subset padded with a duplicate).
    fx.svm.expire_blockhash();
    fx.svm.send_transaction(commit_intent_tx(&fx, 1, 0)).unwrap();
    let dup = (fx.intents[0].intent_pda, fx.intents[0].recipient, fx.intents[0].relayer);
    let triples = vec![dup, dup];

    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&cranker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(&[&cranker], msg, fx.svm.latest_blockhash()))
        .expect_err("a duplicated intent must be rejected");
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("DuplicateIntent")),
        "expected DuplicateIntent; logs: {:?}",
        outcome.meta.logs
    );
}

// I1 (custody-critical): the fund-redirection guard. Present the CORRECT intent
// PDA but a SUBSTITUTED recipient account — the payout must be refused, proving
// `execute_round` pays only the extDataHash-bound keys stored in the Intent.
#[test]
fn execute_round_rejects_redirected_payout() {
    let mut fx = build_round_fixture(2, 2);
    fx.svm.send_transaction(commit_intent_tx(&fx, 0, 0)).unwrap();
    fx.svm.expire_blockhash();
    fx.svm.send_transaction(commit_intent_tx(&fx, 1, 0)).unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();

    let attacker = Pubkey::new_unique();
    let triples = vec![
        // intent 0's real PDA, but the attacker's account swapped in as recipient.
        (fx.intents[0].intent_pda, attacker, fx.intents[0].relayer),
        (fx.intents[1].intent_pda, fx.intents[1].recipient, fx.intents[1].relayer),
    ];
    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&cranker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(&[&cranker], msg, fx.svm.latest_blockhash()))
        .expect_err("a substituted payout account must be rejected");
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("IntentAccountMismatch")),
        "expected IntentAccountMismatch; logs: {:?}",
        outcome.meta.logs
    );
}

// I2: the cross-pool binding guard `require_keys_eq!(intent.pool, pool_key)`.
// A random pubkey would only trip `Account::try_from` (owner/discriminator), NOT
// this check. To drive it, craft a REAL, program-owned `Intent` (valid
// discriminator) whose `pool` field is some OTHER pool, inject it via
// `set_account`, and present it — only the `intent.pool` check can reject it.
#[test]
fn execute_round_rejects_intent_from_another_pool() {
    use anchor_lang::AccountSerialize;
    use pool_program::round::{ActionKind, Intent};
    use solana_sdk::account::Account;

    let mut fx = build_round_fixture(2, 2);
    fx.svm.send_transaction(commit_intent_tx(&fx, 0, 0)).unwrap();
    fx.svm.expire_blockhash();
    fx.svm.send_transaction(commit_intent_tx(&fx, 1, 0)).unwrap();

    // A program-owned Intent that belongs to a DIFFERENT pool but is otherwise
    // well-formed (correct discriminator, round_id matches this round).
    let other_recipient = Pubkey::new_unique();
    let other_relayer = Pubkey::new_unique();
    let foreign = Intent {
        pool: Pubkey::new_unique(), // NOT fx.pool
        round_id: 0,
        recipient: other_recipient,
        relayer: other_relayer,
        fee: FEE,
        action: ActionKind::Withdraw,
    };
    let mut data = Vec::new();
    foreign.try_serialize(&mut data).unwrap(); // writes discriminator + fields
    let foreign_addr = Pubkey::new_unique();
    fx.svm
        .set_account(
            foreign_addr,
            Account {
                lamports: 10_000_000,
                data,
                owner: program_id(), // program-owned so try_from's owner check passes
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    // Replace intent 1 with the foreign-pool intent (unique addr, so no dup).
    let triples = vec![
        (fx.intents[0].intent_pda, fx.intents[0].recipient, fx.intents[0].relayer),
        (foreign_addr, other_recipient, other_relayer),
    ];
    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&cranker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(&[&cranker], msg, fx.svm.latest_blockhash()))
        .expect_err("an intent bound to another pool must be rejected");
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("IntentInvalid")),
        "expected IntentInvalid (intent.pool != pool); logs: {:?}",
        outcome.meta.logs
    );
}

// I4: the `remaining_accounts.len() == intent_count * 3` completeness check.
// count meets the k-floor, but the batch is missing an intent's accounts.
#[test]
fn execute_round_rejects_incomplete_account_set() {
    let mut fx = build_round_fixture(2, 2);
    fx.svm.send_transaction(commit_intent_tx(&fx, 0, 0)).unwrap();
    fx.svm.expire_blockhash();
    fx.svm.send_transaction(commit_intent_tx(&fx, 1, 0)).unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    // count == 2 (meets k), but pass only ONE triple → len 3 != 6.
    let triples = vec![(
        fx.intents[0].intent_pda,
        fx.intents[0].recipient,
        fx.intents[0].relayer,
    )];
    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&cranker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(&[&cranker], msg, fx.svm.latest_blockhash()))
        .expect_err("an incomplete intent-account set must be rejected");
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("IntentAccountsMismatch")),
        "expected IntentAccountsMismatch; logs: {:?}",
        outcome.meta.logs
    );
}
```

Run: `cargo test -p pool-program --test execute_round`
Expected: PASS (6 tests: happy+re-exec, sub-k, duplicate-padding, redirect, cross-pool, incomplete-set).

Also correct the re-execution assertion comment inside `execute_round_pays_the_batch_and_enforces_k_floor` (Task 4 Step 2): the second `execute_round(0)` fails because **`next_round` (round 1) already exists so its `init` fails "already in use"** — NOT because of a `RoundClosed`/`WrongRound` handler check (those were removed as unreachable, see Step 4). Update that comment and assert the log contains `"already in use"`.

- [ ] **Step 7: Lint + commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
git add programs/pool-program/src/action.rs programs/pool-program/src/lib.rs programs/pool-program/tests/execute_round.rs
git commit -m "feat(pool-program): execute_round — on-chain k-floor + vault-signed batch via PooledAction seam"
```

---

## Task 5: `cancel_intent` (coordinator-independent escape hatch)

While a round is still Open, the intent's bound recipient (proving control by signing) reclaims the note's denomination and removes the intent from the round. The nullifier stays burned, so the note is never reusable — funds return without a double-spend.

**Files:**
- Modify: `programs/pool-program/src/lib.rs`
- Create: `programs/pool-program/tests/cancel_intent.rs`
- Test: `programs/pool-program/tests/cancel_intent.rs`

**Interfaces:**
- Consumes: `round::{Round, RoundState, Intent}`, `Pool`, fixture `round_support`.
- Produces: `cancel_intent(round_id: u64, nullifier_hash: [u8;32])`, `CancelIntent` accounts.

- [ ] **Step 1: Write the failing test**

Create `programs/pool-program/tests/cancel_intent.rs`. Note the fixture's `recipient` is a random `Pubkey` with no keypair; for cancel the recipient must **sign**, so this test commits with a recipient we control. Add a fixture variant call by committing normally, but override: the simplest path is to build the intent with a signer recipient. To keep the fixture unchanged, this test re-derives its own intent with a keypair recipient by committing through `commit_intent_tx` after swapping the recipient — instead, add a small helper to `round_support` (Step 1a) that builds a fixture whose recipients are keypairs.

Step 1a — add to `round_support.rs`:

```rust
/// Like `build_round_fixture` but each intent's recipient is a keypair we
/// control (needed by `cancel_intent`, where the recipient must sign). Returns
/// the fixture plus the recipient keypairs (index-aligned with `intents`).
pub fn build_round_fixture_signer_recipients(
    k_floor: u16,
    n: usize,
) -> (RoundFixture, Vec<Keypair>) {
    // Same as build_round_fixture, but generate recipient keypairs and use
    // their pubkeys as the bound recipient. (Copy the body of
    // build_round_fixture, replacing `let recipient = Pubkey::new_unique();`
    // with a Keypair and collecting the keypairs.)
    unimplemented!("see Step 1a instructions")
}
```

Implement it by copying `build_round_fixture`'s body and changing the per-intent recipient to `let recipient_kp = Keypair::new(); let recipient = recipient_kp.pubkey();`, pushing `recipient_kp` into a `Vec<Keypair>` returned alongside the fixture. Airdrop is unnecessary (the recipient receives lamports; it needs no starting balance to sign — LiteSVM lets zero-lamport accounts sign, but to be safe airdrop `1_000_000` to each recipient after building).

Then the test:

```rust
#![allow(deprecated)]
mod round_support;
use round_support::{build_round_fixture_signer_recipients, commit_intent_tx, program_id, DENOMINATION};

use pool_program::round::{Round, RoundState};
use solana_sdk::{
    account::ReadableAccount,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::{Transaction, TransactionError},
};
use anchor_lang::AccountDeserialize;

fn cancel_ix(
    fx: &round_support::RoundFixture,
    i: usize,
    round_id: u64,
    recipient: Pubkey,
) -> Instruction {
    let m = &fx.intents[i];
    let (round, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &round_id.to_le_bytes()],
        &program_id(),
    );
    let mut data = round_support::disc("cancel_intent").to_vec();
    data.extend_from_slice(&round_id.to_le_bytes());
    data.extend_from_slice(&m.nullifier_hash);
    Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new_readonly(fx.pool, false),
            AccountMeta::new(round, false),
            AccountMeta::new(m.intent_pda, false),
            AccountMeta::new(fx.vault, false),
            AccountMeta::new(recipient, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    }
}

#[test]
fn cancel_intent_refunds_and_decrements() {
    let (mut fx, recipients) = build_round_fixture_signer_recipients(2, 1);
    fx.svm.send_transaction(commit_intent_tx(&fx, 0, 0)).unwrap();

    let recipient = &recipients[0];
    let before = fx.svm.get_account(&recipient.pubkey()).map(|a| a.lamports()).unwrap_or(0);

    fx.svm.expire_blockhash();
    let ix = cancel_ix(&fx, 0, 0, recipient.pubkey());
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&fx.payer.pubkey()),
    );
    fx.svm
        .send_transaction(Transaction::new(&[&fx.payer, recipient], msg, fx.svm.latest_blockhash()))
        .expect("recipient may cancel an open-round intent");

    // Refunded denomination (+ closed intent rent), intent PDA gone, count back to 0.
    let after = fx.svm.get_account(&recipient.pubkey()).unwrap().lamports();
    assert!(after >= before + DENOMINATION, "recipient refunded at least the denomination");
    assert!(fx.svm.get_account(&fx.intents[0].intent_pda).is_none(), "intent PDA closed");

    let (round0, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );
    let r0 = Round::try_deserialize(&mut fx.svm.get_account(&round0).unwrap().data()).unwrap();
    assert_eq!(r0.intent_count, 0, "intent_count decremented");

    // Nullifier stays burned: re-committing the same note must fail.
    fx.svm.expire_blockhash();
    let outcome = fx
        .svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .expect_err("cancelled note stays spent (nullifier not returned)");
    assert!(outcome.meta.logs.iter().any(|l| l.contains("already in use")));
}

#[test]
fn cancel_intent_rejects_wrong_signer() {
    let (mut fx, _recipients) = build_round_fixture_signer_recipients(2, 1);
    fx.svm.send_transaction(commit_intent_tx(&fx, 0, 0)).unwrap();

    // An attacker who does NOT control the bound recipient cannot cancel.
    let attacker = Keypair::new();
    fx.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    fx.svm.expire_blockhash();
    let ix = cancel_ix(&fx, 0, 0, attacker.pubkey());
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&attacker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(&[&attacker], msg, fx.svm.latest_blockhash()))
        .expect_err("only the bound recipient may cancel");
    assert!(matches!(
        outcome.err,
        TransactionError::InstructionError(_, InstructionError::Custom(_))
    ));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p pool-program --test cancel_intent`
Expected: FAIL — `cancel_intent` not defined.

- [ ] **Step 3: Implement `cancel_intent`**

Add the handler to `#[program]` in `lib.rs`:

```rust
    pub fn cancel_intent(
        ctx: Context<CancelIntent>,
        _round_id: u64,
        _nullifier_hash: [u8; 32],
    ) -> Result<()> {
        require!(
            ctx.accounts.round.state == crate::round::RoundState::Open,
            PoolError::RoundClosed
        );

        let (denomination, vault_bump) = {
            let pool = ctx.accounts.pool.load()?;
            (pool.denomination, pool.vault_bump)
        };

        let pool_key = ctx.accounts.pool.key();
        let vault_bump_arr = [vault_bump];
        let seeds: &[&[u8]] = &[b"vault", pool_key.as_ref(), &vault_bump_arr];
        let signer_seeds: &[&[&[u8]]] = &[seeds];

        // Return the note's deposited value to its bound recipient. The
        // nullifier PDA is intentionally NOT closed — the note stays spent, so
        // there is no double-spend; the intent PDA is closed by the `close`
        // constraint (rent back to the recipient).
        system_program::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.recipient.to_account_info(),
                },
                signer_seeds,
            ),
            denomination,
        )?;

        let round = &mut ctx.accounts.round;
        round.intent_count = round
            .intent_count
            .checked_sub(1)
            .ok_or(error!(PoolError::RoundOverflow))?;
        Ok(())
    }
```

Add the `CancelIntent` accounts struct:

```rust
#[derive(Accounts)]
#[instruction(round_id: u64, nullifier_hash: [u8; 32])]
pub struct CancelIntent<'info> {
    #[account(
        seeds = [b"pool", pool.load()?.mint.as_ref()],
        bump = pool.load()?.bump
    )]
    pub pool: AccountLoader<'info, Pool>,

    #[account(
        mut,
        seeds = [b"round", pool.key().as_ref(), &round_id.to_le_bytes()],
        bump
    )]
    pub round: Account<'info, crate::round::Round>,

    // `close = recipient` returns the intent's rent and, with `has_one`,
    // proves the caller controls the bound recipient key (non-griefable).
    #[account(
        mut,
        close = recipient,
        seeds = [b"intent", pool.key().as_ref(), nullifier_hash.as_ref()],
        bump,
        constraint = intent.pool == pool.key() @ PoolError::IntentInvalid,
        constraint = intent.round_id == round_id @ PoolError::IntentInvalid,
        has_one = recipient @ PoolError::IntentAccountMismatch
    )]
    pub intent: Account<'info, crate::round::Intent>,

    /// CHECK: SOL vault PDA (system-owned); refunds the denomination via invoke_signed.
    #[account(
        mut,
        seeds = [b"vault", pool.key().as_ref()],
        bump = pool.load()?.vault_bump
    )]
    pub vault: UncheckedAccount<'info>,

    #[account(mut)]
    pub recipient: Signer<'info>,

    pub system_program: Program<'info, System>,
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `anchor build` then `cargo test -p pool-program --test cancel_intent`
Expected: PASS (2 tests).

- [ ] **Step 5: Lint + commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
git add programs/pool-program/src/lib.rs programs/pool-program/tests/round_support.rs programs/pool-program/tests/cancel_intent.rs
git commit -m "feat(pool-program): cancel_intent — recipient-authorized escape hatch for open rounds"
```

---

## Task 6: SDK round builders, e2e rewrite, and remove standalone `withdraw`

Give clients the round instructions, prove the full deposit→commit(k)→execute round trip through the SDK, and delete the single-user `withdraw` path (instruction, accounts, and its tests) — a k=1 exit would bypass the anonymity set.

**Files:**
- Modify: `crates/sdk/src/lib.rs` (remove `build_withdraw_ix` + `WithdrawBuild` + `WithdrawArtifacts` if now unused elsewhere; add `build_commit_intent_ix`, `build_execute_round_ix`, `build_cancel_intent_ix`)
- Modify: `programs/pool-program/src/lib.rs` (remove `withdraw` handler + `Withdraw` accounts)
- Delete: `programs/pool-program/tests/withdraw.rs`
- Rewrite: `crates/sdk/tests/e2e.rs` (deposit→commit(2)→execute), `crates/sdk/tests/sdk.rs` (commit-intent public-input parity)
- Test: `crates/sdk/tests/e2e.rs`, `crates/sdk/tests/sdk.rs`

**Interfaces:**
- Consumes: `sdk::{MerkleTree, Note}`, `prover`, `pool_program::round`, all instructions from Tasks 2–5.
- Produces: `sdk::CommitIntentBuild { instruction, public_inputs }`, `build_commit_intent_ix(...) -> Result<CommitIntentBuild, ProverError>`, `build_execute_round_ix(pool, vault, cranker, round_id, intents: &[(Pubkey, Pubkey, Pubkey)]) -> Instruction`, `build_cancel_intent_ix(pool, vault, recipient, round_id, nullifier_hash) -> Instruction`.

- [ ] **Step 1: Remove the standalone `withdraw` from the program**

Delete the `withdraw` handler from `#[program]` and the `Withdraw` accounts struct from `programs/pool-program/src/lib.rs`. Delete `programs/pool-program/tests/withdraw.rs`. Keep every error variant `commit_intent`/`execute_round` still use (`UnknownRoot`, `FeeExceedsDenomination`, `ProofInvalid`, `ProofMalformed`). Run `anchor build` — it must compile with no `withdraw` references.

- [ ] **Step 2: Write the failing SDK builders test (e2e)**

Rewrite `crates/sdk/tests/e2e.rs` to drive the full round trip through the SDK. Structure (real code — the prover runs, so this is a slow integration test):

```rust
//! End-to-end LiteSVM round trip driven THROUGH the SDK: initialize_pool ->
//! deposit(k) -> commit_intent(k) -> execute_round. Proves the SDK's
//! MerkleTree/proof/instruction builders agree with the on-chain program.
use litesvm::LiteSVM;
use sdk::{
    build_commit_intent_ix, build_deposit_ix, build_execute_round_ix,
    build_initialize_pool_ix, round_pda, MerkleTree, Note, WithdrawArtifacts,
};
use solana_sdk::{
    account::ReadableAccount, compute_budget::ComputeBudgetInstruction, instruction::Instruction,
    message::Message, pubkey::Pubkey, signature::{Keypair, Signer}, transaction::Transaction,
};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const DENOMINATION: u64 = 2_000_000;
const FEE: u64 = 1_000;

// (workspace_root + ensure_build_artifacts helpers — copy from the current
// e2e.rs; they generate circuits/build/* via setup.sh if missing.)

#[test]
fn sdk_driven_round_trip_two_intents() {
    let build_dir = ensure_build_artifacts();
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.add_program_from_file(pool_program::ID, so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &pool_program::ID);
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &pool_program::ID);

    // init(k_floor=2)
    let init = build_initialize_pool_ix(pool, vault, round_pda(pool, 0), mint, payer.pubkey(), DENOMINATION, 2);
    send(&mut svm, &payer, &[&payer], init);

    // two notes -> deposit both
    let notes = [Note::new(), Note::new()];
    let mut tree = MerkleTree::new().unwrap();
    for note in &notes {
        tree.insert(note.commitment());
        send(&mut svm, &payer, &[&payer], build_deposit_ix(pool, vault, payer.pubkey(), note.commitment(), DENOMINATION));
    }
    let root = tree.root();

    // commit both
    let mut triples = Vec::new();
    for (i, note) in notes.iter().enumerate() {
        let recipient = Pubkey::new_unique();
        let relayer = Pubkey::new_unique();
        let path = tree.authentication_path(i);
        let build = build_commit_intent_ix(
            pool, round_pda(pool, 0), recipient, relayer, payer.pubkey(),
            note, &path, root, FEE, 0,
            WithdrawArtifacts {
                wasm_path: &build_dir.join("withdraw_js").join("withdraw.wasm"),
                r1cs_path: &build_dir.join("withdraw.r1cs"),
                zkey_path: &build_dir.join("withdraw.zkey"),
            },
        ).unwrap();
        let (intent, _) = Pubkey::find_program_address(
            &[b"intent", pool.as_ref(), build.public_inputs.nullifier_hash.as_ref()],
            &pool_program::ID,
        );
        svm.expire_blockhash();
        send(&mut svm, &payer, &[&payer], build.instruction);
        triples.push((intent, recipient, relayer));
    }

    // execute
    let cranker = Keypair::new();
    svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    svm.expire_blockhash();
    let exec = build_execute_round_ix(pool, vault, cranker.pubkey(), 0, &triples);
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), exec],
        Some(&cranker.pubkey()),
    );
    svm.send_transaction(Transaction::new(&[&cranker], msg, svm.latest_blockhash())).unwrap();

    for (_, recipient, relayer) in &triples {
        assert_eq!(svm.get_account(recipient).unwrap().lamports(), DENOMINATION - FEE);
        assert_eq!(svm.get_account(relayer).unwrap().lamports(), FEE);
    }
}
```
Provide the `send`, `so_path`, `workspace_root`, `ensure_build_artifacts` helpers (copy the existing e2e.rs infra; `so_path` = `concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/deploy/pool_program.so")`).

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p sdk --test e2e`
Expected: FAIL — `build_commit_intent_ix` / `build_execute_round_ix` not defined.

- [ ] **Step 4: Implement the SDK builders**

In `crates/sdk/src/lib.rs`, remove `build_withdraw_ix` and its `WithdrawBuild` return type (keep `WithdrawArtifacts` — reused by commit). Add:

```rust
#[derive(Debug, Clone)]
pub struct CommitIntentBuild {
    pub instruction: Instruction,
    pub public_inputs: PublicInputs,
}

/// Builds `commit_intent`: generates a real Groth16 proof for `note` bound to
/// `(recipient, relayer, fee)` via extDataHash, then encodes the instruction.
/// Account order matches `programs/pool-program/src/lib.rs`'s `CommitIntent`.
#[allow(clippy::too_many_arguments)]
pub fn build_commit_intent_ix(
    pool: Pubkey,
    round: Pubkey,
    recipient: Pubkey,
    relayer: Pubkey,
    payer: Pubkey,
    note: &Note,
    merkle_path: &MerklePath,
    root: [u8; 32],
    fee: u64,
    round_id: u64,
    artifacts: WithdrawArtifacts,
) -> Result<CommitIntentBuild, ProverError> {
    let ext_data_hash = compute_ext_data_hash(&recipient.to_bytes(), &relayer.to_bytes(), fee);
    let inputs = WithdrawInputs {
        root,
        nullifier_hash: note.nullifier_hash(),
        ext_data_hash,
        nullifier: note.nullifier(),
        secret: note.secret(),
        path_elements: merkle_path.elements,
        path_indices: merkle_path.indices,
    };
    let (proof, public_inputs) =
        prover::prove_withdraw(artifacts.wasm_path, artifacts.r1cs_path, artifacts.zkey_path, &inputs)?;
    let withdraw_proof = pool_program::verifier::WithdrawProof {
        a: prover::proof_a_to_solana_be(&proof.a)?,
        b: prover::g2_to_solana_be(&proof.b)?,
        c: prover::g1_to_solana_be(&proof.c)?,
    };

    let (intent_pda, _) = Pubkey::find_program_address(
        &[b"intent", pool.as_ref(), public_inputs.nullifier_hash.as_ref()],
        &pool_program::ID,
    );
    let (nullifier_pda, _) = Pubkey::find_program_address(
        &[b"nullifier", pool.as_ref(), public_inputs.nullifier_hash.as_ref()],
        &pool_program::ID,
    );

    let mut data = discriminator("commit_intent").to_vec();
    data.extend_from_slice(&withdraw_proof.a);
    data.extend_from_slice(&withdraw_proof.b);
    data.extend_from_slice(&withdraw_proof.c);
    data.extend_from_slice(&public_inputs.root);
    data.extend_from_slice(&public_inputs.nullifier_hash);
    data.extend_from_slice(&fee.to_le_bytes());
    data.extend_from_slice(&round_id.to_le_bytes());

    let instruction = Instruction {
        program_id: pool_program::ID,
        accounts: vec![
            AccountMeta::new_readonly(pool, false),
            AccountMeta::new(round, false),
            AccountMeta::new(intent_pda, false),
            AccountMeta::new(nullifier_pda, false),
            AccountMeta::new_readonly(recipient, false),
            AccountMeta::new_readonly(relayer, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    Ok(CommitIntentBuild { instruction, public_inputs })
}

/// Builds `execute_round`. `intents` is `(intent_pda, recipient, relayer)` per
/// committed intent, in any order; they become the `remaining_accounts`.
pub fn build_execute_round_ix(
    pool: Pubkey,
    vault: Pubkey,
    cranker: Pubkey,
    round_id: u64,
    intents: &[(Pubkey, Pubkey, Pubkey)],
) -> Instruction {
    let (round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &round_id.to_le_bytes()],
        &pool_program::ID,
    );
    let (next_round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &(round_id + 1).to_le_bytes()],
        &pool_program::ID,
    );
    let mut accounts = vec![
        AccountMeta::new(pool, false),
        AccountMeta::new(round, false),
        AccountMeta::new(next_round, false),
        AccountMeta::new(vault, false),
        AccountMeta::new(cranker, true),
        AccountMeta::new_readonly(system_program::ID, false),
    ];
    for (intent, recipient, relayer) in intents {
        accounts.push(AccountMeta::new(*intent, false));
        accounts.push(AccountMeta::new(*recipient, false));
        accounts.push(AccountMeta::new(*relayer, false));
    }
    let mut data = discriminator("execute_round").to_vec();
    data.extend_from_slice(&round_id.to_le_bytes());
    Instruction { program_id: pool_program::ID, accounts, data }
}

/// Builds `cancel_intent` (recipient must sign).
pub fn build_cancel_intent_ix(
    pool: Pubkey,
    vault: Pubkey,
    recipient: Pubkey,
    round_id: u64,
    nullifier_hash: [u8; 32],
) -> Instruction {
    let (round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &round_id.to_le_bytes()],
        &pool_program::ID,
    );
    let (intent, _) = Pubkey::find_program_address(
        &[b"intent", pool.as_ref(), nullifier_hash.as_ref()],
        &pool_program::ID,
    );
    let mut data = discriminator("cancel_intent").to_vec();
    data.extend_from_slice(&round_id.to_le_bytes());
    data.extend_from_slice(&nullifier_hash);
    Instruction {
        program_id: pool_program::ID,
        accounts: vec![
            AccountMeta::new_readonly(pool, false),
            AccountMeta::new(round, false),
            AccountMeta::new(intent, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(recipient, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    }
}
```

- [ ] **Step 5: Rewrite `crates/sdk/tests/sdk.rs`**

Replace the withdraw public-input parity test with a `commit_intent` one: build a `commit_intent` instruction via `build_commit_intent_ix` for the committed bundle note (reconstruct its tree with `MerkleTree`), and assert the ix's embedded `root`/`nullifier_hash` bytes match `build.public_inputs` and the program's own recomputation of `ext_data_hash` for the chosen recipient/relayer/fee. Reuse the file's existing bundle-loading helpers; change `build_withdraw_ix` → `build_commit_intent_ix` and the data-offset assertions to the `commit_intent` layout (proof.a/b/c, root, nullifier_hash, fee, round_id).

- [ ] **Step 6: Run the full workspace**

Run: `cargo test -p sdk` then `cargo test -p pool-program` then `cargo test --workspace`.
Expected: all green. Confirm no references to `withdraw`/`build_withdraw_ix` remain: `git grep -n "build_withdraw_ix\|fn withdraw\|struct Withdraw\b"` returns nothing (except historical plan/spec docs).

- [ ] **Step 7: Lint + commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
git add crates/sdk programs/pool-program
git rm programs/pool-program/tests/withdraw.rs
git commit -m "feat(sdk): round instruction builders + e2e; remove standalone withdraw (exit only via k-round)"
```

---

## Self-Review

**1. Spec coverage** (`docs/superpowers/specs/2026-07-15-mirror-pool-design.md`):
- §2 phase 2 "round engine + k-floor" → Tasks 2–4. §2 phase 3 "PooledAction adapters (behavioral pooling)" seam → Task 4 (`action.rs`), one `Withdraw` impl (further adapters deferred, per approved scope).
- §3.1 instructions `commit_intent`/`execute_round` → Tasks 3–4; `["round",pool,id]` / `["member/intent",...]` PDAs → Tasks 2–4. (`emergency_withdraw` deferred; `cancel_intent` is the coordinator-independent recovery path for this scope.)
- §4 data-flow ④ FORM (k-floor) / ⑤ EXECUTE (vault-signed batch, uniform actor) / ⑦ EXIT (withdraw = a PooledAction) → Task 4. ② DEPOSIT unchanged.
- §4 guarantee (c) "sub-k batches rejected on-chain" → `execute_round` `KFloorNotMet` (Task 4) + host test `meets_k_floor` (Task 2).
- §5 threat model: malicious coordinator forcing thin round → on-chain k-floor (Task 4); custody value-conservation → `split_payout` + vault-debit assertion (Tasks 2, 4); fund redirection → extDataHash-bound recorded keys + `IntentAccountMismatch` (Tasks 3–4); replay/double-commit → nullifier PDA (Task 3); coordinator-down recovery → `cancel_intent` (Task 5).
- §6 testing: invariant `proptest`-style host tests (Task 2), LiteSVM lifecycle (Tasks 3–5), adversarial (sub-k, replay, duplicate, cross-pool, wrong-signer) across Tasks 3–5. (§6.5 empirical anonymity simulation is a separate future analysis, out of this plan's scope.)

**2. Placeholder scan:** `build_round_fixture_signer_recipients` (Task 5, Step 1a) is intentionally described as "copy the body, change recipient to a Keypair" rather than repeating the ~80-line fixture verbatim — the referenced body is fully specified in Task 3. Every handler and accounts struct is complete code. No TBD/TODO in shipped code.

**3. Type consistency:** `Intent { pool, round_id, recipient, relayer, fee, action }` and `Round { state, intent_count }` are used identically in `round.rs` (Task 2), the handlers (Tasks 3–5), and the tests. `meets_k_floor(u32, u16)` / `split_payout(u64,u64)->Result<(u64,u64)>` signatures match between `invariants.rs`, `action.rs` (`WithdrawAction::execute`), and `execute_round`. `MerkleTree`/`MerklePath` fields (`elements`,`indices`) match Task 1 and the fixture. Instruction arg order (and thus Borsh data layout) is consistent between each handler's `#[instruction(...)]`, the test tx builders, and the SDK builders (`commit_intent`: proof, root, nullifier_hash, fee, round_id).

**4. Folded from the independent pre-implementation review:**
- **I3 (dead code removed):** `execute_round`'s `WrongRound`/`RoundClosed` handler checks were removed — the `next_round` `init` constraint and Anchor's `round` account load ARE the re-execution / stale-round guards (Task 4 Step 4). `WrongRound` survives only in `commit_intent` (reachable); `RoundClosed` only in `commit_intent`/`cancel_intent` (cancel's is a CRITICAL guard against a double-refund after execute).
- **I1/I2/I4 (test hardening):** added `execute_round` tests for payout redirection (`IntentAccountMismatch`), a real cross-pool intent (a crafted program-owned `Intent` with a foreign `pool` → `IntentInvalid`), an incomplete account set (`IntentAccountsMismatch`), plus commit-path `fee > denomination` and commit-to-executed-round (`WrongRound`) negatives. Every reachable guard now has an honest test.
- **I6 (intentional spec deviation):** proof verification happens ONCE at `commit_intent`, not re-verified at `execute_round` (spec §4 ⑤ says "re-verify"). This is deliberate and stronger — the nullifier is already burned and payout keys already bound, so re-verification is redundant, and it keeps the ~107k-CU `alt_bn128` pairing OUT of the k-way batch (mitigating the k-per-transaction ceiling below).
- **M2:** `MerkleTree::insert` expects in-field leaves (as produced by `Note::commitment`); documented as a precondition rather than returning `Result` — the fixture only ever inserts `Note` commitments.
- **M6:** `crates/sdk/tests/sdk.rs` is NOT an `initialize_pool` caller (it uses `build_deposit_ix`/`build_withdraw_ix` only) — Task 2's sweep drops it (it's rewritten in Task 6).
- **M7 (honest scope):** the on-chain k-floor guarantees k *intents*, not k distinct *participants* — a self-Sybil can commit k of its own notes. Anti-Sybil (bonding) is spec §5's deferred phase-4 work; this plan ships the enforcement primitive, not Sybil resistance.

**Deferred (out of scope, noted):** off-chain coordinator service, additional `PooledAction` adapters (Stake/Swap), `emergency_withdraw`, executed-intent rent reclamation (`execute_round` leaves executed `Intent` PDAs in place — round-state guards re-execution; a `reap` cleanup instruction is a Plan 5+ optimization), multi-denomination, production trusted-setup ceremony, empirical anonymity simulation (§6.5). **I5 — the k-per-transaction ceiling:** each intent consumes 3 `remaining_accounts` + 2 vault CPIs, so a single `execute_round` tx (Solana's account/CU/1232-byte limits, no ALTs in this scope) caps a round at roughly a dozen intents; batched/paginated or ALT-based large rounds are future work — the Task 4 test prints CU at k=2 to seed a per-intent cost estimate. **M5 — no events:** `commit_intent`/`execute_round`/`cancel_intent` emit no events (a conscious YAGNI call for the on-chain-core scope; clients poll PDAs/balances); event emission for client scanning (spec §4 ⑥) is deferred with the coordinator. **M1 — `round_id + 1` overflow:** seed derivation for `next_round` panics under `overflow-checks` at `round_id == u64::MAX` (unreachable — needs 2^64 rounds); noted, not guarded.
