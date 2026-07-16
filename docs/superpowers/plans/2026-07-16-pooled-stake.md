# Pooled Stake (Plan 5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add pooled **staking** as the second `PooledAction` adapter — k participants each delegate a bucketed amount to a fixed validator in one vault-signed, k-floor-gated batch — which is the first behavioral (non-exit) action and the change that finally makes the `PooledAction` seam genuinely generic (two impls, per-kind dispatch).

**Architecture:** A `Pool` gains an `action_kind` (Withdraw | Stake) + `validator` + `stake_fee`; a stake pool's round executes `StakeAction` per intent — a 4-CPI sequence (CreateAccount → Initialize(staker=vault, withdrawer=recipient) → DelegateStake[vault signs] → Authorize[staker: vault→recipient]) so the vault acts unilaterally while the participant ends with both authorities. No new circuit (the withdraw proof's `extDataHash` binds the stake authority/relayer/fee); no intent-model rewrite (`recipient` = the stake authority). `execute_round` reads `pool.action_kind` and dispatches + parses `remaining_accounts` per kind.

**Tech Stack:** Rust 2021, Anchor 0.31.1, `zero_copy` `Pool` (`AccountLoader`/bytemuck `Pod`), `solana-stake-interface` 1.2.1 (already in the tree via `solana-program` 2.2), native Stake program CPIs via `invoke_signed`, LiteSVM 0.6.1 (with a set-up vote account), `groth16-solana` verifier (unchanged), `proptest`/host unit tests for the value split.

**Design source:** [`docs/superpowers/specs/2026-07-16-pooled-stake-design.md`](../specs/2026-07-16-pooled-stake-design.md) (reviewed against `solana-stake-interface` + mainnet `getStakeMinimumDelegation`).

## Global Constraints

Every task's requirements implicitly include this section. Copy exact values verbatim.

- **Anchor 0.31.1 / Rust 2021.** `cargo fmt` + `cargo clippy --all-targets -- -D warnings` clean before every commit. `overflow-checks` stays on. Conventional commits, one logical change each.
- **Custody fail-closed.** On-chain paths return typed `PoolError`s. No `unwrap()`/`expect()`/`panic!` on attacker-influenced input. Checked arithmetic / `require!` for every amount, index, count. The value split MUST conserve value: `denomination = stake_fee + stake_rent + delegated`, never over-drain the vault.
- **`k`-floor enforced ON-CHAIN, unchanged.** `execute_round` still rejects `intent_count < k_floor`. The stake path does not weaken any Plan 4 guarantee (uniform actor, single-spend, replay, no redirection).
- **Uniform actor + unlinkable identity.** All delegations of a round happen in ONE vault-signed transaction to the SAME validator for the SAME amount; the per-participant stake-account PDA + authority necessarily differ but are ZK-unlinkable (the withdraw privacy model). Do NOT assert byte-identity on the identity fields.
- **A pool is ONE action kind.** A round must never mix withdraw and stake intents (trivially distinguishable). `action_kind` lives on the `Pool`; `execute_round` dispatches on it.
- **Error variants are APPEND-ONLY.** Append every new variant AFTER the current last one (`DuplicateIntent`) — never insert/reorder (hardcoded codes in `deposit.rs` 6001/6002 depend on it).
- **`Pool` stays `zero_copy` / `repr(C)` with NO implicit padding** (bytemuck `Pod` rejects it) and `size_of::<Pool>()` a multiple of 8 — same discipline as Plan 4's tail fields. New fields are appended to the tail; all existing `offset_of!` sites stay valid.
- **Stake value split (verified):** the stake account is funded with `denomination − stake_fee`; DelegateStake stakes its balance above the rent-exempt reserve, so `delegated = denomination − stake_fee − stake_rent`. A **Stake** pool is valid only if `delegated ≥ MIN_STAKE_DELEGATION` (1 SOL = 1_000_000_000 lamports on mainnet, `getStakeMinimumDelegation`) — enforced fail-closed at `initialize_pool`; `DelegateStake` is the ultimate enforcer at execute.
- **Never log/emit secrets.** No note secret, nullifier preimage, or member→action mapping.
- **Tests are Rust-native** (host unit + LiteSVM). Adversarial/negative cases mandatory (sub-k, substituted authority, foreign-pool/round intent, duplicate, wrong stake PDA, re-execute). The **withdraw suite must stay green** (seam-regression proof).
- A change isn't done until `cargo test -p <crate>` is green and you've said so with the output.

---

## File Structure

- `programs/pool-program/src/state.rs` — *modify.* `Pool` gains `action_kind: u8`, `validator: Pubkey`, `stake_fee: u64` (padding-safe tail).
- `programs/pool-program/src/round.rs` — *modify.* `ActionKind` gains `Stake`.
- `programs/pool-program/src/invariants.rs` — *modify.* Add `stake_split(denomination, stake_fee, stake_rent) -> Result<(u64, u64)>` (delegated, fee) + host tests; `MIN_STAKE_DELEGATION` const.
- `programs/pool-program/src/action.rs` — *modify.* Add `StakeAction` (the 4-CPI stake effect).
- `programs/pool-program/src/lib.rs` — *modify.* `initialize_pool` gains `action_kind`/`validator`/`stake_fee` (+ validity); `execute_round` dispatches per `pool.action_kind`; new error variants; `Pubkey`/stake imports.
- `programs/pool-program/Cargo.toml` — *modify.* Add `solana-stake-interface = "1.2"`.
- `crates/sdk/src/lib.rs` — *modify.* `build_initialize_pool_ix` gains the stake args; `build_execute_round_ix` assembles per-kind `remaining_accounts`; `stake_account_pda` helper.
- Tests: `programs/pool-program/tests/stake_round.rs` — *new.* LiteSVM pooled-stake round (vote-account setup) + adversarial. `initialize_pool.rs` / `round_support.rs` — *modify* (new init args). `crates/sdk/tests/e2e.rs` — *modify* (a stake round trip). Existing withdraw/commit/execute/cancel suites — *keep green*.

**Interface names (verbatim):** `Pool.action_kind: u8` (0=Withdraw, 1=Stake), `Pool.validator: Pubkey`, `Pool.stake_fee: u64`; `round::ActionKind::Stake`; `invariants::{MIN_STAKE_DELEGATION, stake_split}`; `action::StakeAction`; `initialize_pool(denomination: u64, k_floor: u16, action_kind: u8, validator: Pubkey, stake_fee: u64)`; stake `remaining_accounts` layout = `[intent, stake_account, relayer] × count` then the shared tail `[validator, stake_program, stake_config, clock, stake_history, rent]`; stake PDA seeds `["stake", pool, nullifier_hash]`.

---

## Task 1: Pool stake config + `ActionKind::Stake` + `initialize_pool` validity + sweep

Give the `Pool` an action kind + stake params, add the `Stake` variant, validate stake-pool config at init (denomination clears the 1-SOL delegation floor), and sweep every `initialize_pool` caller. Withdraw pools are unaffected (they pass `action_kind = 0`).

**Files:** Modify `state.rs`, `round.rs`, `invariants.rs`, `lib.rs`; sweep `crates/sdk/src/lib.rs`, `programs/pool-program/tests/{initialize_pool.rs,deposit.rs,round_support.rs}`, `crates/sdk/tests/e2e.rs`. Test: `invariants.rs` host tests + `initialize_pool.rs`.

**Interfaces:**
- Produces: `Pool.{action_kind, validator, stake_fee}`; `ActionKind::Stake`; `invariants::{MIN_STAKE_DELEGATION, stake_split, STAKE_ACCOUNT_SIZE}`; `initialize_pool(denomination, k_floor, action_kind, validator, stake_fee)`; error variants `WrongActionConfig`, `StakeDenominationTooLow`.

- [ ] **Step 1: Host tests for the value split (write first, run, fail)**

Add to `programs/pool-program/src/invariants.rs`:

```rust
/// Stake account layout size (StakeStateV2) — used only for the rent-exempt
/// minimum. Kept as a const so the split math is host-testable without a syscall.
pub const STAKE_ACCOUNT_SIZE: usize = 200;

/// The Stake program's minimum delegation (1 SOL on mainnet, verified via
/// `getStakeMinimumDelegation` — the `stake_raise_minimum_delegation_to_1_sol`
/// feature is active). The on-chain `DelegateStake` is the ultimate enforcer;
/// this const gates `initialize_pool` so a stake pool can't be created that would
/// fail every round.
pub const MIN_STAKE_DELEGATION: u64 = 1_000_000_000;

/// Split a stake pool's `denomination` into `(delegated, fee)`. The stake account
/// is funded with `denomination - stake_fee`; DelegateStake stakes its balance
/// above the rent reserve, so `delegated = denomination - stake_fee - stake_rent`.
/// Fails closed if the fee+rent exceed the denomination or the delegated amount is
/// below the network minimum.
pub fn stake_split(denomination: u64, stake_fee: u64, stake_rent: u64) -> Result<(u64, u64)> {
    let after_fee = denomination
        .checked_sub(stake_fee)
        .ok_or(error!(PoolError::FeeExceedsDenomination))?;
    let delegated = after_fee
        .checked_sub(stake_rent)
        .ok_or(error!(PoolError::StakeDenominationTooLow))?;
    require!(
        delegated >= MIN_STAKE_DELEGATION,
        PoolError::StakeDenominationTooLow
    );
    Ok((delegated, stake_fee))
}

#[cfg(test)]
mod stake_tests {
    use super::*;

    const RENT: u64 = 2_282_880; // ~rent-exempt for 200 bytes; exact value pinned at runtime

    #[test]
    fn stake_split_conserves_and_floors() {
        let denom = MIN_STAKE_DELEGATION + RENT + 5_000;
        assert_eq!(stake_split(denom, 5_000, RENT).unwrap(), (MIN_STAKE_DELEGATION, 5_000));
    }

    #[test]
    fn stake_split_rejects_below_min_delegation() {
        // delegated = MIN - 1 < MIN → fail closed
        let denom = MIN_STAKE_DELEGATION - 1 + RENT + 5_000;
        assert!(stake_split(denom, 5_000, RENT).is_err());
    }

    #[test]
    fn stake_split_rejects_fee_plus_rent_over_denomination() {
        assert!(stake_split(1_000, 900, 200).is_err());
    }
}
```

Run: `cargo test -p pool-program --lib stake_tests`
Expected: FAIL — `stake_split` / `StakeDenominationTooLow` not defined.

- [ ] **Step 2: Add the `Stake` variant + the error variants**

In `programs/pool-program/src/round.rs`:
```rust
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActionKind {
    Withdraw,
    Stake,
}
```

In `programs/pool-program/src/lib.rs`, APPEND after `DuplicateIntent`:
```rust
    #[msg("action_kind/validator/stake_fee configuration is invalid for this pool")]
    WrongActionConfig,
    #[msg("stake pool denomination is too low to cover fee + rent + minimum delegation")]
    StakeDenominationTooLow,
    #[msg("account does not match the pool's configured stake action")]
    StakeAccountInvalid,
```

- [ ] **Step 3: Run the host tests to verify they pass**

Run: `cargo test -p pool-program --lib stake_tests`
Expected: PASS (3 tests).

- [ ] **Step 4: Extend `Pool` (padding-safe)**

In `programs/pool-program/src/state.rs`, append to the `Pool` tail (after `current_round_id`):
```rust
    pub current_round_id: u64,
    // Plan 5: a pool is ONE action kind (0 = Withdraw, 1 = Stake). Stored as u8
    // (not the `ActionKind` enum) because zero_copy `Pool` is bytemuck `Pod`.
    // `stake_fee` (8-aligned at the current tail end 3936) then `validator`
    // ([u8;32], 1-aligned) then `action_kind` (u8) then an explicit trailing pad
    // keep the struct free of implicit padding and a multiple of 8 (3936 → 3984).
    pub stake_fee: u64,
    pub validator: Pubkey,
    pub action_kind: u8,
    _reserved3: [u8; 7],
}
```
(The `const _: () = assert!(core::mem::size_of::<Pool>().is_multiple_of(8));` below the struct must still hold — new size 3984, a multiple of 8.)

- [ ] **Step 5: `initialize_pool` — new args + validity**

In `programs/pool-program/src/lib.rs`, change the handler:
```rust
    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        denomination: u64,
        k_floor: u16,
        action_kind: u8,
        validator: Pubkey,
        stake_fee: u64,
    ) -> Result<()> {
        require!(k_floor >= crate::round::MIN_K_FLOOR, PoolError::KFloorTooLow);
        // Validate the action config. Withdraw pools carry no stake params; stake
        // pools must name a validator and clear the delegation floor.
        match action_kind {
            0 => require!(
                validator == Pubkey::default() && stake_fee == 0,
                PoolError::WrongActionConfig
            ),
            1 => {
                require!(validator != Pubkey::default(), PoolError::WrongActionConfig);
                let stake_rent = Rent::get()?.minimum_balance(crate::invariants::STAKE_ACCOUNT_SIZE);
                // Fails closed if denomination can't cover fee + rent + min delegation.
                crate::invariants::stake_split(denomination, stake_fee, stake_rent)?;
            }
            _ => return err!(PoolError::WrongActionConfig),
        }

        // ... existing vault funding + tree init (unchanged) ...

        {
            let mut pool = ctx.accounts.pool.load_init()?;
            pool.mint = ctx.accounts.mint.key();
            pool.denomination = denomination;
            pool.k_floor = k_floor;
            pool.current_round_id = 0;
            pool.action_kind = action_kind;
            pool.validator = validator;
            pool.stake_fee = stake_fee;
            pool.bump = ctx.bumps.pool;
            pool.vault_bump = ctx.bumps.vault;
            pool.filled_subtrees = z;
            pool.current_root = root;
            pool.roots[0] = root;
        }
        // ... existing Round(0) creation (unchanged) ...
        Ok(())
    }
```
(Keep the existing vault-funding, tree-init, and `Round(0)` bodies exactly; only the signature, the validity `match`, and the three `pool.*` field sets are added.)

- [ ] **Step 6: Sweep every `initialize_pool` caller (add the 3 args)**

Withdraw callers pass `action_kind = 0, validator = Pubkey::default(), stake_fee = 0`:
- `crates/sdk/src/lib.rs::build_initialize_pool_ix` — add `action_kind: u8, validator: Pubkey, stake_fee: u64` params; append `action_kind` (1 byte) + `validator` (32) + `stake_fee.to_le_bytes()` (8) to the instruction data after `k_floor`. Update its unit test.
- `programs/pool-program/tests/initialize_pool.rs`, `deposit.rs::setup_pool`, `round_support.rs` (both fixtures), `crates/sdk/tests/e2e.rs` — pass the withdraw defaults (`0`, default pubkey, `0`) after `k_floor` in the `initialize_pool` data / builder call.
- Data layout after this task: `disc(8)‖denomination(8)‖k_floor(2)‖action_kind(1)‖validator(32)‖stake_fee(8)`.

- [ ] **Step 7: Stake-pool init tests**

Append to `programs/pool-program/tests/initialize_pool.rs`:
```rust
#[test]
fn initialize_stake_pool_stores_config() {
    // action_kind = 1, a nonzero validator, denomination clearing the floor.
    // Assert Pool.action_kind == 1, Pool.validator == V, Pool.stake_fee == F via
    // offset_of! reads (mirroring the existing current_root offset test).
    // (denomination = MIN_STAKE_DELEGATION + rent(~0.0023 SOL) + fee + slack.)
}

#[test]
fn initialize_stake_pool_rejects_below_delegation_floor() {
    // action_kind = 1 with denomination < fee + rent + MIN_STAKE_DELEGATION →
    // expect_err, logs contain "StakeDenominationTooLow".
}

#[test]
fn initialize_withdraw_pool_rejects_stake_params() {
    // action_kind = 0 with a nonzero validator → WrongActionConfig.
}
```
Write these in full using the existing `initialize_pool.rs` helpers (hand-built ix + `Pool::try_deserialize` / `offset_of!`).

- [ ] **Step 8: Build, run, lint, commit**

Run: `anchor build` (or `cargo build-sbf --manifest-path programs/pool-program/Cargo.toml`), then `cargo test -p pool-program` and `cargo test -p sdk` — all green (existing suites updated for the new signature; new stake-config tests pass). `cargo fmt` + `cargo clippy --all-targets -- -D warnings` clean.
```bash
git add programs/pool-program/src crates/sdk/src/lib.rs programs/pool-program/tests crates/sdk/tests
git commit -m "feat(pool-program): pool action_kind/validator/stake_fee + stake_split invariant + initialize_pool validity"
```

---

## Task 2: `StakeAction` — the 4-CPI stake effect

The heart of Plan 5: the `PooledAction` impl that delegates one intent's note to the pool's validator, unilaterally (vault-signed), leaving the participant with both stake authorities.

**Files:** Modify `programs/pool-program/Cargo.toml` (dep), `programs/pool-program/src/action.rs`. (Exercised end-to-end in Task 3's LiteSVM tests.)

**Interfaces:**
- Consumes: `invariants::stake_split`, the vault signer seeds, the Stake program CPIs.
- Produces: `action::StakeAction` impl of `PooledAction`.

- [ ] **Step 1: Add the stake-interface dependency**

In `programs/pool-program/Cargo.toml` `[dependencies]`:
```toml
solana-stake-interface = "1.2"
```
(Already resolved in the lockfile via `solana-program` 2.2 — this makes it a direct dep for the instruction builders + `StakeStateV2`.)

- [ ] **Step 2: Implement `StakeAction`**

In `programs/pool-program/src/action.rs`, add (keep `WithdrawAction` unchanged):
```rust
use anchor_lang::solana_program::{program::invoke_signed, system_instruction};
use solana_stake_interface::{
    instruction as stake_instruction,
    state::{Authorized, Lockup, StakeAuthorize, StakeStateV2},
};

/// Delegate a single intent's note to the pool's validator. Ordered so the VAULT
/// acts unilaterally (the participant's key is never present at execute):
///   1. CreateAccount    the stake PDA, funded with `denomination - fee`
///   2. Initialize       staker = VAULT, withdrawer = intent.recipient
///   3. DelegateStake     vault signs as staker → validator
///   4. Authorize(Staker) vault → intent.recipient (participant now holds both authorities)
///   5. fee → relayer
/// `delegated = denomination - fee - stake_rent` is what actually stakes (balance
/// above the rent reserve); `DelegateStake` enforces the network minimum.
pub struct StakeAction<'a, 'info> {
    pub vault: AccountInfo<'info>,
    pub stake_account: AccountInfo<'info>,
    pub recipient: AccountInfo<'info>, // = the stake authority (data, not a signer here)
    pub relayer: AccountInfo<'info>,
    pub validator: AccountInfo<'info>,
    pub stake_program: AccountInfo<'info>,
    pub stake_config: AccountInfo<'info>,
    pub clock: AccountInfo<'info>,
    pub stake_history: AccountInfo<'info>,
    pub rent: AccountInfo<'info>,
    pub system_program: AccountInfo<'info>,
    pub vault_seeds: &'a [&'a [&'a [u8]]],
    pub stake_seeds: &'a [&'a [&'a [u8]]],
    pub denomination: u64,
    pub fee: u64,
    pub stake_rent: u64,
}

impl PooledAction for StakeAction<'_, '_> {
    fn execute(&self) -> Result<()> {
        // Value split (fail-closed) — total to the stake account = denomination - fee.
        let (_delegated, fee) =
            crate::invariants::stake_split(self.denomination, self.fee, self.stake_rent)?;
        let to_stake = self
            .denomination
            .checked_sub(fee)
            .ok_or(error!(crate::PoolError::FeeExceedsDenomination))?;

        // 1. Create the stake PDA (vault funds; PDA signs for itself).
        invoke_signed(
            &system_instruction::create_account(
                self.vault.key,
                self.stake_account.key,
                to_stake,
                StakeStateV2::size_of() as u64,
                self.stake_program.key,
            ),
            &[self.vault.clone(), self.stake_account.clone(), self.system_program.clone()],
            &[self.vault_seeds[0], self.stake_seeds[0]],
        )?;

        // 2. Initialize: staker = VAULT, withdrawer = participant.
        let authorized = Authorized { staker: *self.vault.key, withdrawer: *self.recipient.key };
        invoke_signed(
            &stake_instruction::initialize(self.stake_account.key, &authorized, &Lockup::default()),
            &[self.stake_account.clone(), self.rent.clone()],
            &[self.stake_seeds[0]],
        )?;

        // 3. Delegate — the VAULT signs as the staker authority.
        invoke_signed(
            &stake_instruction::delegate_stake(
                self.stake_account.key,
                self.vault.key,
                self.validator.key,
            ),
            &[
                self.stake_account.clone(),
                self.validator.clone(),
                self.clock.clone(),
                self.stake_history.clone(),
                self.stake_config.clone(),
                self.vault.clone(),
            ],
            &[self.vault_seeds[0]],
        )?;

        // 4. Hand the staker authority to the participant.
        invoke_signed(
            &stake_instruction::authorize(
                self.stake_account.key,
                self.vault.key,
                self.recipient.key,
                StakeAuthorize::Staker,
                None,
            ),
            &[self.stake_account.clone(), self.clock.clone(), self.vault.clone()],
            &[self.vault_seeds[0]],
        )?;

        // 5. Fee → relayer (from the vault).
        if fee > 0 {
            invoke_signed(
                &system_instruction::transfer(self.vault.key, self.relayer.key, fee),
                &[self.vault.clone(), self.relayer.clone(), self.system_program.clone()],
                self.vault_seeds,
            )?;
        }
        Ok(())
    }
}
```
(`vault_seeds`/`stake_seeds` are `&[&[&[u8]]]` with one entry each — `self.vault_seeds[0]` / `self.stake_seeds[0]` are the seed slices. The exact `delegate_stake`/`authorize` account orders come from `solana-stake-interface` 1.2 — the reviewer confirmed the stake-config account is still passed for backward compatibility.)

- [ ] **Step 3: Build + lint + commit**

Run: `cargo build-sbf --manifest-path programs/pool-program/Cargo.toml` (compiles the CPIs; no test yet — Task 3 exercises it). `cargo fmt` + `cargo clippy -p pool-program --all-targets -- -D warnings` clean.
```bash
git add programs/pool-program/Cargo.toml programs/pool-program/src/action.rs
git commit -m "feat(pool-program): StakeAction — vault-unilateral 4-CPI pooled delegation"
```

---

## Task 3: `execute_round` per-kind dispatch + LiteSVM pooled-stake round

Make `execute_round` branch on `pool.action_kind`, parse the stake `remaining_accounts` layout, and build/verify each `StakeAction`. Then prove a real pooled-stake round in LiteSVM (with a validator vote account) plus the adversarial set. **The withdraw path stays byte-for-byte as it is** (the branch wraps it).

**Files:** Modify `programs/pool-program/src/lib.rs` (execute_round). Create `programs/pool-program/tests/stake_round.rs`. Modify `round_support.rs` (a stake fixture + a vote-account setup helper).

**Interfaces:**
- Consumes: `action::StakeAction`, `Pool.{action_kind, validator, stake_fee}`, `invariants::STAKE_ACCOUNT_SIZE`.
- Produces: the stake dispatch arm; stake `remaining_accounts` layout `[intent, stake_account, relayer]×count` + shared `[validator, stake_program, stake_config, clock, stake_history, rent]`; stake PDA seeds `["stake", pool, nullifier_hash]`.

- [ ] **Step 1: Write the failing pooled-stake test**

Create `programs/pool-program/tests/stake_round.rs`. Use a `round_support` helper that (a) creates a validator **vote account** in the SVM (via `litesvm`'s `set_account` with a serialized `VoteState`, or the Vote program CreateAccount), (b) initializes a **Stake** pool at that validator, (c) deposits + commits k notes with stake authorities. The test then builds `execute_round` with the stake `remaining_accounts` and asserts:
```rust
// after execute_round on a k=2 stake pool:
for m in &fx.intents {
    let (stake_pda, _) = Pubkey::find_program_address(
        &[b"stake", fx.pool.as_ref(), m.nullifier_hash.as_ref()], &program_id());
    let acct = fx.svm.get_account(&stake_pda).unwrap();
    assert_eq!(acct.owner, solana_sdk::stake::program::ID, "stake account owned by Stake program");
    // deserialize StakeStateV2: assert delegation.voter_pubkey == validator,
    // and authorized.staker == authorized.withdrawer == m.recipient (post-Authorize).
}
// vault debited exactly k * denomination (fee to relayer + rest into stake accounts).
```
Plus adversarial tests (full code, mirroring `execute_round.rs`): sub-k → `KFloorNotMet`; a substituted authority (wrong `recipient` in the triple) → `IntentAccountMismatch`; a foreign-pool crafted `Intent` → `IntentInvalid`; a duplicated intent → `DuplicateIntent`; a **wrong stake-account PDA** (not `["stake", pool, nullifier_hash]`) → `StakeAccountInvalid`; re-execute → "already in use".

Run: `cargo test -p pool-program --test stake_round`
Expected: FAIL — the stake dispatch doesn't exist; execute_round still assumes withdraw's 3-account layout.

- [ ] **Step 2: Implement the per-kind dispatch in `execute_round`**

In `programs/pool-program/src/lib.rs`, replace the fixed `rem.len() == count * 3` block + the single withdraw loop with a branch on `pool.action_kind` (read the action_kind + validator + stake_fee into the initial tuple). Keep the existing withdraw arm exactly; add the stake arm:

```rust
        // (read action_kind, validator, stake_fee alongside the existing tuple)
        let rem = ctx.remaining_accounts;
        match action_kind {
            0 => { /* WITHDRAW — the existing code verbatim: rem.len()==count*3, the loop */ }
            1 => {
                // Stake: [intent, stake_account, relayer] × count, then the shared tail.
                const TAIL: usize = 6; // validator, stake_program, stake_config, clock, stake_history, rent
                require!(
                    rem.len() == (count as usize) * 3 + TAIL,
                    PoolError::IntentAccountsMismatch
                );
                let tail = &rem[(count as usize) * 3..];
                let (validator_ai, stake_prog, stake_config, clock, stake_history, rent_ai) =
                    (&tail[0], &tail[1], &tail[2], &tail[3], &tail[4], &tail[5]);
                require_keys_eq!(*validator_ai.key, validator, PoolError::StakeAccountInvalid);
                let stake_rent = Rent::get()?.minimum_balance(crate::invariants::STAKE_ACCOUNT_SIZE);

                let mut seen: Vec<Pubkey> = Vec::with_capacity(count as usize);
                for i in 0..(count as usize) {
                    let intent_ai = &rem[i * 3];
                    let stake_ai = &rem[i * 3 + 1];
                    let relayer_ai = &rem[i * 3 + 2];

                    let intent: Account<crate::round::Intent> = Account::try_from(intent_ai)
                        .map_err(|_| error!(PoolError::IntentInvalid))?;
                    require_keys_eq!(intent.pool, pool_key, PoolError::IntentInvalid);
                    require!(intent.round_id == round_id, PoolError::IntentInvalid);
                    require!(!seen.contains(intent_ai.key), PoolError::DuplicateIntent);
                    seen.push(*intent_ai.key);
                    require!(
                        intent.action == crate::round::ActionKind::Stake,
                        PoolError::IntentInvalid
                    );
                    // The stake account must be the intent's canonical PDA.
                    let nh = /* nullifier_hash for this intent: see note below */;
                    let (expected_stake, _) = Pubkey::find_program_address(
                        &[b"stake", pool_key.as_ref(), nh.as_ref()], &crate::ID);
                    require_keys_eq!(*stake_ai.key, expected_stake, PoolError::StakeAccountInvalid);
                    require_keys_eq!(*relayer_ai.key, intent.relayer, PoolError::IntentAccountMismatch);

                    let stake_bump_arr = [/* stake bump */];
                    let stake_seed_refs: &[&[u8]] =
                        &[b"stake", pool_key.as_ref(), nh.as_ref(), &stake_bump_arr];
                    let stake_seeds: &[&[&[u8]]] = &[stake_seed_refs];

                    let action = crate::action::StakeAction {
                        vault: ctx.accounts.vault.to_account_info(),
                        stake_account: stake_ai.clone(),
                        recipient: /* AccountInfo carrying intent.recipient — see note */,
                        relayer: relayer_ai.clone(),
                        validator: validator_ai.clone(),
                        stake_program: stake_prog.clone(),
                        stake_config: stake_config.clone(),
                        clock: clock.clone(),
                        stake_history: stake_history.clone(),
                        rent: rent_ai.clone(),
                        system_program: ctx.accounts.system_program.to_account_info(),
                        vault_seeds: signer_seeds,
                        stake_seeds,
                        denomination,
                        fee: intent.fee,
                        stake_rent,
                    };
                    crate::action::PooledAction::execute(&action)?;
                }
            }
            _ => return err!(PoolError::WrongActionConfig),
        }
        // (round.state = Executed; current_round_id += 1; next_round init — unchanged)
```

**Two implementation notes the implementer must resolve (call them out in the report):**
1. **`nullifier_hash` at execute.** The stake PDA seed needs the intent's `nullifier_hash`, which is NOT currently a field of `Intent`. Cleanest: **add `nullifier_hash: [u8; 32]` to `Intent`** (set in `commit_intent`, `Intent::SPACE += 32`) so `execute_round` can re-derive `["stake", pool, nullifier_hash]` and verify the passed stake account. (This also lets the withdraw path drop nothing — it just carries the field.) The stake bump: store it too, or re-derive with `find_program_address` (costs CU but fine at k≈17). Prefer storing `nullifier_hash` + re-deriving the bump via `find_program_address` for simplicity.
2. **`recipient` as an AccountInfo for `StakeAction`.** `intent.recipient` is a `Pubkey` (the authority is CPI *data*, not a passed account). `StakeAction` only needs the recipient's key for `Authorize`/`Initialize` data — so pass an `AccountInfo` whose `.key` is `intent.recipient`. Since the participant's key is not a passed account, either (a) require the caller to pass the recipient as a read-only account in the triple (making it `[intent, stake_account, recipient, relayer]×count` = 4/intent, k≈13), OR (b) change `StakeAction` to take `recipient: Pubkey` instead of `AccountInfo` (cleaner — the recipient is never a CPI account, only data). **Choose (b):** make `StakeAction.recipient: Pubkey`; keep the triple at 3 accounts (`[intent, stake_account, relayer]`), preserving k≈17. Update Task 2's `StakeAction` accordingly (recipient is `Pubkey`, used in `Authorized`/`authorize` data).

- [ ] **Step 3: Reconcile Task 2 (recipient: Pubkey) + Intent.nullifier_hash**

Apply note (2): change `StakeAction.recipient` to `Pubkey` and use it directly in `Authorized { withdrawer: self.recipient }` and `authorize(..., &self.recipient, ...)`. Apply note (1): add `nullifier_hash: [u8;32]` to `Intent` (round.rs, `SPACE += 32`), set it in `commit_intent` (it already has `nullifier_hash` in scope), and derive the stake PDA + bump in `execute_round` via `find_program_address`.

- [ ] **Step 4: Build, run, verify**

Run: `cargo build-sbf ...` then `cargo test -p pool-program --test stake_round`
Expected: PASS (happy path + all adversarial). Then `cargo test -p pool-program` overall green (withdraw suite unchanged — the seam-regression proof). Print the stake-round CU.

- [ ] **Step 5: Lint + commit**
```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add programs/pool-program/src programs/pool-program/tests/stake_round.rs programs/pool-program/tests/round_support.rs
git commit -m "feat(pool-program): execute_round dispatches pooled Stake (per-kind remaining_accounts + StakeAction)"
```

---

## Task 4: SDK stake builders + e2e + cancel-on-stake + seam regression

Give clients the stake round, prove the full deposit→commit(k)→stake-execute trip through the SDK, and confirm `cancel_intent` + the whole withdraw suite still hold.

**Files:** Modify `crates/sdk/src/lib.rs`. Modify `crates/sdk/tests/e2e.rs`. Test: `crates/sdk/tests/e2e.rs`, and a `cancel_intent`-on-stake case (in `stake_round.rs` or `cancel_intent.rs`).

**Interfaces:**
- Produces: `sdk::stake_account_pda(pool, nullifier_hash)`; `build_initialize_pool_ix` stake variant (Task 1 already added args — confirm); `build_execute_round_ix` per-kind `remaining_accounts` assembly (append stake PDAs + the shared tail for stake pools).

- [ ] **Step 1: SDK helpers + per-kind execute builder**

In `crates/sdk/src/lib.rs`:
```rust
pub fn stake_account_pda(pool: Pubkey, nullifier_hash: [u8; 32]) -> Pubkey {
    Pubkey::find_program_address(
        &[b"stake", pool.as_ref(), nullifier_hash.as_ref()], &pool_program::ID).0
}
```
Extend `build_execute_round_ix` (or add `build_execute_stake_round_ix`) to assemble the stake layout: for each intent `[intent_pda, stake_account_pda, relayer]`, then append the shared tail `[validator, stake_program (solana_sdk::stake::program::ID), stake_config (stake::config::ID), clock (sysvar), stake_history (sysvar), rent (sysvar)]` — all read-only except the stake accounts. Keep the withdraw builder path unchanged.

- [ ] **Step 2: SDK e2e — a pooled-stake round trip (write, fail, implement, pass)**

Add to `crates/sdk/tests/e2e.rs` a `sdk_driven_stake_round` test: set up a vote account; `build_initialize_pool_ix(..., action_kind=1, validator=V, stake_fee=F)`; deposit 2 notes; `build_commit_intent_ix` for each (the extDataHash now binds the stake authority — same call, `recipient` = the authority); `build_execute_round_ix` (stake layout); assert both stake PDAs are delegated to `V` with authorities = their recipients. Reuse the `e2e.rs` helpers (`send`, `ensure_build_artifacts`, `so_path`).

Run: `cargo test -p sdk --test e2e`
Expected: FAIL then PASS.

- [ ] **Step 3: cancel_intent on a stake pool**

Add a test (in `stake_round.rs`) that commits one stake intent (k_floor=2, round stays Open), then `cancel_intent` with the recipient signing → the denomination is refunded to the recipient, `intent_count` decrements, the intent PDA closes, and the nullifier stays burned (re-commit fails). This confirms cancel is generic across action kinds (no stake account was created yet — cancel is pre-execute).

- [ ] **Step 4: Full workspace green + commit**

Run: `cargo test -p pool-program`, `cargo test -p sdk`, `cargo test --workspace` — all green (the **withdraw suite unchanged** = seam regression proven; stake round + e2e + cancel-on-stake pass). `cargo fmt` + `cargo clippy --all-targets -- -D warnings` clean.
```bash
git add crates/sdk programs/pool-program/tests
git commit -m "feat(sdk): pooled-stake builders + e2e; cancel_intent works on stake pools"
```

---

## Self-Review

**1. Spec coverage** (`docs/superpowers/specs/2026-07-16-pooled-stake-design.md`):
- §1/§2 pooled stake + generic seam → Tasks 1-3 (`ActionKind::Stake`, `StakeAction`, per-kind `execute_round`).
- §2.1 the **4-CPI order** (Create → Initialize[staker=vault, withdrawer=recipient] → DelegateStake[vault signs] → Authorize[staker→recipient]) → Task 2 `StakeAction`, exactly (the reviewed signer fix).
- §2.2 account budget (3/intent + 6 shared → k≈17) → Task 3's stake layout `count*3 + 6`.
- §6 value split `delegated = denomination − stake_fee − stake_rent` + the `≥ MIN_STAKE_DELEGATION` validity floor → `invariants::stake_split` (Task 1) + `initialize_pool` validity + `DelegateStake` at execute.
- §6 fixed per-pool fee (byte-amount uniformity) → `Pool.stake_fee` + init validity; `commit_intent`'s `fee == pool.stake_fee` on stake pools (add this `require!` in Task 1's init/commit — note: `commit_intent` currently allows any `fee ≤ denomination`; for a **stake** pool it must equal `pool.stake_fee`; add that guard and a test).
- §4 threat deltas (authority redirection, non-uniform batch, sub-k, whale-self-fill residual) → Task 3 adversarial tests + the honest residual (documented, not tested — it's a stated tradeoff).
- §5 testing (happy + adversarial + cancel + **withdraw seam regression**) → Tasks 3-4.

**2. Placeholder scan:** Task 3 Step 2 intentionally leaves two `/* ... */` decisions (the `nullifier_hash` source + `recipient: Pubkey` vs account) and resolves them explicitly in Step 3 with the chosen answer (add `Intent.nullifier_hash`; make `StakeAction.recipient: Pubkey`) — these are directed decisions, not open placeholders. Task 1 Step 7 and Task 3 Step 1 describe test bodies in prose with the exact asserts; the implementer writes them in full using the named existing helpers.

**3. Type consistency:** `ActionKind::Stake`, `Pool.{action_kind:u8, validator:Pubkey, stake_fee:u64}`, `StakeAction` (recipient: `Pubkey` per the Step-3 reconciliation), `stake_split(u64,u64,u64)->Result<(u64,u64)>`, the stake `remaining_accounts` shape (`[intent, stake_account, relayer]×count` + 6-tail), and the stake PDA seeds `["stake", pool, nullifier_hash]` are used identically across `round.rs`, `action.rs`, `lib.rs`, the tests, and the SDK. `Intent` gains `nullifier_hash` (Task 3 Step 3) — every `Intent::SPACE` / constructor site updated in the same step.

**Deferred (out of scope, honestly):** pooled un-stake / reward claim (incentive module), pooled swap (account envelope + chunked executor), multi-action pools, incentive/bonding (the whale-self-fill residual is stated), the effective-k harness (Plan 6), and the ~1.003-SOL stake-pool crowd-depth floor (a production concern, fine for the bounty demo — recorded in the spec §6).
