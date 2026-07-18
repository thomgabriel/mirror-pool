# F1 — Round-engine hardening: the MAX_K cap

**Date:** 2026-07-18 · **Status:** fork spec-review passed (Task A approved; Task B dropped — §5)
**Grounding:** `docs/research/solana-execution-limits.md` (§1–2),
`docs/superpowers/plans/2026-07-18-round-hardening-brief.md`
**Scope:** one plan, one branch (`feat/round-hardening`). No `Pool`/`Round`/`Intent` layout change.

---

## 1. Problem (source-confirmed in `programs/pool-program/src/lib.rs`)

**Unbounded rounds strand funds (liveness bug).** `commit_intent` increments
`round.intent_count` with only an overflow guard (`lib.rs:184–188`); `execute_round` settles the
whole round in one vault-signed transaction (`rem.len() == count*3` withdraw, `count*3 + 6` stake).
Solana's 64-account-lock ceiling caps an executable round at roughly 17–19 intents (v0+ALT), so a
round that grows past the envelope is **permanently unexecutable**: `execute_round` can never build
a valid transaction, and committed funds exit only through `cancel_intent` — the single-note,
linkable, sub-`k` path. An adversary can induce this by committing `N × denomination` (recoverable
after the cancel timeout): a cheap griefing DoS that also degrades everyone onto the linkable exit.

## 2. Fix — action-kind-aware `MAX_K` cap (fail-closed)

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
  RoundFull)`. `MAX_K` is the maximum **executable** count (`<=` after increment). This
  deliberately diverges from `solana-execution-limits.md` §2's `< MAX_K` phrasing — `<` after the
  increment would silently cap real rounds one below the constant; the `<=` form is what makes the
  "exactly-`MAX_K` executes" guard test coherent. `cancel_intent` already decrements
  `intent_count`, so a full round can free a slot before execution. The new `require!`s are
  fail-closed atomic: a rejected commit reverts the increment and the `intent`/`nullifier` PDA
  inits in the same transaction.

### Pinning methodology — measured, not estimated

The constant is set by a sweep, and the two binding dimensions are pinned by the method that can
actually observe each:

1. *Account locks — a programmatic compiled-transaction test, not arithmetic in a comment
   (fork finding A1).* LiteSVM almost certainly does **not** enforce `MAX_TX_ACCOUNT_LOCKS` (the
   64-lock check runs at transaction sanitization/banking stage, not inside the VM), so in-VM
   execution cannot observe the lock wall. The lock drift guard is therefore a test that builds
   the **real v0+ALT `execute_round` transaction at `MAX_K` via the SDK builder**, resolves its
   message, and asserts the fully-resolved key count (static keys + ALT-loaded addresses) ≤ 64 —
   for both action kinds, with the `ComputeBudget SetComputeUnitLimit` instruction included. A
   static arithmetic comment goes stale the moment someone adds a per-intent account; a
   compiled-tx assertion catches it. (The implementer still verifies the LiteSVM-lock-enforcement
   assumption and records the answer in the test's comments.)
2. *Compute — in-VM measurement:* a LiteSVM sweep executes real rounds at increasing `k` for
   **both** action kinds, each transaction carrying a `SetComputeUnitLimit` instruction, and
   records CU consumed; the sweep establishes whether stake is lock-bound (≈17) or compute-bound
   near k≈16 — the open question the estimate cannot answer.

The shipped constant is **1 below the measured ceiling** (headroom for cranker-added
priority-fee/tip instructions and accounts). The spec of record (this file) and README Limitations
both carry the measured raw ceiling *and* the shipped constant.

**Measured 2026-07-18** (`programs/pool-program/tests/max_k.rs::sweep_execute_round_ceiling`;
full tables in the Task 1 report):

- **Withdraw is lock-bound, not compute-bound.** The sweep found no failure of any kind (compute
  or otherwise) up to k=21 — 133,929 CU at k=21, well under the 1.4M budget. The ceiling is set
  entirely by the 64-account-lock arithmetic, `⌊(64-9)/3⌋ = 18` (conservative, counting the ALT
  table account itself). Shipped one below: **`MAX_K_WITHDRAW = 17`**.
- **Stake is heap-bound, not lock- or compute-bound** — a material correction to this section's
  earlier ≈16–17 estimate. The sweep hits `ProgramFailedToComplete` / "memory allocation failed,
  out of memory" at k=12 (only ~270k CU extrapolated, far under budget, and far short of the
  lock-arithmetic bound `⌊(64-15)/3⌋ = 16`). Root cause: `solana_program`'s default global
  allocator is a 32 KiB bump allocator that never frees, and each stake intent's 5 CPIs
  (`create_account`/`initialize`/`delegate_stake`/`authorize`/fee-transfer) accumulate allocations
  across the round. Re-measured with `ComputeBudgetInstruction::request_heap_frame(256 KiB)`:
  identical failure at the identical k — the wall is **not liftable by the cranker**; only a
  custom on-chain allocator could raise it (out of this plan's scope; future work). Shipped one
  below the measured ceiling of 11: **`MAX_K_STAKE = 10`**.
- **LiteSVM-lock-enforcement answer (fork finding A1's open question):** legacy `Message`s cannot
  reach either ceiling at all — they hard-panic (not a graceful `TransactionError`) once total
  account keys exceed 38, from `solana-compute-budget-instruction`'s internal
  `ComputeBudgetProgramIdFilter` array bound (`FILTER_SIZE = PACKET_DATA_SIZE / 32 = 38`), which
  binds tighter than either the 64-account-lock wall or this section's original ~35-account
  legacy-MTU estimate. The sweep and both guard-test layers therefore compile real v0+ALT
  `VersionedTransaction`s throughout — confirming LiteSVM enforces neither the 64-lock nor any
  compute ceiling for an in-range resolved v0+ALT key set, which is exactly why the compiled-tx
  key-count test (`crates/sdk/tests/tx_envelope.rs`) is the authoritative lock guard rather than
  an in-VM assertion.

The two pre-measurement number sets this section previously carried (raw lock arithmetic:
withdraw ≈ 19 / stake ≈ 16–17; the brief's buffered starting points: withdraw ≈ 16 / stake ≈ 13)
are both superseded by the measurement above.

### Permanent guard tests (LiteSVM + host)

- A round at **exactly `MAX_K`** executes in one transaction — both kinds. This is the compute
  drift guard: if a future change (heavier CPI, new per-intent work) pushes the real ceiling below
  the constant, this test fails. Paired with the compiled-tx key-count test above, both binding
  dimensions are guarded programmatically.
- The `(MAX_K + 1)`-th `commit_intent` fails `RoundFull` — both kinds if the caps differ.
- `initialize_pool` with `k_floor > max_k(kind)` fails `KFloorTooHigh`; `k_floor == max_k(kind)`
  succeeds.
- Host unit tests on `max_k` (both kinds return the constants; consts ≥ `MIN_K_FLOOR`).

*Test-runtime commitment (fork finding A2):* the exact-`MAX_K` and `RoundFull` tests need ~16–20
real Groth16 proofs each. The plan **commits to a reusable proof-fixture set** — deposit all notes
first, generate every proof against one (ring-retained) root, cache the set once per test binary
(`OnceLock` or equivalent) — never per-test regeneration. If wall-clock still threatens the inner
loop, the sweep (not the guard tests) may be `#[ignore]`-gated for explicit/CI runs.

## 3. Error variants (append-only, after `FeeNotUniform`, in this order)

1. `RoundFull` — "round already holds the maximum number of executable intents"
2. `KFloorTooHigh` — "k_floor exceeds the maximum executable round size"

## 4. Honesty ledger

- **This changes no anonymity claim.** It closes a liveness bug and makes the already-disclosed
  per-round ceiling *enforced and measured*: README Limitations trades the ~17/19 estimate for the
  pinned numbers. The realized anonymity-set ceiling (low tens, set by the Solana transaction
  envelope, not by our design) remains disclosed beside the whale-self-fill residual.
- **The v0+ALT requirement stays off-chain** (SDK docs + soak): a program cannot observe its own
  transaction's version or account-resolution mechanism, so there is nothing to encode on-chain.

## 5. Examined and dropped: canonical batch ordering (fork spec-review, 2026-07-18)

The brief's Item B — require `remaining_accounts` sorted by intent PDA key so the cranker cannot
choose batch order — was designed, adversarially reviewed, and **dropped: its anonymity claim is
vacuous**. The record, so it is not re-litigated:

- In `execute_round`, each recipient rides in the same `[intent, recipient, relayer]` triple as
  its intent and is bound to it by `require_keys_eq!` — the intent↔recipient pairing is in the
  transaction *structure*, order-independent. Sorting permutes positions; it changes no pairing.
- The (commit-time → recipient → action) linkage is **already fully public and permanent**:
  `commit_intent`'s own transaction names the recipient and relayer as accounts, and
  `(recipient, relayer, committed_slot)` sit in the Intent PDA, which `execute_round` never
  closes. Batch position cannot re-link what was never unlinked.
- The one thing the protocol actually hides — **which deposit funds each intent** — is protected
  by the ZK proof and is orthogonal to execution order. No adversary vantage exists where
  within-batch position bridges to the funding secret.
- What remains of the idea is a determinism/perf cleanup (an O(n) strict-increase check replacing
  the O(n²) `seen`-Vec dedup) — at `k ≤ ~17` that saves ≤ ~289 pubkey compares and costs an SDK
  interface change plus test churn. Not worth it; dropped, not deferred.
- **Consequences:** the soak has **no F1 dependency** (ordering was the only interface coupling —
  the "B before the soak" sequencing rule disappears), and the research framing that called
  batch-ordering an anonymity gap (`anonymity-frontier-and-antisybil.md` §6.5,
  `solana-execution-limits.md` §4, the finish-roadmap, the F1 brief) is **overstated and is being
  corrected by the review lane**: the honest line is *"we examined execute-order; it is not an
  anonymity gap — the linkage is already public per Intent PDA — so canonical ordering is at most
  a determinism cleanup."*

## 6. Non-goals

- No canonical batch ordering (§5 — examined, vacuous, dropped).
- No chunked execution (YAGNI verdict, `solana-execution-limits.md` §3).
- No per-pool `max_k` parameter (rejected above).
- No shared-relayer account optimization (§6 of the limits doc — a MAX_K-vs-correlation tradeoff
  deferred with the research).
- No `Round`/`Intent`/`Pool` layout changes.

## 7. Process

Branch `feat/round-hardening` → fork spec review (**passed 2026-07-18**: Task A approved with A1/A2
folded above; Task B dropped) → plan (`writing-plans`) → fork reviews the plan → subagent-driven
TDD build → whole-branch review (strongest model) → fork reviews the merged branch → local merge.
`cargo fmt` + `cargo clippy --all-targets -- -D warnings` + full `cargo test` green before "done."
No push without the user's explicit yes.
