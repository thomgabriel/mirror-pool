# Timeout-gated cancel — design (Plan 6a)

> Plan 6 was reordered (2026-07-17): the timeout-gated cancel ships first (this
> spec), then a minimal effective-k analytical core as the companion (Plan 6b).
> Rationale: `cancel_intent` is an ungated sub-k exit we already flagged as
> contradicting the k-anonymity claim — a hole in the core guarantee — whereas
> the effective-k harness only *measures* a residual whose fix is deferred. The
> code says the hole is the higher-value target.

## Problem

`cancel_intent` today (Plan 4/5) lets a committed participant reclaim their
denomination from an **open** round at **any time** before execution: round-Open
guard → refund to recipient → `intent_count -= 1` → close the intent PDA (the
nullifier PDA stays, so the note stays spent). There is no time gate.

That makes cancel a **free, instant sub-k exit**, with three concrete costs:

1. **Instant commit→cancel round-trip.** Deposit, commit, immediately cancel —
   a zero-cost probe/bypass that reclaims funds with no k-anonymity and lets an
   actor manipulate round composition at will.
2. **The k-claim is dishonest.** "k-anonymity over actions" is really
   "k-anonymity, except any participant can leave whenever" — the commitment is
   not enforced.
3. **Sybil-yank griefing is free.** An attacker holds `k−1` sybil intents in an
   open round; when a victim brings the count to `k`, the attacker cancels one
   sybil *before the crank lands*, dropping back to `k−1` so `execute_round`
   (which requires `≥ k_floor`) fails. Each yank is free (the deposit is
   refunded) and instantly replaceable, so the attacker can stall execution
   indefinitely and eventually force the victim into a linkable exit.

## What a timeout gate is — and is NOT

A timeout gate makes a committed intent **uncancelable for `N` slots after it is
committed**; cancel is permitted only once `current_slot ≥ committed_slot + N`.

**This is not a privacy fix, and the spec must not claim it is.** The honest
shape of the trade:

- **Cancel remains a linkable sub-k exit BY CONSTRUCTION.** A patient attacker
  commits, waits exactly `N` slots, and cancels — a fully scheduled, attacker-
  planned linkable exit at latency `N`. The gate does not remove this.
- **Its security is workload-contingent.** The only thing that defeats a patient
  exit is the round *executing within the attacker's window*, which requires
  `k−1` other honest commits plus the crank landing. In a busy pool the gate
  bites; in a quiet pool, a fresh pool, or a quiet weekend — conditions an
  attacker can choose — its added security rounds toward zero.
- **It trades honest-user liveness for attacker latency.** The same `N` that
  merely delays a patient attacker (who does not care) locks an honest user into
  a genuinely failed round (who does). That cost is bounded (`N` slots to a
  refund) and is the price of removing the instant exit.

## What the gate genuinely buys (the exact claim strength)

1. **Removes the instant commit→cancel round-trip.** The zero-cost probe/bypass
   is gone; the minimum round-trip is now `N` slots.
2. **Makes the commitment claim TRUE.** "You are committed for `N` slots after
   you join" becomes an on-chain-enforced statement, replacing today's implicit
   "…except you can leave whenever."
3. **Raises sybil-yank griefing from free to capital-locked-per-cycle.** Because
   cancel keeps the nullifier burned, each yanked sybil's note is *permanently
   spent*. Sustaining a stall now requires a pipeline of ripe (past-`N`) sybils,
   each a fresh deposit whose capital is locked ≥ `N` and whose nullifier is
   burned per yank; meanwhile every fresh replacement is uncancelable for `N`,
   during which a sustained `k` lets the crank execute normally. The attack goes
   from free/instant to capital-locked and nullifier-burning per cycle — a cost
   increase, not a prevention.

That is the entire claim list. Nothing about this gate strengthens the
anonymity of a *successful* (k-filled, executed) round, which already has no
sub-k exit.

## Design decisions

### Reference point: per-intent `committed_slot` (not round-level)

The clock counts from **each intent's own commit**, not from round creation.

- A round-level reference (`round.created_slot + N`) would make a participant who
  commits *late* into an already-old round **instantly** cancel-eligible —
  reopening the exact instant round-trip the gate exists to remove. Per-intent
  closes that: every participant waits the full `N` from their own commit,
  regardless of round age.
- Staggered per-participant deadlines within a round are correct, not a defect:
  cancel eligibility is inherently per-participant.
- Cost: one `u64` field on `Intent` (`committed_slot`), set in `commit_intent`.

### Unit: slots (not unix timestamp)

The gate uses the Clock sysvar's **slot** number.

- An adversary cannot accelerate the slot counter; network degradation only
  *lengthens* the wall-clock lockup. Slots therefore give a tamper-resistant
  **lower bound** on the lockup — the only bound a lockup needs.
- `unix_timestamp` is validator-voted and can be nudged within bounds; for a
  security lockup the tamper-resistant slot count is strictly preferable, at the
  cost of a less human-friendly duration (handled by the const's comment).

### Where `N` lives: a global const (for now)

`pub const TIMEOUT_SLOTS: u64 = 9_000;` — ~1 hour at 400 ms/slot.

- `N` is **load-bearing, not a tuning detail**: it means anything only if it is
  ≥ a credible fill horizon, so that "the round failed" is plausible by the time
  cancel opens. That points to hours, not minutes. `9_000` is stated as a
  **judgment call whose security value is workload-contingent**, not a derived
  number.
- **Honest caveat, already active:** different pools have different fill
  horizons *today* — stake pools fill slower than withdraw pools (the 1-SOL
  delegation floor thins the crowd), so one `N` is already not-forever with the
  two action kinds we have. The const carries a comment: *promote to a bounded
  per-pool config when fill horizons diverge (already true for stake vs
  withdraw); kept a const here to avoid unused config surface.*

## Considered and deliberately out of scope

- **A companion round-expiry / force-advance path.** `current_round_id` only
  advances on `execute_round`, so a failed round is never explicitly closed. We
  checked whether the gate needs to also expire/advance a timed-out round:
  **it does not.** New commits accumulate toward `k` in the *same* open round,
  and the gate rate-limits yanks, so a sustained `k` lets the crank slip through
  — a stuck round self-heals as long as honest arrivals outpace the (now-costly)
  yanks. A permanent stall requires arrivals to cease entirely, which is a
  crowd-depth problem owned by the incentive/coordinator layer, not this gate.
- **Per-pool timeout config.** Deferred to a bounded-config promotion (see the
  const caveat); a single honestly-caveated const is the YAGNI choice now.
- **Making cancel unlinkable / removing the sub-k exit entirely.** Impossible
  without a different exit primitive; explicitly not a goal here.

## Data-model & code changes

- `Intent` gains `committed_slot: u64` (`round.rs`; `Intent::SPACE` 121 → 129),
  set in `commit_intent` from `Clock::get()?.slot`.
- `commit_intent`: after recording the intent, set `intent.committed_slot`.
- `cancel_intent`: add the gate before the refund. Compute the earliest
  cancelable slot fail-closed and require the current slot has reached it:
  ```rust
  let unlock = intent.committed_slot
      .checked_add(crate::invariants::TIMEOUT_SLOTS)
      .ok_or(error!(PoolError::CancelTooEarly))?; // overflow → fail closed (cannot cancel)
  require!(Clock::get()?.slot >= unlock, PoolError::CancelTooEarly);
  ```
  The intent account is already in the `CancelIntent` context (bound
  `has_one = recipient`), so `committed_slot` is readable before the `close`.
- `PoolError` gains `CancelTooEarly`, **appended** after the current last variant
  (`StakeAccountInvalid`) — error codes stay append-only (6001/6002 in
  `deposit.rs` unshifted).
- `TIMEOUT_SLOTS` const lives in `invariants.rs` (host-visible, alongside the
  other protocol constants).

No change to `execute_round`, `Pool`, the vault flow, the circuits, or the SDK
proof path. The SDK `build_cancel_intent_ix` is unchanged (the gate is enforced
on-chain; the client just needs to submit after the timeout).

## Testing

LiteSVM lets tests set the Clock sysvar directly, so both gate directions are
cheap and deterministic — tests **warp**, they do not wait:

- `cancel at committed_slot + N − 1` → `CancelTooEarly` (specific error).
- `cancel at committed_slot + N` → succeeds (refund to recipient, `count -= 1`,
  intent PDA closed, nullifier stays burned → re-commit still fails).
- `execute_round` still succeeds when `k` is met, both before and after the
  timeout (the gate touches only cancel, never execute).
- **The existing cancel tests are updated to warp forward**: the Plan 4
  `cancel_intent` tests and the Plan 5 `cancel_intent_works_on_stake_pool` test
  currently cancel immediately; each now advances the Clock past `N` first. This
  is the seam-regression proof that cancel's refund/close/nullifier semantics are
  otherwise unchanged.
- Adversarial: the sybil-yank cost is asserted at the mechanism level (a yanked
  intent's nullifier stays burned, so the same note cannot be re-committed) —
  the economic claim is documented, the burn is tested.

## Honesty statement (for the spec's own claims section)

Cancel remains a **linkable sub-k exit by construction**. The timeout gate
(a) removes the instant version, (b) makes "committed for `N` slots" a true,
enforced statement, and (c) raises sybil-yank griefing from free to
capital-locked-per-cycle — and its bite is **proportional to pool traffic**,
which we state out loud rather than implying a uniform guarantee.
