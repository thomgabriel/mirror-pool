# F1 — Round-engine hardening: MAX_K cap + canonical batch ordering

**Date:** 2026-07-18 · **Status:** approved design, pending fork spec-review
**Grounding:** `docs/research/solana-execution-limits.md` (§1–2, §4),
`docs/research/anonymity-frontier-and-antisybil.md` §6.5,
`docs/superpowers/plans/2026-07-18-round-hardening-brief.md`
**Scope:** one plan, two tasks, one branch (`feat/round-hardening`). No `Pool`/`Round`/`Intent`
layout change anywhere in F1.

---

## 1. Problems (both source-confirmed in `programs/pool-program/src/lib.rs`)

**A — unbounded rounds strand funds (liveness bug).** `commit_intent` increments
`round.intent_count` with only an overflow guard (`lib.rs:184–188`); `execute_round` settles the
whole round in one vault-signed transaction (`rem.len() == count*3` withdraw, `count*3 + 6` stake).
Solana's 64-account-lock ceiling caps an executable round at roughly 17–19 intents (v0+ALT), so a
round that grows past the envelope is **permanently unexecutable**: `execute_round` can never build
a valid transaction, and committed funds exit only through `cancel_intent` — the single-note,
linkable, sub-`k` path. An adversary can induce this by committing `N × denomination` (recoverable
after the cancel timeout): a cheap griefing DoS that also degrades everyone onto the linkable exit.

**B — cranker-chosen batch order re-links initiator → action (anonymity gap).** Both
`execute_round` arms iterate `remaining_accounts` in exactly the order the cranker supplies, with
no on-chain constraint beyond a `seen`-Vec duplicate check. If that order tracks commit order — or
the cranker simply chooses it — batch **position** deanonymizes the participant *after* the ZK
proof succeeds. An off-chain "the cranker shuffles" promise is unenforceable; a
SlotHashes/blockhash-seeded on-chain permutation is rejected — that entropy source is
leader-grindable by exactly the party that controls ordering and timing.

## 2. Fixes

### Task A — action-kind-aware `MAX_K` cap (fail-closed)

- Two compile-time constants in `invariants.rs` — `MAX_K_WITHDRAW`, `MAX_K_STAKE` — behind a pure
  `pub fn max_k(action_kind: ActionKind) -> u16`, host-unit-tested like `cancel_unlock_slot`.
  Compile-time consts, not a pool parameter: the cap derives from Solana runtime limits, not pool
  policy; a configurable cap would let a pool be initialized back into the exact stranded-funds bug
  this closes (decision: user-approved 2026-07-18). If Solana's inactive `increase_tx_account_lock_limit`
  feature (64→128) ever activates on mainnet, raising the consts is a program upgrade — acceptable,
  disclosed.
- `initialize_pool`: `require!(k_floor <= max_k(kind), KFloorTooHigh)` — a pool whose floor exceeds
  the envelope could never execute any round.
- `commit_intent`: after the existing `checked_add`, `require!(round.intent_count <= max_k(kind),
  RoundFull)`. `MAX_K` is the maximum **executable** count (`<=` after increment). `cancel_intent`
  already decrements `intent_count`, so a full round can free a slot before execution.

**Pinning methodology — measured, not estimated.** The constant is set by a sweep, and the two
binding dimensions are pinned by the method that can actually observe each:

1. *Account locks (deterministic arithmetic):* count the fully-resolved key set — fixed context
   accounts + program ids + the ComputeBudget program id (the cranker must send
   `SetComputeUnitLimit` past ~k=6 stake) + 3 per intent + stake's 6-account shared tail — and
   assert ≤ 64 at the candidate `MAX_K`. This dimension is exact; no VM needed.
2. *Compute (in-VM measurement):* a LiteSVM sweep executes real rounds at increasing `k` for
   **both** action kinds, each transaction carrying a `SetComputeUnitLimit` instruction, and
   records CU consumed; the sweep establishes whether stake is lock-bound (≈17) or compute-bound
   near k≈16 — the open question the estimate cannot answer.

The shipped constant is **1 below the measured ceiling** (headroom for cranker-added
priority-fee/tip instructions and accounts). The spec of record (this file, updated at
implementation time) and README Limitations both carry the measured raw ceiling *and* the shipped
constant. Conservative expectations from the arithmetic: withdraw ceiling ≈ 19, stake ≈ 16–17.

*Caveat the implementer must resolve:* verify whether LiteSVM enforces `MAX_TX_ACCOUNT_LOCKS` at
all. If it does, the sweep observes the lock wall directly; if not, dimension 1's arithmetic
assertion carries the lock dimension and the sweep carries compute only — either way the method is
recorded honestly in the sweep test's comments.

**Permanent guard tests (LiteSVM + host):**
- A round at **exactly `MAX_K`** executes in one v0+ALT transaction — both kinds. This is the
  drift guard: if a future change (new account, heavier CPI) pushes the real ceiling below the
  constant, this test fails.
- The `(MAX_K + 1)`-th `commit_intent` fails `RoundFull` — both kinds if the caps differ.
- `initialize_pool` with `k_floor > max_k(kind)` fails `KFloorTooHigh`; `k_floor == max_k(kind)`
  succeeds.
- Host unit tests on `max_k` (both kinds return the constants; consts ≥ `MIN_K_FLOOR`).

*Test-runtime note:* the exact-`MAX_K` tests need ~16–19 real Groth16 proofs each. The plan must
decide the mitigation concretely (proof caching across the test, parallel proving, or an
`#[ignore]`-gated tier that CI runs explicitly) — not hand-wave it; the inner loop must stay usable.

### Task B — canonical batch ordering (derived-by-us; label as such)

- Both `execute_round` arms require the intent keys `rem[i*3].key` to be **strictly increasing**
  (byte-lexicographic `Pubkey` order) across the batch; violation →
  `PoolError::IntentsNotSorted`. The sort key is the **intent PDA key**: seeded
  `["intent", pool, nullifier_hash]`, it is fixed at commit, hash-derived (funder-unlinkable —
  hiding), already present in the account meta, and costs zero deserialization. Raw
  `nullifier_hash` is not stored on `Intent` (it exists only as a PDA seed), so the PDA key is the
  only candidate requiring no layout change.
- **Strict increase subsumes the `seen`-Vec duplicate check** (equal keys violate strict `>`), so
  the `seen` Vec is removed from both arms — a simplification, not an addition. The
  `DuplicateIntent` error variant becomes unreferenced but **stays in the enum**: `PoolError`
  variants are append-only (error-code ABI stability; existing tests hardcode discriminants), and
  Anchor's generated code keeps an unreferenced variant warning-free. One-line comment marks it
  retained for code stability.
- **No randomness:** a shuffle seeded from SlotHashes/blockhash is explicitly rejected
  (leader-grindable, §6.5); a VRF/commit-reveal beacon is out of scope (YAGNI). Deterministic
  hash-order is the fix that removes cranker discretion with zero new trust assumptions.
- **SDK (the interface change — why B lands before the soak):**
  `build_execute_round_ix` and `build_execute_stake_round_ix` sort the per-intent triples by
  intent key internally before emitting account metas — callers may pass any order and get a valid
  transaction. The SDK e2e tests and, later, the soak binary build against this final shape.

**Tests (LiteSVM):**
- A correctly-sorted batch executes (both kinds — covered by updating the existing round tests to
  the SDK builders / sorted order).
- An out-of-order batch (two intents swapped) rejects `IntentsNotSorted`.
- A duplicated intent triple still fails closed — now via `IntentsNotSorted`.
- **Canonical order is independent of commit order:** commit ≥3 intents in an order that differs
  from their PDA sort, assert execution succeeds only in PDA-sorted order and fails in
  commit order.

## 3. Error variants (append-only, after `FeeNotUniform`, in this order)

1. `RoundFull` — "round already holds the maximum number of executable intents"
2. `IntentsNotSorted` — "intent accounts must be in strictly increasing key order"
3. `KFloorTooHigh` — "k_floor exceeds the maximum executable round size"

## 4. Honesty ledger

- **A changes no anonymity claim.** It closes a liveness bug and makes the already-disclosed
  per-round ceiling *enforced and measured*: README Limitations trades the ~17/19 estimate for the
  pinned numbers. The realized anonymity-set ceiling (low tens, set by the Solana transaction
  envelope, not by our design) remains disclosed beside the whale-self-fill residual.
- **B upgrades the headline** from "uniform signer, disclosed ordering residual" to "uniform
  signer **+ canonical order**." The construction is **derived by us** (like `k∞`/`Adv` framing) —
  not a literature-named mixnet permutation; Furukawa–Sako / Neff verifiable shuffles remain the
  contrast, not the claim.
- **What B does *not* fix:** `committed_slot` is public, so commit-*timing* correlation
  (cross-round timing, §6.7) is untouched — that residual stays disclosed, not absorbed into the
  ordering claim. The cranker also still chooses *when* to execute; only *within-batch position*
  is removed as a signal.
- **The v0+ALT requirement stays off-chain** (SDK docs + soak): a program cannot observe its own
  transaction's version or account-resolution mechanism, so there is nothing to encode on-chain.

## 5. Non-goals

- No chunked execution (YAGNI verdict, `solana-execution-limits.md` §3).
- No per-pool `max_k` parameter (rejected above).
- No shared-relayer account optimization (§6 of the limits doc — a MAX_K-vs-correlation tradeoff
  deferred with the research).
- No `Round`/`Intent`/`Pool` layout changes.

## 6. Process

Branch `feat/round-hardening` → fork reviews this spec → plan (`writing-plans`) → fork reviews the
plan → subagent-driven TDD build (Task A, Task B) → whole-branch review (strongest model) → fork
reviews the merged branch → local merge. `cargo fmt` + `cargo clippy --all-targets -- -D warnings`
+ full `cargo test` green before "done." No push without the user's explicit yes.
