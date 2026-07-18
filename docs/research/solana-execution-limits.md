---
title: "Solana execution limits & MAX_K — the single-transaction anonymity envelope for execute_round"
date: 2026-07-17
status: research (informational — grounds a missing MAX_K cap in commit_intent and the single-tx-vs-chunking decision for execute_round; companion to the anonymity-frontier batch-ordering finding)
companion_to:
  - docs/superpowers/specs/2026-07-15-mirror-pool-design.md
  - docs/superpowers/specs/2026-07-16-pooled-stake-design.md
  - docs/research/anonymity-frontier-and-antisybil.md
method: >-
  Solana runtime limits (account-lock ceiling, transaction MTU, compute budget) read from primary
  sources — docs.anza.xyz (versioned transactions / lookup tables), the solana-sdk
  MAX_TX_ACCOUNT_LOCKS constant, and a live mainnet-beta feature-account query for
  increase_tx_account_lock_limit (2026-07-17) — and mapped against a first-hand read of the merged
  execute_round / commit_intent / action.rs. Runtime numbers marked here must still be PINNED by a
  LiteSVM sweep before any constant is hard-coded (CLAUDE.md mandates the in-VM test).
scope: >-
  How many identical actions one vault-signed execute_round transaction can settle (MAX_K), why that
  ceiling is the pool's real per-round anonymity-set ceiling, the missing on-chain cap that lets a
  round grow permanently unexecutable, the YAGNI call on chunked execution, and the cross-chunk
  ordering / Sealevel-contention residuals.
---

# Solana execution limits & MAX_K for execute_round

> **Purpose.** `execute_round` settles an entire round of `k` identical actions in **one
> vault-signed transaction** (`programs/pool-program/src/lib.rs:296` onward), iterating
> cranker-supplied `remaining_accounts`. Solana caps how many accounts one transaction can lock,
> so **the single-transaction envelope is the pool's real per-round anonymity-set ceiling** — a
> fact the design half-acknowledges (`action.rs:80` comments the stake path "k≈17") but does not
> yet *enforce*. This document pins that ceiling to primary Solana limits, shows the latent
> liveness+anonymity bug that follows from not enforcing it, and makes the build-vs-defer call on
> chunked execution.

---

## 0. The through-line: the executable unit must equal the anonymity unit

A round's anonymity set is the `k` intents that clear the floor and execute *together, indistinguishably*.
On Solana that set can only be as large as **one transaction can atomically settle** — there is no L1
cross-transaction atomicity, so anything that spans transactions is no longer one indivisible batch.
The single sentence that orders this doc: **size the round to the transaction, or the round stops being
a round.** Everything below is that constraint made precise, plus the one-line cap the code is missing.

---

## 1. The single-transaction account envelope → MAX_K

`execute_round` passes, per participant, three accounts — withdraw `[intent, recipient, relayer]`,
stake `[intent, stake_account, relayer]` — plus fixed context accounts, and for stake a 6-account
shared tail (`lib.rs:296–357`). The binding limit is **not** compute at any bounty-relevant `k`; it is
Solana's **per-transaction account-lock ceiling**.

### 1.1 The numbers, and which constraint binds

| Pool / tx form | Fixed keys | Per-intent | Ceiling that binds | **MAX_K** |
|---|---|---|---|---|
| **Withdraw, v0 + ALT** | 7 (6 context + program id) | 3 | 64 account-locks | **≈ 19** (`7 + 3·19 = 64`) |
| **Stake, v0 + ALT** | 13 (6 context + program id + 6-account stake tail) | 3 | 64 account-locks | **≈ 17** (`13 + 3·17 = 64`) — confirms `action.rs:80` |
| Withdraw, legacy (no ALT) | 7 | 3 | 1232-byte MTU (~35 keys) | ≈ 9 |
| Stake, legacy (no ALT) | 13 | 3 | 1232-byte MTU (~35 keys) | ≈ 7 |

Three grounded facts behind the table:

1. **The mainnet account-lock limit is 64, today.** `MAX_TX_ACCOUNT_LOCKS` is 64 by default; the
   `increase_tx_account_lock_limit` gate (64→128, the Neon-EVM-era change) was verified **inactive on
   mainnet-beta as of 2026-07-17** by a live feature-account query — only devnet/testnet activated it.
   The lock check counts the **fully-resolved** key set (static keys **plus** every ALT-loaded
   address), with no exemption. **If that gate later activates on mainnet, all ceilings roughly double
   (stake ≈ 37, withdraw ≈ 40)** — so the constant must never be hard-coded without re-checking the
   feature status.
2. **An Address Lookup Table is *required* to reach 17/19, but cannot exceed 64 locks.** ALT replaces
   each 32-byte inline key with a 1-byte index, defeating the 1232-byte MTU that otherwise caps a
   legacy tx at ~35 keys (stake ≈ 7, withdraw ≈ 9). ALT raises *addressability* to 256 — but the 64
   *lock* limit still binds and still counts ALT-resolved accounts. So the cranker **must** build a
   v0 + ALT transaction to reach a meaningful `k`, and even then 64 locks is the wall.
3. **Compute does not bind first (with one caveat).** From the repo's own LiteSVM datapoints
   (withdraw `k=2` ≈ 24.8k CU, ~12.4k/intent; stake `k=2` ≈ 58.3k CU, ~29k/intent), extrapolated
   `k=19` withdraw ≈ 0.24M CU and `k=17` stake ≈ 0.5M CU both sit under the 1,400,000 CU/tx max.
   **Caveat:** that is a single non-regression datapoint, and stake runs ~5 CPIs/intent, so stake
   being lock-bound (17) rather than compute-bound near `k≈16` **must be measured**, not assumed.
   Two second-order account costs also erode the ceiling: a `ComputeBudget SetComputeUnitLimit`
   instruction (which the cranker *must* send, since the default 200k CU/instruction is exceeded past
   ~`k=6` stake) adds the ComputeBudget program id as a key, dropping stake 17→**16**.

### 1.2 The honest headline

**Our realized per-round anonymity set is bounded to the low tens (~17–19), not by our privacy
design but by the Solana transaction envelope.** This is a material limitation and belongs in the
threat model and README beside the whale-self-fill residual — the two are different axes (composition
vs. size), and both cap the *effective* `k` below any nominal count.

---

## 2. The missing cap: an unbounded round is a latent liveness + anonymity bug

`commit_intent` increments `round.intent_count` with a `checked_add(1)` **overflow** guard only
(`lib.rs:185–188`); `meets_k_floor` is a **lower** bound; `execute_round` settles the **whole** round
in one call and flips `Open → Executed` in that call (`lib.rs:433`). There is **no upper bound
anywhere** (verified: `grep -riE 'MAX_K|max_intent|max_round'` over `programs/`+`crates/` returns
nothing).

Consequence: a round that accumulates more intents than one transaction can settle (past ~17–19,
or ~7–9 without ALT) becomes **permanently unexecutable** — `execute_round` can never construct a
valid transaction (`TooManyAccountLocks` / MTU / CU). Committed funds then exit *only* through
`cancel_intent`, whose own doc comment (`lib.rs:192–198`) calls it a **single-note, non-`k`-anonymous,
linkable** exit. So an unbounded round is a **liveness hole and an anonymity-degradation vector at
once**, and an adversary can *induce* it by committing enough intents (cost: `N × denomination`
locked, recoverable — a cheap griefing DoS that also forces everyone onto the linkable exit).

**Fix (fail-closed, per CLAUDE.md) — tracked as a spawned implementation task:**

1. `initialize_pool`: `require!(k_floor <= MAX_K)`.
2. `commit_intent`: `require!(intent_count < MAX_K)` after the `checked_add`.
3. Make `MAX_K` **action-kind-aware** (stake cap < withdraw cap: the 6-account tail + ~5 CPIs/intent).
4. **Pin `MAX_K` by a LiteSVM sweep**, not by this estimate — conservative starting points that sit
   safely under the 64-lock walls: withdraw ≈ 16, stake ≈ 13.
5. Require the cranker to build a **v0 + ALT** transaction (legacy MTU caps `k` at ~7–9, likely below
   a useful `k_floor`).

Sizing `MAX_K` to one transaction **aligns the executable unit with the anonymity unit** (§0) — the
single cheapest change that closes the bug *and* makes the anonymity ceiling explicit.

---

## 3. Chunked execution — the YAGNI call

**Do not build chunking for the bounty.** It is only justified to *advertise* `k > ~17`, and it buys
that at the cost of new surface (below). For a bounty-scope pool the correct move is single-tx
settlement + the `MAX_K` cap of §2.

If large-`k` is ever advertised, the correct construction is a **per-intent settled marker, not a
cursor.** A start-index cursor (SPL-stake-pool style) is the wrong fit: intents are independent PDAs
seeded by `nullifier_hash` with **no canonical on-chain order** (the cranker supplies order; there is
no shuffle), so "resume from index N" is meaningless. Instead:

- `RoundState { Open, Executing, Executed }`; reuse the **existing intent PDA** as the marker — a
  guarded `intent.executed` flag or close-the-intent-PDA-on-settle (absence == done). This is exactly
  the idempotency idiom the pool already uses for the nullifier PDA (init-fails-if-exists = atomic
  single-commit; `lib.rs:168`), and the merkle-distributor `ClaimStatus` pattern
  (saber-hq/merkle-distributor) — order-free, permissionlessly resumable. (Do **not** model it on
  SPL account-compression, which is an order-*managing* concurrent Merkle tree — a counterexample.)
- The in-call `seen` Vec blocks intra-chunk double-pay; the settled marker blocks cross-chunk double-pay.
- **Finalize on the last chunk only:** defer `round.state = Executed` **and** the `next_round` init
  (seeds `["round", pool, round_id+1]` — currently the single-call replay guard) until
  `settled_count == intent_count`. Flipping them on the first chunk would block every later chunk.
  This relocates the existing execute-once guarantee to finalize, unchanged.
- A dead cranker never strands funds (unsettled deposits stay in the vault); the genuinely new risk is
  the `Executing`-state stall window vs. `cancel_intent` (gated on `state == Open`) — mitigated by
  permissionless resume, but strictly more surface than single-tx + `MAX_K`.

---

## 4. Batch ordering — corrected (see `anonymity-frontier-and-antisybil.md` §6.5)

The **single-tx** design has **no anonymity ordering leak**. An earlier draft here (and the
anonymity-frontier doc) framed cranker-chosen execution order as a re-linking channel to be closed by an
on-chain sort; a 2026-07-18 spec review **retracted that**: the recipient rides in the same
`[intent, recipient, relayer]` triple as its intent, and `(recipient, committed_slot)` is already public
in the never-closed Intent PDA, so batch position leaks nothing not already public and never bridges to
the ZK-hidden funding linkage (full argument in `anonymity-frontier-and-antisybil.md` §6.5). A canonical
sort is at most an `O(n)` duplicate-check + determinism **cleanup**, not a privacy mechanism.

**Chunking stays a non-goal (§3)** on its own merits — the per-writable-account-per-block CU cap (§5) and
the added resumable-state-machine surface — *not* on an ordering-privacy argument. A SlotHashes-seeded
shuffle would be leader-grindable regardless, but there is nothing here it needs to hide.

---

## 5. Sealevel contention — already well-handled

Checked the account constraints against Sealevel's write-lock model:

- **The design already implements the key mitigation.** `Pool` (the zero-copy `AccountLoader`) is
  **read-only** (no `mut`) in `CommitIntent` and `CancelIntent` — only `Deposit` and `ExecuteRound`
  take it `mut` — and the counter (`intent_count`) lives on the **per-round `Round` PDA**, not the hot
  `Pool` account. So concurrent commit/cancel do not contend on `Pool`'s write lock.
- **The residual is bounded, not a DoS.** The hot writable accounts during an active round are the
  `Round` PDA (every commit/cancel/execute for that `round_id`) and the vault. Sealevel serializes
  txs declaring the same account writable, so concurrent `commit_intent`s and `execute_round` can't
  run in parallel — a **throughput throttle** (bounded by the per-writable-account-per-block CU cap;
  excluded txs retry next block), not an unbounded DoS. The one real liveness vector is **priority-fee
  griefing** on the shared `Round` PDA — inherent to any Sealevel design funneling a counter onto one
  shared account, not a mirror-pool-specific bug.
- **No reentrancy** inside `execute_round`'s sequential CPIs: Solana holds write-locks for the whole
  tx (not per-CPI), txs are atomic, and every CPI target (System/Stake builtins) is trusted with no
  callback into the program.
- **Chunking note:** the per-writable-account-per-block CU cap **sums across all txs** touching the
  vault in a block, so "many chunks in one block" (§4's mitigation) does not escape it — a very-large
  round may be *forced* to span blocks, which reintroduces the §4 timing leak. Another reason to
  prefer single-tx + `MAX_K`.

---

## 6. The relayer-account lever (raises MAX_K, at an anonymity cost)

Per-intent account count (3) is what sets `MAX_K`. If a **single shared relayer** were reused across
all intents in a round, the per-intent count drops 3→2, materially **raising** `MAX_K` — but a shared
relayer is a **correlation vector** (every payout's fee leg points at one key), an anonymity cost. The
stake path already keeps its count at 3 by passing `recipient` as **Pubkey CPI-data**, not an account
(`action.rs:79–80`). Whether `relayer` is per-intent or shareable, and whether the withdraw
`recipient` could likewise become CPI-data, is a real `MAX_K`-vs-correlation tradeoff to evaluate
(measure both) — **not** a free win.

---

## 7. Honest limitations

1. **`MAX_K` is estimated, not measured here.** The ~17/19 (v0+ALT) and ~7/9 (legacy) figures follow
   from the 64-lock arithmetic and a **single** LiteSVM CU datapoint (`k=2`). Before any constant is
   hard-coded, a LiteSVM sweep over `k` for *both* action kinds must establish whether stake is
   lock-bound (17) or compute-bound near `k≈16`, and confirm the compiled account set (program ids
   included).
2. **The 64-lock ceiling is a point-in-time mainnet fact (2026-07-17).** If
   `increase_tx_account_lock_limit` activates on mainnet, ceilings roughly double — re-confirm before
   relying on 64.
3. **No production precedent found** for a *single-instruction* ALT-batched CPI-payout at this shape:
   the Solana lookup-table course batches 57 *plain SOL transfers* (compute-light, not CPI-heavy
   stake), and Jito's distributor batches via **many independent per-claim txs**, not one
   CPI-per-recipient instruction. So the stake compute headroom near the ceiling is genuinely
   unvalidated by prior art — measure it.
4. **This doc does not itself change code.** The `MAX_K` cap is a tracked implementation task (spec →
   plan → TDD → review); the numbers above are the inputs to that task, not a merged decision.

---

## 8. What to apply — and where

| Finding | Action | Where |
|---|---|---|
| Missing `MAX_K` cap (§2) | Add `require!(intent_count < MAX_K)` at commit, `require!(k_floor <= MAX_K)` at init; action-kind-aware; pin by LiteSVM | `commit_intent`, `initialize_pool` (implementation task) |
| Per-round `k` ceiling ~17–19 (§1) | Disclose as a size limitation beside whale-self-fill | README "Limitations"; spec threat table |
| Cranker must use v0 + ALT (§1) | Document the cranker's tx-construction requirement | pooled-stake / coordinator notes |
| Do **not** chunk (§3) | Record the YAGNI verdict + the settled-marker pattern *if* revisited | this doc; spec open-questions |
| Same-block requirement *if* chunked (§4) | Record so a future large-`k` build doesn't reintroduce the leak | this doc |
| Batch order leaks to cranker (§4) | Cross-reference the anonymity-frontier ordering fix | `anonymity-frontier-and-antisybil.md` |

---

## References (grounded via the 2026-07-17 systems pass, primary Solana sources)

1. Versioned transactions & Address Lookup Tables — `https://docs.anza.xyz/proposals/versioned-transactions` (256-address *addressability* via `u8` indexing; ALT resolution = static keys + ALT writable/readonly indexes).
2. `MAX_TX_ACCOUNT_LOCKS` — solana-sdk (`transaction`), "maximum number of accounts that a transaction may lock"; default **64**, **128** under `increase_tx_account_lock_limit`. Lock check: `validate_account_locks` over the fully-resolved key set.
3. `increase_tx_account_lock_limit` feature — **inactive on mainnet-beta as of 2026-07-17** (live feature-account query; devnet/testnet activated).
4. Per-transaction compute limit **1,400,000 CU**; default **200,000 CU/instruction** absent `ComputeBudget SetComputeUnitLimit`; builtin System/Stake CPIs default ~3,000 CU each — Solana runtime docs.
5. Merkle-distributor `ClaimStatus` per-claim self-marking PDA — `github.com/saber-hq/merkle-distributor` (`programs/merkle-distributor/src/lib.rs`) — the order-free, resumable marker pattern (contrast SPL account-compression, an order-managing structure).
6. Repo LiteSVM datapoints (per-tx CU at `k=2`): withdraw ≈ 24,794 CU, stake ≈ 58,313 CU — `programs/pool-program` tests (single datapoint; a sweep is required).
