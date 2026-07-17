# Pool.fee uniformity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generalise the stake-only `stake_fee` into one mandatory pool-wide `fee`, enforced uniformly for BOTH action kinds at commit AND execute — closing the shipped withdraw-pool payout-amount fingerprint.

**Architecture:** A pure rename `Pool.stake_fee → Pool.fee` (byte-identical `u64`, no layout change), one unconditional `require!(fee == pool.fee, FeeNotUniform)` at commit that subsumes the old per-intent bound, a symmetric execute-time defense-in-depth check added to the withdraw arm (the stake arm already has one), a per-kind init bound, and a new appended `FeeNotUniform` error. No new PDA, circuit, or pure fn.

**Tech Stack:** Rust 2021, Anchor 0.31.1, LiteSVM 0.6.1.

## Global Constraints

- **Anchor 0.31.1 / Rust 2021.** `cargo fmt` + `cargo clippy --all-targets -- -D warnings` clean before every commit. `overflow-checks` on. Conventional commits, one logical change each.
- **Error variants are APPEND-ONLY.** `FeeNotUniform` goes AFTER the current last variant `CancelTooEarly` — never insert/reorder (`deposit.rs` hardcodes codes 6001/6002).
- **Fail-closed.** Typed `PoolError`; no `unwrap`/`expect`/`panic!` on attacker input.
- **Uniformity is the product — enforce it IDENTICALLY on every `PooledAction` adapter.** Both the stake AND withdraw execute arms re-assert `intent.fee == pool.fee` (Plan 5's banked lesson: "the account executes" is not the invariant, "it executes identically" is).
- **The field change is a PURE RENAME** (`stake_fee: u64 → fee: u64`, same offset): `size_of::<Pool>()` unchanged, the size assert holds, the instruction wire-format is byte-identical. **No migration.**
- **HONEST-CLAIMS-ONLY (spec ceiling — hand this to reviewers verbatim; it is not modesty).** Any comment/doc must claim ONLY: this closes the *settlement payout-amount* fingerprint. It must NOT claim it closes the commit-time priority-fee/CU-price fingerprint (advisory, unconstrainable on-chain), and must NOT present it as a crowd-depth / real-k mechanism — it is a **nominal-cost anti-Sybil tax** (`k_∞` unmoved). Adversary = the passive on-chain observer.
- **Both the withdraw suite AND the stake suite stay green.** The withdraw fixtures MUST be updated so `pool.fee == the committed intent fee` (see Task 1 Step 5) — else the uniformity guard rejects every existing withdraw intent.
- **Record the measured withdraw-round CU** (like Plan 4/5's 26,247 / 56,800 figures) to confirm the added `require!` leaves `k` unaffected.
- A change isn't done until `cargo test -p pool-program` / `cargo test --workspace` is green and you've said so with the output.

---

## File Structure

- `programs/pool-program/src/lib.rs` — *modify.* `FeeNotUniform` variant; `initialize_pool` (param rename + D3 init bound + field set); `commit_intent` (one unconditional check); `execute_round` (rename the `fee` local, change the stake-arm error, ADD the withdraw-arm check); `WrongActionConfig` message text.
- `programs/pool-program/src/state.rs` — *modify.* `stake_fee` → `fee` (field + the layout comment).
- `programs/pool-program/src/invariants.rs` — *modify (optional).* `stake_split`'s `stake_fee` param name (internal; no behaviour change).
- `programs/pool-program/tests/round_support.rs` — *modify.* Withdraw fixtures init with `fee = FEE`; rename stake fixtures' `stake_fee` args.
- `programs/pool-program/tests/{commit_intent.rs, execute_round.rs, initialize_pool.rs}` — *modify.* Rename; update the stake fee-mismatch tests' expected error to `FeeNotUniform`; ADD withdraw uniformity tests.
- `crates/sdk/src/lib.rs` — *modify.* `build_initialize_pool_ix` `stake_fee` param → `fee`; the offset-test label. (Task 2)
- `crates/sdk/tests/e2e.rs` — *modify.* The withdraw e2e inits its pool with `fee = FEE`. (Task 2)

**Interface names (verbatim):** `Pool.fee: u64`; `PoolError::FeeNotUniform`; `initialize_pool(denomination: u64, k_floor: u16, action_kind: u8, validator: Pubkey, fee: u64)`; the commit guard `require!(fee == pool.fee, PoolError::FeeNotUniform)`; the execute guard (both arms) `require!(intent.fee == fee, PoolError::FeeNotUniform)`.

---

## Task 1: Program change + program tests + fixture fee-matching

The whole on-chain change (an atomic rename + the new checks) with its tests. Because a field rename is compile-breaking, this task lands as one cohesive change; the NEW withdraw tests are written to genuinely exercise the added guards (non-tautological).

**Files:** Modify `src/state.rs`, `src/lib.rs`, `src/invariants.rs`; `tests/round_support.rs`, `tests/commit_intent.rs`, `tests/execute_round.rs`, `tests/initialize_pool.rs`.

**Interfaces:**
- Produces: `Pool.fee`, `PoolError::FeeNotUniform`, the unified commit/execute fee guards, the `initialize_pool(… fee)` signature.

- [ ] **Step 1: Append the `FeeNotUniform` error variant**

In `src/lib.rs`, in `#[error_code] pub enum PoolError`, append AFTER the current last variant `CancelTooEarly`:
```rust
    #[msg("intent fee does not equal the pool's uniform fee")]
    FeeNotUniform,
```

- [ ] **Step 2: Rename the `Pool` field (pure rename)**

In `src/state.rs`, rename `pub stake_fee: u64,` → `pub fee: u64,` and update the adjacent layout comment (drop "stake_fee" → "fee"). The offset, `size_of::<Pool>()`, and the size assert are unchanged.

- [ ] **Step 3: `initialize_pool` — param rename + D3 init bound + field set**

In `src/lib.rs` `initialize_pool`: rename the `stake_fee: u64` param → `fee: u64`. Replace the withdraw validation and update the stake call + field set:
```rust
        match action_kind {
            0 => {
                // Withdraw pools carry no validator; the pool-wide fee must fit the denomination.
                require!(validator == Pubkey::default(), PoolError::WrongActionConfig);
                require!(fee <= denomination, PoolError::FeeExceedsDenomination);
            }
            1 => {
                require!(validator != Pubkey::default(), PoolError::WrongActionConfig);
                let stake_rent =
                    Rent::get()?.minimum_balance(crate::invariants::STAKE_ACCOUNT_SIZE);
                // Fails closed if denomination can't cover fee + rent + min delegation.
                crate::invariants::stake_split(denomination, fee, stake_rent)?;
            }
            _ => return err!(PoolError::WrongActionConfig),
        }
```
And the field set: `pool.stake_fee = stake_fee;` → `pool.fee = fee;`.

- [ ] **Step 4: `commit_intent` — one unconditional uniformity check**

In `src/lib.rs` `commit_intent`, replace the per-intent bound AND the stake-only branch:
```rust
            require!(fee <= pool.denomination, PoolError::FeeExceedsDenomination);
            // Stake pools require a uniform, pool-fixed fee (privacy + liveness — see note).
            if pool.action_kind == 1 {
                require!(fee == pool.stake_fee, PoolError::WrongActionConfig);
            }
            pool.action_kind
```
with the single unconditional check (the `fee <= denomination` bound is subsumed — `pool.fee` was validated ≤ denomination at init):
```rust
            // Uniform pool-wide fee for BOTH action kinds: a variable fee is a payout-amount
            // fingerprint (settlement side), and fee=0 on withdraw is free self-fill.
            require!(fee == pool.fee, PoolError::FeeNotUniform);
            pool.action_kind
```

- [ ] **Step 5: `execute_round` — rename the local, change the stake error, ADD the withdraw check**

In `src/lib.rs` `execute_round`:
1. In the top field destructure, rename `stake_fee` → `fee` (both the binding and `pool.stake_fee` → `pool.fee`). The local is now in scope for both dispatch arms.
2. **Stake arm** (the existing defense-in-depth): change
   `require!(intent.fee == stake_fee, PoolError::WrongActionConfig);` →
   `require!(intent.fee == fee, PoolError::FeeNotUniform);` and update its comment to say `== pool.fee`.
3. **Withdraw arm — ADD the symmetric check.** Immediately before `let action = crate::action::WithdrawAction {`, insert (mirroring the stake arm, using the in-scope `fee` local):
```rust
                    // Defense-in-depth: fee was fixed at commit (== pool.fee), so payouts are
                    // uniform across the round. Re-assert so a stale/forged intent can't slip a
                    // non-uniform amount into the batch — uniformity is enforced IDENTICALLY on
                    // every PooledAction adapter, withdraw included.
                    require!(intent.fee == fee, PoolError::FeeNotUniform);
```

- [ ] **Step 6: Update the `WrongActionConfig` message + `stake_split` param**

In `src/lib.rs`, the `WrongActionConfig` `#[msg(...)]`: drop "stake_fee" → `#[msg("action_kind/validator/fee configuration is invalid for this pool")]`.
In `src/invariants.rs`, optionally rename `stake_split`'s `stake_fee` parameter → `fee` (internal; update its doc comment's "stake_fee" mentions). No behaviour change.

- [ ] **Step 7: Fix the withdraw fixtures (the breaking ripple) + rename stake fixtures**

In `tests/round_support.rs`: the two withdraw fixtures hand-build `initialize_pool` data with the fee bytes set to `0` while committing intents with `fee: FEE` (=1_000). Set the pool's fee to `FEE` so `fee == pool.fee` holds — in each withdraw `initialize_pool` data blob, encode `FEE.to_le_bytes()` where the `stake_fee`/`fee` field is written (the trailing 8 bytes after `action_kind(0) ‖ validator(default 32)`). Rename the stake fixture's `stake_fee` params/locals → `fee` where it aids clarity (behaviour unchanged — stake intents already commit `fee == stake_fee`).

Verify by reading: after this step, every withdraw fixture's `pool.fee` equals the `FEE` its intents commit with.

- [ ] **Step 8: Update existing tests + ADD withdraw uniformity tests**

Update (rename + expected-error):
- The stake fee-mismatch tests (`commit_intent_rejects_wrong_stake_fee`, `execute_round_stake_rejects_wrong_fee`) now expect **`FeeNotUniform`** instead of `WrongActionConfig` (the D1 change). Rename `stake_fee` locals → `fee` as convenient.
- `initialize_withdraw_pool_rejects_stake_params` (in `initialize_pool.rs`): mechanical — rename the trailing `stake_fee = 0` arg → `fee = 0`; the assertion (nonzero validator → `WrongActionConfig`) is unchanged (confirmed: it exercises only the validator half).

ADD (in `commit_intent.rs` / `execute_round.rs` / `initialize_pool.rs`, using the existing withdraw fixtures + hand-built ix helpers):
- **Commit-time reject:** on a withdraw pool with `pool.fee = FEE`, commit one intent with `fee = FEE + 1` → rejected with `FeeNotUniform` (assert the specific error). A matching-fee commit succeeds.
- **Execute-time payout uniformity:** after `execute_round` on a k≥2 withdraw pool, assert every recipient received the **identical** `denomination − pool.fee` (read each recipient balance) — the direct amount-uniformity assertion.
- **Execute-time defense-in-depth (fix-B):** forge a withdraw `Intent` PDA directly with `fee ≠ pool.fee` (bypassing `commit_intent`) and pass it to `execute_round` → rejected with `FeeNotUniform`. **Template: `execute_round_stake_rejects_wrong_fee`** — copy its Intent-forging setup (it writes an `Intent` account with a mismatched fee the same way); this is the test that proves the newly-added withdraw-arm `require!` fires.
- **Init store/enforce/reject:** `initialize_pool` for a withdraw pool with nonzero `fee ≤ denomination` stores it (offset/deserialize read) and `commit_intent` enforces it; `fee > denomination` → `FeeExceedsDenomination`.

- [ ] **Step 9: Build, run, measure CU, lint, commit**

Run: `cargo build-sbf --manifest-path programs/pool-program/Cargo.toml` then `cargo test -p pool-program` — all green (the withdraw suite passes with the fixed fixtures; the stake suite passes with the renamed error; the new withdraw uniformity tests pass). **Record the measured k≥2 withdraw-round CU** (print it in the execute test, as Plan 4/5 did) and note it in the report — confirm it is essentially unchanged (the added `require!` is negligible; `k` is unaffected). `cargo fmt` + `cargo clippy --all-targets -- -D warnings` clean.
```bash
git add programs/pool-program/src programs/pool-program/tests
git commit -m "feat(pool-program): one pool-wide uniform fee for both action kinds (FeeNotUniform)"
```

---

## Task 2: SDK rename + e2e fee-match + workspace green

Rename the client-facing builder param, keep the wire-format byte-identical, and fix the withdraw e2e's pool fee so it matches its commit fee.

**Files:** Modify `crates/sdk/src/lib.rs`, `crates/sdk/tests/e2e.rs`.

**Interfaces:**
- Consumes: the `initialize_pool(… fee)` wire-format (byte-identical to before — only the param name changed).

- [ ] **Step 1: Rename the builder param + offset test label**

In `crates/sdk/src/lib.rs`, `build_initialize_pool_ix`: rename the `stake_fee: u64` param → `fee: u64` and the local it encodes. The instruction data layout is unchanged (`… ‖ validator(32) ‖ fee(8)`), so the encoding line is byte-identical. In the `initialize_pool` offset test, relabel the `data[51..59]` assertion `"stake_fee"` → `"fee"` (the bytes/offset are unchanged).

- [ ] **Step 2: Fix the withdraw e2e's pool fee**

In `crates/sdk/tests/e2e.rs`, the withdraw round-trip test inits its pool via `build_initialize_pool_ix(...)` and commits intents with `FEE` (asserting `payout = DENOMINATION - FEE`). Pass `fee = FEE` to `build_initialize_pool_ix` (where it currently passes `0`/`stake_fee`) so `commit_intent`'s uniformity guard accepts the intents. The payout assertions (`DENOMINATION - FEE` to recipient, `FEE` to relayer) are unchanged. The stake e2e already commits `fee == stake_fee`; just rename the arg.

- [ ] **Step 3: Full workspace green + lint + commit**

Run: `cargo test -p sdk` then `cargo test --workspace` — all green (the withdraw e2e passes with the matched fee; the stake e2e unchanged; the whole withdraw + stake suites green = the seam-regression proof). `cargo fmt` + `cargo clippy --all-targets -- -D warnings` clean.
```bash
git add crates/sdk
git commit -m "feat(sdk): rename initialize_pool stake_fee arg to fee; match withdraw e2e pool fee"
```

---

## Self-Review

**1. Spec coverage** (`docs/superpowers/specs/2026-07-17-pool-fee-uniformity-design.md`):
- `FeeNotUniform` appended (D1) → Task 1 Step 1.
- `stake_fee → fee` pure rename (D4) → Task 1 Step 2 (state) + Steps 3/5/6 (lib) + Task 2 (sdk).
- Init bound (D3), withdraw `fee <= denomination`, stake via `stake_split` → Task 1 Step 3.
- One unconditional commit check (D2), subsuming the per-intent bound → Task 1 Step 4.
- Execute-time check on BOTH arms (fix B) — stake error changed, withdraw check ADDED → Task 1 Step 5.
- `WrongActionConfig` message + `stake_split` param → Task 1 Step 6.
- Withdraw uniformity tests incl. the crafted-intent fix-B test + CU measurement → Task 1 Steps 8–9.
- SDK rename + wire-compat + e2e → Task 2.
- Honest-claims ceiling: the only prose added is the two guard comments (Steps 4, 5), which stay at the ceiling (payout-amount fingerprint / nominal tax) — no priority-fee or crowd-depth claim.

**2. Placeholder scan:** No TBD/TODO. The forged-Intent test points at a concrete existing template (`execute_round_stake_rejects_wrong_fee`) rather than describing forging abstractly. Fixture edits name the exact byte position.

**3. Type consistency:** `Pool.fee: u64`, `PoolError::FeeNotUniform`, `initialize_pool(… fee: u64)`, the commit guard `fee == pool.fee`, and the execute guard `intent.fee == fee` are used identically across `state.rs`, `lib.rs`, the tests, and the SDK. The wire-format is byte-identical, so no offset changes anywhere.

**4. The breaking ripple is covered:** Task 1 Step 7 (program fixtures) and Task 2 Step 2 (SDK e2e) update every withdraw pool's fee to match its committed intent fee — without which the uniformity guard would reject every existing withdraw intent. This is the difference between a green and a red withdraw suite.
