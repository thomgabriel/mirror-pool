# Timeout-gated cancel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Gate `cancel_intent` so a committed intent is uncancelable for `TIMEOUT_SLOTS` slots after it was committed, turning cancel from an instant sub-k exit into a liveness valve for rounds that failed to fill.

**Architecture:** A pure `cancel_unlock_slot(committed_slot) -> Result<u64>` in `invariants.rs` (host-tested, incl. the overflow branch). `Intent` gains a `committed_slot: u64` set in `commit_intent` from the Clock; `cancel_intent` reads it, computes the unlock slot via the pure fn, and requires the current slot has reached it (`CancelTooEarly` otherwise). No change to `execute_round`, `Pool`, circuits, or SDK.

**Tech Stack:** Rust 2021, Anchor 0.31.1, LiteSVM 0.6.1 (`warp_to_slot` sets the Clock sysvar's slot).

## Global Constraints

- **Anchor 0.31.1 / Rust 2021.** `cargo fmt` + `cargo clippy --all-targets -- -D warnings` clean before every commit. `overflow-checks` on. Conventional commits, one logical change each.
- **Error variants are APPEND-ONLY.** `CancelTooEarly` goes AFTER the current last variant `StakeAccountInvalid` — never insert/reorder (hardcoded codes 6001/6002 in `deposit.rs` depend on it).
- **Fail-closed.** The unlock arithmetic uses `checked_add`; overflow returns `CancelTooEarly` (cannot cancel). No `unwrap()`/`expect()`/`panic!` on attacker-influenced input.
- **Invariant logic lives in a pure `pub fn` with host unit tests** — `cancel_unlock_slot` goes in `invariants.rs` (the `meets_k_floor`/`stake_split` precedent), so the overflow branch is coverage-visible (SBF in-VM lines aren't measured).
- **`N` = `TIMEOUT_SLOTS: u64 = 9_000`** (~1h at 400 ms/slot). A judgment call whose security is workload-contingent; the const carries the promote-to-per-pool-config caveat (already true for stake vs withdraw fill horizons).
- **Reference = per-intent `committed_slot`, unit = slots** (not round-level, not unix timestamp) — see spec §"Design decisions".
- **Honest-claims-only.** Cancel remains a linkable sub-k exit by construction; this gate only removes the instant version, makes "committed for N slots" enforced, and raises sybil-yank cost. Do not add code or comments claiming more.
- **Tests are Rust-native** (host unit + LiteSVM). The withdraw suite AND `execute_round` stay green (the gate touches only cancel). Existing cancel tests are updated to warp — that IS the seam-regression proof that refund/close/nullifier semantics are otherwise unchanged.
- A change isn't done until `cargo test -p pool-program` is green and you've said so with the output.

---

## File Structure

- `programs/pool-program/src/invariants.rs` — *modify.* Add `pub const TIMEOUT_SLOTS: u64` + `pub fn cancel_unlock_slot(committed_slot: u64) -> Result<u64>` + host tests.
- `programs/pool-program/src/lib.rs` — *modify.* Append `CancelTooEarly` to `PoolError`; set `intent.committed_slot` in `commit_intent`; add the gate to `cancel_intent`.
- `programs/pool-program/src/round.rs` — *modify.* `Intent` gains `committed_slot: u64` (`SPACE` 121 → 129).
- `programs/pool-program/tests/cancel_intent.rs` — *modify.* Two new gate-direction tests; warp the existing successful-cancel test.
- `programs/pool-program/tests/stake_round.rs` — *modify.* Warp the existing `cancel_intent_works_on_stake_pool` test.

**Interface names (verbatim):** `invariants::TIMEOUT_SLOTS: u64` (= 9_000); `invariants::cancel_unlock_slot(committed_slot: u64) -> Result<u64>`; `round::Intent.committed_slot: u64`; `PoolError::CancelTooEarly`.

---

## Task 1: Pure `cancel_unlock_slot` + `TIMEOUT_SLOTS` + `CancelTooEarly`

The host-testable arithmetic core, with the overflow branch under a host unit test. Appends the error variant the pure fn (and Task 2's gate) return.

**Files:** Modify `programs/pool-program/src/invariants.rs` and `programs/pool-program/src/lib.rs` (error variant only). Test: `invariants.rs` host tests.

**Interfaces:**
- Produces: `invariants::TIMEOUT_SLOTS: u64` (= 9_000), `invariants::cancel_unlock_slot(committed_slot: u64) -> Result<u64>`, `PoolError::CancelTooEarly`.

- [ ] **Step 1: Append the error variant**

In `programs/pool-program/src/lib.rs`, in the `#[error_code] pub enum PoolError`, append AFTER the current last variant `StakeAccountInvalid`:
```rust
    #[msg("intent cannot be cancelled until its commit timeout has elapsed")]
    CancelTooEarly,
```

- [ ] **Step 2: Write the failing host tests**

In `programs/pool-program/src/invariants.rs`, add to the existing `#[cfg(test)] mod` (or a new `#[cfg(test)] mod cancel_tests { use super::*; ... }` — match the file's existing test-module style):
```rust
#[test]
fn cancel_unlock_slot_adds_timeout() {
    assert_eq!(cancel_unlock_slot(0).unwrap(), TIMEOUT_SLOTS);
    assert_eq!(cancel_unlock_slot(1_000).unwrap(), 1_000 + TIMEOUT_SLOTS);
}

#[test]
fn cancel_unlock_slot_overflow_fails_closed() {
    // committed_slot so large that +TIMEOUT_SLOTS overflows u64 → cannot cancel.
    assert!(cancel_unlock_slot(u64::MAX).is_err());
    assert!(cancel_unlock_slot(u64::MAX - TIMEOUT_SLOTS + 1).is_err());
    // exactly representable boundary still succeeds:
    assert_eq!(cancel_unlock_slot(u64::MAX - TIMEOUT_SLOTS).unwrap(), u64::MAX);
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p pool-program --lib cancel_unlock_slot`
Expected: FAIL — `cannot find function cancel_unlock_slot` / `cannot find value TIMEOUT_SLOTS`.

- [ ] **Step 4: Implement the const + pure fn**

In `programs/pool-program/src/invariants.rs` (top-level, alongside the other consts/fns — match the surrounding style; `use anchor_lang::prelude::*;` is already in scope as `split_payout`/`stake_split` use `error!`/`Result`):
```rust
/// Slots a committed intent stays uncancelable, counted from its own commit.
/// ~1h at 400 ms/slot. A workload-contingent judgment call, not a derived number:
/// it means anything only if it is >= a credible fill horizon so that "the round
/// failed" is plausible by the time cancel opens. Promote to a bounded per-pool
/// config when fill horizons diverge (already true for stake vs withdraw); kept a
/// const here to avoid unused config surface.
pub const TIMEOUT_SLOTS: u64 = 9_000;

/// Earliest slot at which an intent committed at `committed_slot` may be cancelled.
/// Fails closed on overflow (cannot cancel) rather than wrapping.
pub fn cancel_unlock_slot(committed_slot: u64) -> Result<u64> {
    committed_slot
        .checked_add(TIMEOUT_SLOTS)
        .ok_or(error!(crate::PoolError::CancelTooEarly))
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p pool-program --lib cancel_unlock_slot`
Expected: PASS (2 tests).

- [ ] **Step 6: Lint + commit**

Run: `cargo fmt` + `cargo clippy --all-targets -- -D warnings` (clean — the Global Constraint scope).
```bash
git add programs/pool-program/src/invariants.rs programs/pool-program/src/lib.rs
git commit -m "feat(pool-program): cancel_unlock_slot pure fn + TIMEOUT_SLOTS + CancelTooEarly"
```

---

## Task 2: Wire the gate into `commit_intent` / `cancel_intent` + LiteSVM tests

Record each intent's commit slot, gate cancel on it, and prove both directions in LiteSVM by warping the Clock. Update the existing successful-cancel tests to warp (the seam-regression proof).

**Files:** Modify `programs/pool-program/src/round.rs`, `programs/pool-program/src/lib.rs`, `programs/pool-program/tests/cancel_intent.rs`, `programs/pool-program/tests/stake_round.rs`.

**Interfaces:**
- Consumes: `invariants::cancel_unlock_slot`, `invariants::TIMEOUT_SLOTS`, `PoolError::CancelTooEarly` (Task 1).
- Produces: `round::Intent.committed_slot: u64`.

- [ ] **Step 1: Write the failing gate-direction tests**

In `programs/pool-program/tests/cancel_intent.rs`, add two tests. They reuse the file's existing helpers **exactly as `cancel_intent_refunds_and_decrements` does** — read that test first for the precise shapes:
- `build_round_fixture_signer_recipients(2, 1) -> (RoundFixture, Vec<Keypair>)`,
- `commit_intent_tx(&fx, i, round_id)` (must be sent to actually commit the intent on-chain — the fixture only deposits + builds proofs),
- `cancel_ix(fx, i, round_id, recipient_pubkey) -> Instruction` (note the arity: `i` and `round_id` are separate args), which is then wrapped in a `Message` with a `ComputeBudgetInstruction::set_compute_unit_limit(400_000)` prefix and a `Transaction` signed by `[&fx.payer, recipient]`.

Both tests commit at a **nonzero base slot** so the assertion has teeth: if `commit_intent` failed to write `committed_slot` (left it 0), the unlock would be `TIMEOUT_SLOTS` and `cancel_rejected_before_timeout` would wrongly succeed — so the nonzero base makes the test non-tautological. `cancel_intent.rs` already imports `AccountDeserialize`, `ReadableAccount`, `Message`, `Transaction`, `ComputeBudgetInstruction`, `Keypair`, `Signer`; add only `use solana_sdk::clock::Clock;` and `use pool_program::round::Intent;` (the file currently imports only `Round`).
```rust
const BASE_SLOT: u64 = 10_000; // nonzero, so an unwritten committed_slot (0) fails the reject test

fn cancel_tx(fx: &round_support::RoundFixture, recipient: &Keypair) -> Transaction {
    let ix = cancel_ix(fx, 0, 0, recipient.pubkey());
    let msg = Message::new(
        &[ComputeBudgetInstruction::set_compute_unit_limit(400_000), ix],
        Some(&fx.payer.pubkey()),
    );
    Transaction::new(&[&fx.payer, recipient], msg, fx.svm.latest_blockhash())
}

#[test]
fn cancel_rejected_before_timeout() {
    let (mut fx, recipients) = build_round_fixture_signer_recipients(2, 1);
    fx.svm.warp_to_slot(BASE_SLOT);
    fx.svm.send_transaction(commit_intent_tx(&fx, 0, 0)).unwrap();

    // The intent recorded committed_slot == BASE_SLOT. One slot before unlock → locked.
    fx.svm
        .warp_to_slot(BASE_SLOT + pool_program::invariants::TIMEOUT_SLOTS - 1);
    fx.svm.expire_blockhash();
    let err = fx
        .svm
        .send_transaction(cancel_tx(&fx, &recipients[0]))
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("CancelTooEarly"),
        "expected CancelTooEarly before the timeout, got: {err:?}"
    );
}

#[test]
fn cancel_allowed_at_timeout() {
    let (mut fx, recipients) = build_round_fixture_signer_recipients(2, 1);
    fx.svm.warp_to_slot(BASE_SLOT);
    fx.svm.send_transaction(commit_intent_tx(&fx, 0, 0)).unwrap();

    // Prove commit_intent wrote the current clock slot (not a default).
    let acct = fx.svm.get_account(&fx.intents[0].intent_pda).unwrap();
    let intent = Intent::try_deserialize(&mut acct.data()).unwrap();
    assert_eq!(intent.committed_slot, BASE_SLOT, "commit_intent records the clock slot");

    // Exactly at the unlock slot → cancelable.
    fx.svm
        .warp_to_slot(BASE_SLOT + pool_program::invariants::TIMEOUT_SLOTS);
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(cancel_tx(&fx, &recipients[0]))
        .expect("cancel must succeed once the timeout has elapsed");
    // Intent PDA closed (rent-swept) confirms the refund/close path ran.
    assert!(
        fx.svm.get_account(&fx.intents[0].intent_pda).is_none(),
        "intent PDA closed on cancel"
    );
}
```
(`Message`, `Transaction`, `ComputeBudgetInstruction` are already imported in `cancel_intent.rs` for the existing test; add only the `Clock`/`Intent` uses noted above.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p pool-program --test cancel_intent cancel_rejected_before_timeout cancel_allowed_at_timeout`
Expected: FAIL — `committed_slot` is not a field of `Intent` yet, so the file does not compile (and, once it does, `cancel_rejected_before_timeout` would see a successful cancel with no gate).

- [ ] **Step 3: Add `committed_slot` to `Intent`**

In `programs/pool-program/src/round.rs`, add the field to `Intent` (after `action`) and bump `SPACE`:
```rust
pub struct Intent {
    pub pool: Pubkey,
    pub round_id: u64,
    pub recipient: Pubkey,
    pub relayer: Pubkey,
    pub fee: u64,
    pub action: ActionKind,
    pub committed_slot: u64,
}

impl Intent {
    pub const SPACE: usize = 8 + 32 + 8 + 32 + 32 + 8 + 1 + 8;
}
```

- [ ] **Step 4: Set `committed_slot` in `commit_intent`**

In `programs/pool-program/src/lib.rs`, in `commit_intent`, immediately after the `intent.action = match action_kind { ... };` block, add:
```rust
        intent.committed_slot = Clock::get()?.slot;
```
(`Clock::get()` is available via `anchor_lang::prelude::*`, same `Sysvar::get()` pattern as the existing `Rent::get()` in the stake path.)

- [ ] **Step 5: Add the gate to `cancel_intent`**

In `programs/pool-program/src/lib.rs`, in `cancel_intent`, immediately after the existing `RoundClosed` round-Open `require!` and before the vault/seed setup, add:
```rust
        // Timeout gate: a committed intent is uncancelable until TIMEOUT_SLOTS
        // slots after its own commit. Removes the instant commit->cancel exit and
        // makes "committed for N slots" enforced; cancel remains a linkable sub-k
        // exit by construction once the window opens (see the spec's claim list).
        let unlock = crate::invariants::cancel_unlock_slot(ctx.accounts.intent.committed_slot)?;
        require!(Clock::get()?.slot >= unlock, PoolError::CancelTooEarly);
```

- [ ] **Step 6: Run the new tests to verify they pass**

Run: `cargo test -p pool-program --test cancel_intent cancel_rejected_before_timeout cancel_allowed_at_timeout`
Expected: PASS (2 tests).

- [ ] **Step 7: Warp the existing successful-cancel tests**

Two existing tests cancel immediately and now must warp past the timeout first. The base-independent warp is: read the commit slot, then warp `TIMEOUT_SLOTS` beyond it. Insert it **after the intent is committed and before the cancel transaction is built** (so `latest_blockhash()` is read after any `expire_blockhash`). Add `use solana_sdk::clock::Clock;` to each file if not already imported.

- `programs/pool-program/tests/cancel_intent.rs`, `cancel_intent_refunds_and_decrements`: immediately after `fx.svm.send_transaction(commit_intent_tx(&fx, 0, 0)).unwrap();`, add:
  ```rust
      let committed = fx.svm.get_sysvar::<Clock>().slot;
      fx.svm.warp_to_slot(committed + pool_program::invariants::TIMEOUT_SLOTS);
  ```
- `programs/pool-program/tests/stake_round.rs`, `cancel_intent_works_on_stake_pool`: immediately after that test sends its `commit_intent_tx(...)` (read the test for the exact call — it commits one intent on a stake pool), add the same two lines.

**Do NOT change `cancel_intent_rejects_wrong_signer`** — its wrong signer fails the `Signer` / `has_one` account-validation constraint *before* the handler body runs the timeout gate, so it still fails as expected with no warp. (Verify by reading it: it must not reach a successful account-validation path.)

- [ ] **Step 8: Full suite green + lint + commit**

Run: `cargo build-sbf --manifest-path programs/pool-program/Cargo.toml` then `cargo test -p pool-program` — all green (the 2 new gate tests pass; the warped existing cancel tests pass; `execute_round` and the withdraw suite unchanged). `cargo fmt` + `cargo clippy --all-targets -- -D warnings` clean.
```bash
git add programs/pool-program/src programs/pool-program/tests/cancel_intent.rs programs/pool-program/tests/stake_round.rs
git commit -m "feat(pool-program): timeout-gate cancel_intent (per-intent committed_slot + N-slot lockout)"
```

---

## Self-Review

**1. Spec coverage** (`docs/superpowers/specs/2026-07-17-timeout-gated-cancel-design.md`):
- Pure `cancel_unlock_slot` + `TIMEOUT_SLOTS` in `invariants.rs`, host-tested incl. overflow → Task 1.
- `Intent.committed_slot` (SPACE 121→129) set in `commit_intent` → Task 2 Steps 3-4.
- `cancel_intent` gate `current_slot >= committed_slot + N else CancelTooEarly` → Task 2 Step 5.
- `CancelTooEarly` appended after `StakeAccountInvalid` → Task 1 Step 1.
- Slots via Clock; per-intent reference → Task 2 (reads `intent.committed_slot`, `Clock::get()?.slot`).
- Tests warp both directions + host overflow test → Task 1 Step 2, Task 2 Step 1.
- Existing Plan 4/5 cancel tests updated to warp (seam regression) → Task 2 Step 7.
- Honest-claims-only: the only prose added is the gate comment in Step 5, which states exactly the spec's claim strength (removes instant exit, enforces the commitment, sub-k-by-construction) — no overclaim.
- Considered-out-of-scope items (round-expiry, per-pool config) → no task, by design.

**2. Placeholder scan:** No TBD/TODO. The test steps say "match the exact `cancel_ix`/fixture shapes already in `cancel_intent.rs`" — this is a directed instruction to reuse a concrete existing helper (read the file), not a placeholder; the asserted behavior and warp mechanics are fully specified.

**3. Type consistency:** `cancel_unlock_slot(u64) -> Result<u64>`, `TIMEOUT_SLOTS: u64`, `Intent.committed_slot: u64`, `PoolError::CancelTooEarly` are used identically in `invariants.rs`, `round.rs`, `lib.rs`, and both test files. `warp_to_slot` / `get_sysvar::<Clock>()` are the verified LiteSVM 0.6.1 API.
