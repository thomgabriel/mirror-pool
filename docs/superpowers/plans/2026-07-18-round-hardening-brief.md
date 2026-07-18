# F1 — Round-engine hardening: design brief (for the building lane)

> **What this is:** reviewer-lane input, grounded in `docs/research/solana-execution-limits.md` and
> `docs/research/anonymity-frontier-and-antisybil.md` §6.5. The building lane still runs its own
> `brainstorm → spec → plan → TDD`; use this as the requirements + the open questions, not as a finished
> spec. Fork reviews the spec, the plan, and the merged branch (same gate as Pool.fee / effective-k).
> Two items; lean toward **one spec/plan with two tasks** (both touch the round handlers).

---

## Item A — `MAX_K` cap (a latent liveness bug)

**Problem (source-confirmed).** `commit_intent` bounds nothing — it only `checked_add(1)`s
`round.intent_count` (`programs/pool-program/src/lib.rs:185`). `execute_round` settles the *whole* round
in one vault-signed tx (`rem.len() == count*3` for withdraw, `count*3 + 6` for stake, `lib.rs:300/353`).
Solana's 64-account-lock limit therefore caps an executable round at ~17 (stake) / ~19 (withdraw) with a
v0+ALT tx (~7/9 legacy). A round that accumulates past that is **permanently unexecutable** → funds exit
only via `cancel_intent`, the linkable, sub-k path. It is inducible by griefing (cost = `N × denomination`
locked). See `solana-execution-limits.md` §1–2.

**Fix (fail-closed).**
- Add `PoolError::RoundFull`.
- `initialize_pool`: `require!(k_floor <= MAX_K, …)`.
- `commit_intent`: after the `checked_add`, `require!(round.intent_count <= MAX_K, RoundFull)`.
- `MAX_K` is **action-kind-aware** (withdraw cap > stake cap: the 6-account tail + ~5 CPIs/intent).
- **Pin `MAX_K` by a LiteSVM sweep, not the estimate.** Conservative starting points under the 64-lock
  walls: withdraw ≈ 16, stake ≈ 13. Account for the `ComputeBudget SetComputeUnitLimit` ix's program id
  as a key (drops the stake ceiling by 1).

**Tests (LiteSVM + host).**
- Fill a round to `MAX_K`; assert the `(MAX_K+1)`-th `commit_intent` fails `RoundFull`.
- A round at exactly `MAX_K` **executes** in one tx (guards against setting `MAX_K` too high).
- Cover both action kinds if the caps differ.

**Open for the brainstorm.** Exact constants (measure). Compile-time const vs. pool parameter. Whether to
encode the "cranker must use v0+ALT" requirement anywhere on-chain or leave it to the SDK/soak.

---

## Item B — Canonical batch ordering — **DROPPED (fork spec-review, 2026-07-18)**

> **Retraction.** The anonymity claim below is vacuous and Item B was dropped at the spec-review
> gate: the intent↔recipient pairing is bound inside each `[intent, recipient, relayer]` triple
> (order-independent), `commit_intent`'s own transaction publicly names the recipient, and
> `(recipient, committed_slot)` sit in the never-closed Intent PDA — so batch position cannot
> re-link what was never unlinked; the funding secret is ZK-protected and orthogonal to order.
> Full record: spec `2026-07-18-round-hardening-design.md` §5; research corrected in `14e6d49`
> (`anonymity-frontier-and-antisybil.md` §6.5, `solana-execution-limits.md` §4). The text below
> is retained unedited as the historical input the review overturned.

**Problem (source-confirmed — retracted, see banner).** `execute_round` iterates `remaining_accounts` in the **order the cranker
supplies** (`for i in 0..count` over `rem[i*3..]`), with no on-chain shuffle; `commit_intent` records only
`committed_slot`. So batch **position** can re-link initiator → action *after* the crypto succeeds — the
disclosed ordering side-channel. See `anonymity-frontier-and-antisybil.md` §6.5, `solana-execution-limits.md` §4.

**Fix (derived-by-us — label as such, not literature-named).**
- Require `remaining_accounts` **sorted by a value fixed and hiding at commit** — the intent PDA key
  (seeded `["intent", pool, nullifier_hash]`, already in hand as `rem[i*3].key`), which is a hash-derived,
  funder-unlinkable value. On-chain: assert each triple's intent-key is **strictly greater** than the
  previous; reject otherwise (`PoolError::IntentsNotSorted`).
- Strictly-increasing **subsumes the existing `seen`-Vec duplicate check** — a nice simplification, not an
  addition. Confirm and remove the now-redundant `seen` scan.
- No randomness beacon / VRF (within YAGNI). **REJECT** a SlotHashes/blockhash-seeded shuffle — that source
  is leader-grindable (§4).

**Interface change (this is why B lands *before* the soak).** The cranker must now sort the
`[intent, recipient, relayer]` triples by intent-key before building the tx. The SDK's
`build_execute_round_ix` / `build_execute_stake_round_ix` (and the soak binary) must emit the sorted order.
Build the soak once, against this final shape.

**Tests.**
- A correctly-sorted batch executes; an out-of-order batch rejects `IntentsNotSorted`.
- The canonical order is **independent of commit order** (commit three intents in one order, assert the
  required execution order is the nullifier-derived sort, not the commit sequence).
- The duplicate-intent case still fails closed (now via the strict-increase check).

**Open for the brainstorm.** Sort key: intent PDA key vs. raw `nullifier_hash` (both derive from
`nullifier_hash`; the PDA key is already in the account meta — cheapest). Error naming. Confirm the
`seen`-Vec removal is safe under the new invariant.

---

## Honesty + sequencing notes

- **A** is disclosed as the realized per-round k ceiling (already in README Limitations); it does not
  change any anonymity claim, it closes a bug.
- **B** upgrades the headline from "uniform *signer*, disclosed order residual" to "uniform signer **+**
  canonical order" — keep the `k_∞`/`Adv`-style "derived-by-us, not a literature term" labeling.
- Order of work: **B before the soak** (interface coupling + it strengthens the soak's assertion set);
  **A** is independent and can land in parallel or after — it doesn't block a small-`k` soak.
