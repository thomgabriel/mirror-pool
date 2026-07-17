# Pool.fee uniformity — design spec

> **Status:** design (spec-only — no implementation yet; the review session checks this
> against the research + source before the plan is written). First of three build items
> from the mechanism-research pass (`docs/research/crowd-depth-and-timing-mechanisms.md`
> §5.2): (1) this `Pool.fee` uniformity change, then (2) a `round_executable_slot` timing
> slice and (3) the effective-k harness fixture — both separate later plans.
>
> **Primary design sources:** `crowd-depth-and-timing-mechanisms.md` §2.6 (Pool.fee as the
> cheapest nominal-cost anti-Sybil tax; bonded-slashing rejected), §4.4 (the shipped
> amount-uniformity leak + the priority-fee advisory), §5.1–5.2 (the convergence + build
> order). `anonymity-frontier-and-antisybil.md` §1 (min-entropy `k_∞` lens) and §2.5 (the
> passive on-chain observer as the primary adversary). `behavioral-privacy-industry-practices.md`
> already foresaw this: the settlement side is "where reuse/self-transfer/**denomination
> linkage** still bites."

## Problem — a shipped, adversary-free settlement fingerprint

Fixed `denomination` already forecloses *principal* fingerprinting (`deposit` requires
`amount == denomination`). **The payout split is not fixed for withdraw pools.**

- `commit_intent` checks only `fee <= pool.denomination` for a withdraw intent
  (`lib.rs:149`); the uniformity gate `fee == pool.stake_fee` (`lib.rs:152`) fires **only**
  when `pool.action_kind == 1` (stake). So a withdraw pool accepts a **free per-intent
  `fee`**.
- `WithdrawAction::execute` pays `split_payout(denomination, fee) → (payout, fee)` as **two
  separate, publicly-visible vault transfers** — `denomination − fee` to `recipient`, `fee`
  to `relayer` (`action.rs:36`, `invariants.rs:12`).

Therefore **any two withdraw intents in the same round with different `fee` are pairwise
distinguishable by output amount alone, with zero timing analysis**, and a user/relayer
pairing that consistently uses a fixed non-default fee becomes a **stable cross-round
fingerprint**. This is structurally the Zcash 249.9999→250.0001 ZEC value-linkage finding
(Kappos *et al.* 2018 / benthamsgaze: **28.5% of coins linked**). It is exploitable **today,
with no active adversary** — just two different fees in one round. It is also the cheapest
anti-Sybil gap: a whale self-filling a withdraw pool pays **`fee = 0`**, i.e. self-fill is
free (`crowd-depth-and-timing-mechanisms.md` §2.6).

Two independent research deep-dives — the anti-Sybil side (§2.6) and the amount-uniformity
side (§4.4) — converge on the **same one change** (§5.1).

## The change

Generalise the shipped, stake-only `stake_fee` into **one mandatory, pool-wide `fee`,
enforced uniformly for BOTH action kinds.** Plan 5 built the first caller (stake); this is
the "second concrete caller" YAGNI asks for before generalising a pattern — so the
generalisation is now warranted, not speculative. No new PDA, no circuit change, no new
pure fn; the enforcement is a one-line `require!`, matching the `lib.rs:151-153` precedent.

## Honest scoping — state this plainly (the review rejects overclaiming)

- **This closes the *payout-amount* fingerprint at settlement, and nothing more.** It does
  **NOT** close the *commit-time priority-fee / CU-price* fingerprint: a relayer's chosen
  compute-unit price on `commit_intent` is a Solana analogue of Tutela's gas-price heuristic,
  a client choice the protocol **cannot constrain on-chain**. That is `[UNVERIFIED — reasoned
  by analogy to a verified Ethereum-mixer finding]`, **advisory guidance, not a build item**
  in this spec (same category as destination-reuse guidance; §4.4).
- **In the min-entropy `k_∞` lens this is a NOMINAL-COST anti-Sybil tax, never a crowd-depth
  mechanism.** A whale is one funder who simply pays `m·fee`; `k_∞` (which scores
  probability-mass concentration by *funder*) is **unmoved** (`crowd-depth-and-timing-mechanisms.md`
  §0.3 bucket 2). It must never be presented as raising distinct-human k.
- **Primary adversary: the passive on-chain observer reading round outputs** (frontier §2.5),
  not coordinator compromise. Trusting the coordinator does nothing against this attacker; the
  amount fingerprint is readable by anyone, for free, from the chain.

## The `stake_fee → fee` change surface (verified touch points)

- **`state.rs:46`** — rename the field `stake_fee: u64` → `fee: u64`. **This is a PURE
  RENAME, not a layout change:** `u64 → u64` at the same tail offset, so `size_of::<Pool>()`
  is unchanged and the size assert (`state.rs:56`) holds trivially. **Correction to the
  research:** `crowd-depth-and-timing-mechanisms.md` §5.2's "a `Pool` layout change is not
  backward-compatible" migration note is **over-stated for this rename** — existing pool
  accounts stay **byte-compatible** (identical bytes, a renamed field). This is unlike the
  timeout-cancel `Intent::SPACE` 121→129 change, which *was* a real size change; here there
  is **no migration concern**.
- **`lib.rs:24-30`** — `initialize_pool`'s `stake_fee: u64` param → `fee: u64` (same position;
  wire-format unchanged — see SDK below).
- **`lib.rs:40`** — the withdraw-pool validation currently forces `stake_fee == 0`. Replace
  with a pool-wide fee bound (decision D3).
- **`lib.rs:48`** — `stake_split(denomination, stake_fee, stake_rent)` → `…, fee, …`.
- **`lib.rs:80`** — `pool.stake_fee = stake_fee` → `pool.fee = fee`.
- **`lib.rs:149,151-152`** — the core change: collapse the per-intent `fee <= denomination`
  check and the stake-only `fee == stake_fee` branch into ONE unconditional check (decision D2).
- **`lib.rs:263-281`** — `execute_round` already reads the pool fee (`stake_fee` → `fee`) into
  a local at the top, **in scope for both dispatch arms**; no change beyond the rename.
- **`lib.rs:270,280`** — the stake dispatch reads `pool.stake_fee` → `pool.fee`.
- **`lib.rs:384`** — the stake arm's execute-time defense-in-depth
  `require!(intent.fee == stake_fee, WrongActionConfig)` → `require!(intent.fee == fee,
  PoolError::FeeNotUniform)` (rename + the D1 error change).
- **`lib.rs:~349` (withdraw arm) — ADD the symmetric check.** The withdraw dispatch currently
  passes `fee: intent.fee` into `WithdrawAction` with **no execute-time re-check** — an
  asymmetry with the stake arm. Add, before constructing `WithdrawAction` (mirroring line 384,
  using the in-scope `fee` local): `require!(intent.fee == fee, PoolError::FeeNotUniform);`.
  This is the load-bearing fix that makes D1's "both action kinds" true (see D1), restores
  stake/withdraw symmetry, and honors Plan 5's banked lesson — *"uniformity is the product;
  execute IDENTICALLY; apply this lens to every future `PooledAction` adapter."* `WithdrawAction`
  is a `PooledAction` adapter that now has a uniform fee to protect.
- **`lib.rs:688`** — the `WrongActionConfig` `#[msg("…stake_fee configuration is invalid…")]`
  text must drop "stake_fee" (→ "fee").
- **`invariants.rs:34`** — `stake_split`'s `stake_fee` parameter name (internal; optional
  rename for consistency — no behavioural effect).
- **`crates/sdk/src/lib.rs:137,147,154`** — `build_initialize_pool_ix`'s `stake_fee` param →
  `fee`, and the encoded byte at the same offset. **Wire-format unchanged:** the instruction
  data layout `disc(8)‖denomination(8)‖k_floor(2)‖action_kind(1)‖validator(32)‖fee(8)` is
  byte-identical; only the param/label change.
- **`crates/sdk/src/lib.rs:~631`** — the `initialize_pool` byte-offset test asserts
  `data[51..59]` — same offset, just relabel `"stake_fee"` → `"fee"`.

## Decisions this spec makes (and justifies)

### D1 — Error variant: add an appended `FeeNotUniform`

Add a **new `PoolError` variant `FeeNotUniform`, appended after the current last variant
`CancelTooEarly`** (`lib.rs:695`), honouring the append-only convention the timeout-cancel
spec established (`deposit.rs` hardcodes error codes 6001/6002; inserting/reordering breaks
them). Use it for the commit-time uniformity check (D2) **and** the execute-time
defense-in-depth check on **both** action kinds — which requires **adding** the withdraw-side
re-check that does not exist today (the stake arm's check at `lib.rs:384` moves to this
variant; the withdraw arm gains a symmetric one — see the ripple's `lib.rs:~349` entry). The
"both kinds" property is not automatic in the current wiring; it is a deliberate part of this
change.

*Why not reuse `WrongActionConfig`:* its message and role are **init-time pool-config
validation** ("action_kind/validator/fee configuration is invalid for *this pool*"). A
per-intent "this intent's fee ≠ the pool's uniform fee" is a **distinct runtime condition**;
overloading `WrongActionConfig` would give a misleading error and blur two different failure
classes. A dedicated variant keeps `WrongActionConfig` scoped to init and gives an honest
message. The append is cheap and the convention is already set — so the semantic clarity wins.

### D2 — `commit_intent`: one unconditional uniformity check

Replace the current two checks — the per-intent `fee <= pool.denomination` (`lib.rs:149`)
**and** the stake-only `if action_kind == 1 { fee == pool.stake_fee }` (`lib.rs:151-152`) —
with a single **unconditional** guard:

```
require!(fee == pool.fee, PoolError::FeeNotUniform);   // both action kinds
```

*Why this is complete and not a regression:* `pool.fee` is validated `≤ denomination` at
`initialize_pool` (D3), so `fee == pool.fee` implies `fee ≤ denomination` — the per-intent
bound at `lib.rs:149` is **subsumed** and removed (CLAUDE.md: no dead code / no dead
asymmetry). `FeeExceedsDenomination` remains used at init (D3) **and** as execute-time
defense-in-depth inside `split_payout` (`invariants.rs:13`) / `stake_split`, so it is not
orphaned. A withdraw pool that wants no relayer fee sets `fee = 0`.

### D3 — Init-time fee bound (both kinds)

`initialize_pool` validates the pool-wide `fee` per kind:

- **Withdraw** (`action_kind == 0`): replace the `stake_fee == 0` force with
  `require!(fee <= denomination, PoolError::FeeExceedsDenomination)`. `validator` must still
  be `Pubkey::default()` (a withdraw pool has no validator).
- **Stake** (`action_kind == 1`): unchanged — `stake_split(denomination, fee, stake_rent)?`
  already enforces the tighter bound `delegated = denomination − fee − rent ≥
  MIN_STAKE_DELEGATION`.

*The `fee == denomination` (payout 0) edge — decision: allow it.* For a withdraw pool
`fee == denomination` yields `payout = 0`; `split_payout`'s `if payout > 0` already skips the
recipient transfer safely (`action.rs:37`), and the round stays **uniform** (every intent
pays 0 to its recipient and `fee` to its relayer). It is an economically pointless but
**safe** config; forbidding it (`fee < denomination`) would be extra surface for a
configuration no honest operator chooses (YAGNI). We therefore keep the loosest safe bound
`fee <= denomination`, matching the pre-existing per-intent semantics.

### D4 — Field is a pure rename (no migration)

Per the source-verified layout above, `stake_fee → fee` is byte-identical. **No migration
note is warranted** (correcting the research). The instruction wire-format is likewise
unchanged, so the SDK and on-chain handler stay compatible across the rename.

## Value conservation (confirm)

- **Withdraw:** every intent in a round now has `fee == pool.fee`, so `payout = denomination
  − pool.fee` is **identical across the round** — the fingerprint is closed by construction.
  Per intent the vault is debited `payout + fee = denomination`; conserved.
- **Stake:** `delegated = denomination − pool.fee − stake_rent` was already uniform for stake
  pools; unchanged. Per intent the vault is debited `denomination`; conserved.

No path over-drains the vault; the amounts are uniform across the round for both kinds.

## Disclosed tradeoff

This **removes per-intent relayer-fee flexibility on withdraw pools** — a pool now has one
fee for all its intents/relayers. **That is the intended effect:** a variable fee *is* the
fingerprint. A relayer that wants a different fee uses a different pool (a different
anonymity set), which is the honest way to price differently without leaking a linkage.

## Testing (TDD, per CLAUDE.md)

- **Rename** the existing stake fee-uniformity tests (`stake_fee` → `fee`), including the SDK
  `initialize_pool` offset test's label. These must stay green (behaviour unchanged for stake).
- **ADD withdraw uniformity tests** (the new coverage — the shipped leak this closes):
  - two withdraw intents committed to one round with **different** `fee` → the non-matching
    one is rejected with `FeeNotUniform` (assert the specific error, not a generic failure);
  - after `execute_round` on a withdraw pool, **all recipient payouts are identical** across
    the round (`payout == denomination − pool.fee` for every intent) — the direct
    amount-uniformity assertion, non-tautological;
  - **execute-time defense-in-depth (the fix-B check):** a *crafted* withdraw `Intent` whose
    `fee ≠ pool.fee` (built directly to bypass `commit_intent`, mirroring the existing stake
    `execute_round_stake_rejects_wrong_fee` test) → `execute_round` rejects it with
    `FeeNotUniform`. This is the test that proves the newly-added withdraw-arm re-check
    actually fires;
  - `initialize_pool` for a withdraw pool with a nonzero `fee ≤ denomination` **stores** it
    (offset/deserialize read) and `commit_intent` then **enforces** it;
  - `initialize_pool` for a withdraw pool with `fee > denomination` → rejected
    (`FeeExceedsDenomination`).
- **The existing withdraw-config test needs only a mechanical rename (confirmed against
  source).** `initialize_withdraw_pool_rejects_stake_params` passes `action_kind = 0` with a
  **nonzero validator** and `stake_fee = 0`, and asserts `WrongActionConfig` — it exercises
  **only the validator half**, with no fee-rejection assertion. Under D3 a nonzero validator is
  still rejected, so the change is purely mechanical: rename the trailing `stake_fee = 0`
  argument to `fee = 0`; the assertion is unchanged.
- The check is a one-line `require!` and the init bound is a trivial comparison — **no new
  pure fn** is warranted (the research's explicit call; YAGNI). The SBF-invisible branches
  here are trivial equality/inequality; no `invariants.rs` host-coverage addition is needed
  beyond the existing `split_payout`/`stake_split` tests.

## Explicitly OUT OF SCOPE (valid research — do not fold in, do not drop)

- **The full spec threat-model rewrite** (naming the passive on-chain observer + quantifying
  via the Plan-6b harness, frontier §2.5) — a separate edit to the main design spec.
- **The bond design rule** "key a membership bond to the deposit-commitment `C`, never to
  `recipient`" (§2.2) — for future bonding, which is DEFERRED until a Swap action gives a
  market-priceable payoff `V`.
- **The effective-k harness and `round_executable_slot`** — separate later plans (§5.2 items
  2 and 3).
- **Do NOT build** RLN, bonding-with-slashing, anonymity mining, operator-funded decoys, or
  cover/dummy traffic — all researched and **rejected** (§1, §2.3, §3.1–3.2, §4.3); name them
  as traps, do not implement.

## Citations

- **Nominal-cost anti-Sybil tax:** Bissias *et al.* 2014 (Xim, WPES 2014) — attacker cost
  grows linearly with the fraction occupied while honest cost stays flat (`[VERIFIED]`
  in-tree).
- **Amount / value linkage (the settlement fingerprint):** Kappos *et al.* 2018, "An
  Empirical Analysis of Anonymity in Zcash," USENIX Security (benthamsgaze) — the
  value-linkage precedent, **28.5% of coins linked**. This is a real published USENIX paper
  (firm ground, unlike the withdrawn 34.7% preprint), but the specific number is **supporting
  color, not load-bearing** — the core argument (variable fee → distinguishable payout) is a
  *structural fact about the code*, independent of any percentage. **[Confirm the 28.5% against
  the Kappos primary before it appears in the final bounty submission.]**
- **Primary adversary framing:** `anonymity-frontier-and-antisybil.md` §2.5 — the passive
  on-chain observer.
- **Priority-fee advisory (adjacent, NOT a build):** Solana relayer priority-fee as a
  gas-price-heuristic analogue — `[UNVERIFIED — reasoned by analogy]`.
- **Do NOT cite** the withdrawn "FIFO 34.7%" figure (Cristodaro *et al.* 2025,
  arXiv:2510.09433, `WITHDRAWN` 2025-11-18).

## Standards (CLAUDE.md / code-craft)

Fail-closed typed `PoolError` (append-only); comment *why*, not *what* (the one-line check
needs no narration beyond the fingerprint rationale); YAGNI (no new PDA/circuit/pure-fn/
config surface); conventional commits; spec → plan → TDD before code.
