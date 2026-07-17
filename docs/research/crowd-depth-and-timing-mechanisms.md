---
title: "Crowd-depth & timing mechanisms for mirror-pool — a build/no-build decision doc"
date: 2026-07-17
status: research (informational — synthesises four mechanism deep-dives into build-vs-defer verdicts for the mechanisms the frontier survey deferred; feeds the Plan 6b harness fixture, a Plan 6c timing/uniformity slice, and the anti-Sybil future-work note in the final design docs)
companion_to:
  - docs/research/anonymity-frontier-and-antisybil.md
  - docs/superpowers/specs/2026-07-15-mirror-pool-design.md
  - docs/superpowers/specs/2026-07-17-timeout-gated-cancel-design.md
  - docs/research/behavioral-rounds-followup-proposal.md
method: >-
  Four mechanism deep-dives (RLN · bonding at the ["member",pool,C_m] seam · cold-start &
  anonymity-mining · timing/intersection mitigations), each fact-checked against primary sources
  read in full where marked VERIFIED and flagged otherwise, synthesised into a single decision doc.
  Every mechanism is tethered to a concrete mirror-pool seam — a PDA, a pure fn, the circuit, or the
  CU/account-lock budget — read first-hand from the merged tree this session (invariants.rs, round.rs,
  state.rs, lib.rs, nullifier.rs, action.rs, withdraw.circom). Code-grounded, not written blind.
scope: >-
  For each deferred mechanism: does it deepen REAL (distinct-funder) k or only resist NOMINAL inflation,
  what does it cost against our budget, and BUILD / DEFER-AS-CITED-FUTURE-WORK / REJECT-AS-POOR-FIT. One
  through-line frames every verdict; a composition section names the minimal honest buildable subset and
  its order; a closing section preserves the load-bearing verification flags and maps each decision to a
  citation and a destination doc.
---

# Crowd-depth & timing mechanisms for mirror-pool

> **Purpose.** The frontier survey (`docs/research/anonymity-frontier-and-antisybil.md`)
> established the honest position: mirror-pool's on-chain gate
> `meets_k_floor(intent_count, k_floor)` (`programs/pool-program/src/invariants.rs:6`)
> counts **raw intents, not distinct funders**, so "the k-floor buys k *candidates*,
> not k-anonymity," and the spec books *Sybil / set poisoning* as "**Residual:** not fully
> solved" (`docs/superpowers/specs/2026-07-15-mirror-pool-design.md:181`). That survey ranked a
> menu but deferred the hard decisions. This document turns the deferred menu into concrete,
> cited **build/no-build engineering verdicts** for the frontier area — crowd depth (distinct-human
> k) and timing/intersection — where the current honest line is "closing the gap is future work."
>
> The organising claim, sharper than the survey's "counting is not hiding": **a single funder is a
> single unit of probability mass, however many slots, identities, bonds, or decoys it spreads
> across.** §0 makes that a lens; §§1–4 apply it to RLN, bonding, cold-start/mining, and timing;
> §5 names the minimal honest subset worth building and its order; §6 preserves the verification
> flags and maps each decision to the design docs.
>
> Verification flags from the source deep-dives (`[VERIFIED]`, `[UNVERIFIED]`, `[UNVERIFIED-PRIMARY]`,
> `[UNVERIFIED-SECONDARY]`, `WITHDRAWN`) are **preserved verbatim** — do not launder any flagged claim
> into a confident one when this lands in the design docs. In particular, **Cristodaro *et al.* 2025
> (arXiv:2510.09433, the withdrawn "34.7%" figure) is `WITHDRAWN` and is not cited anywhere in this
> document; do not reintroduce it.**
>
> **Grounding files read this session:** `programs/pool-program/src/{invariants.rs, round.rs, state.rs,
> lib.rs, nullifier.rs, action.rs}`, `circuits/circom/withdraw.circom`,
> `docs/superpowers/specs/{2026-07-15-mirror-pool-design.md, 2026-07-17-timeout-gated-cancel-design.md}`,
> `docs/research/anonymity-frontier-and-antisybil.md`.

---

## 0. The through-line — distinct-funder k, and the min-entropy lens that decides every verdict

### 0.1 The binding constraint, stated once

mirror-pool's unsolved constraint is **crowd depth measured in distinct humans (distinct funders),
not notes.** `meets_k_floor` counts `intent_count` — an integer of *intents*, blind by construction
to how many real-world entities funded them (`invariants.rs:6`, `round.rs:15-19`). A whale who posts
`m` independently-valid, mutually-unlinked commitments is, on-chain, cryptographically indistinguishable
from `m` honest joiners. That indistinguishability **is** the Sybil problem; it is not a bug in any one
mechanism, and no mechanism below converts it into a provable, slashable event — the most any of them
can do is *tax* it or *measure* it.

### 0.2 The lens: `k_∞` counts funders, not slots

The survey adopted min-entropy effective-k as the measurement (`anonymity-frontier-and-antisybil.md`
§1.2c), and it is the tool that decides every verdict in this doc:

```
k_∞ := 2^{H_∞(X)} = 1 / maxᵢ pᵢ          pᵢ = adversary posterior that funding-entity i initiated a given action
```

Grounded in Cachin 1997 (Prop. 2.3/2.4, `H_∞ = −log maxₓ P(x)`) and Dodis–Reyzin–Smith 2007 §2.1
(**predictability** `maxₐ P[A=a] = 2^{−H_∞(A)}` *is* the optimal single-guess success probability, by
definition) — both `[VERIFIED]`, full primary text, in the survey's appendix. The load-bearing property
of this metric: it scores **probability-mass concentration by funder**, so *one entity holding many
slots is one large mass, not many small ones.* Whale self-fill collapses it to `k_∞ = k/m` for `m`
self-filled slots.

### 0.3 The classification theorem this doc runs on

Because `k_∞` sees funders and not slots, every mechanism sorts into exactly one of four buckets by a
single question — **does it add distinct funders, or only slots controlled by an existing funder?**

| If a mechanism… | …then in the `k_∞` lens it is | Governs the verdict for |
|---|---|---|
| **adds slots controlled by one funder** | inert-at-best, adversarial-at-worst — the extra mass concentrates on that funder, *lowering* `k_∞` or leaving it flat | operator-funded decoys (§3), RLN as a crowd-depth fix (§1) |
| **taxes each slot linearly** | resists *nominal* inflation only — a whale is one funder who simply pays `m` times; `k_∞` is unmoved | bonding / fee-floor (§2) |
| **operates on a different axis (across-round timing), not within-round composition** | orthogonal to `k_∞` entirely — must not be sold as a crowd-depth fix | jitter / scheduling / cover (§4) |
| **adds genuinely distinct funders** | the *only* bucket that raises real `k_∞` | anchor-tenant partner whose own users fund (§3); proof-of-personhood in theory (defeated in practice — survey §3.4) |

**The two identities that frame the decoy/bootstrap verdicts (task-critical):**

1. **An operator funding `d` decoys is the whale with `m = d`.** The min-entropy formula does not
   change because the label on the funder changed from "attacker" to "treasury": `k_∞ = k/d` is the
   *identical* equation (deep-dive 3, §B.2). Worse, the operator has the *opposite* obfuscation
   incentive of a real attacker (treasury auditability, operational simplicity, honesty-to-a-judge all
   pull toward a *labeled* address), making it the single easiest entity in the whole pool to cluster —
   so the realistic case is not the benign "perfectly-hidden decoy" tie with an all-genuine round, but
   the adversarial `k_∞ = k/d` collapse.
2. **Decoys and self-fill collapse `k_∞` identically, so a "seed" that becomes a structural share of
   the pool *is* the whale-self-fill residual, re-labeled** — and it defeats the k-floor's one job:
   because `meets_k_floor` counts raw intents, a decoy is precisely the mechanism that lets
   `execute_round` fire on a round whose *genuine* population is below the floor the gate exists to
   enforce. Not "doesn't help" — **actively adversarial to the architecture's own core safety invariant.**

Every §3 bootstrap verdict is downstream of these two identities. Every §1/§2 verdict is downstream of
buckets 1 and 2. §4 is explicitly bucket 3 — a different axis, scoped honestly so it is never mistaken
for a crowd-depth mechanism.

### 0.4 Notation guard

`k` is reserved **exclusively** for mirror-pool's k-floor (the anonymity-set floor, `invariants.rs:6`).
RLN's own literature uses `k` for "messages allowed per epoch"; below that quantity is written **`L`**,
never `k`. `m` = a whale's self-filled slice; `d` = an operator's decoy count (`= m` in the lens);
`g = k − d` = distinct genuine funders. RLN's per-epoch message allowance `L` and mirror-pool's `k` are
unrelated numbers.

---

## 1. Rate-Limiting Nullifiers (RLN)

**Goal it appears to serve:** "one identity → one slot per round," so a whale cannot self-fill.
**One-line verdict:** **REJECT-AS-POOR-FIT** — not on cost (it fits the budget trivially) but because
it is *mechanically dominated* by a one-line PDA already in this codebase's idiom, *provably orthogonal*
to the whale-self-fill residual, and *regresses* a privacy strength the design deliberately holds.

### 1.1 Mechanism (compressed)

Semaphore-lineage (barryWhiteHat, ethresear.ch, 2019-02-18, `[VERIFIED, primary]`): a member stakes a
persistent identity commitment `Poseidon(a₀)` into a Merkle tree; per epoch, each signal reveals one
point `(x, y=A(x))` on a **degree-1** Shamir line `A(x)=a₁·x+a₀` with `a₁=Poseidon(a₀, externalNullifier,
messageId)` (PSE rln-docs, `[VERIFIED, primary]`). One signal = one point = perfect, *information-theoretic*
secrecy of `a₀` (Shamir 1979, `t−1` shares reveal zero — `[VERIFIED bibliographically]`). A **second**
signal reusing the same `(epoch, messageId)` slot yields a second point on the same line → anyone
Lagrange-interpolates `a₀` and slashes the stake (originally 33% to the discoverer, 67% burned —
`[UNVERIFIED for the current/2025 Waku deployment]`; only the 2019 proposal is directly confirmed).
`messageId ∈ 0..L−1` is what generalises "exactly 1" to "up to `L`" messages per epoch.

**Maturity, checked live 2026-07-17:** PSE's standalone reference implementation is **"Inactive" /
"sunset"** (`pse.dev/en/projects/rln`, `[VERIFIED, primary, fetched]`); the one live consumer, Waku-RLN-Relay,
was **still pre-mainnet in mid-2025** (Waku Monthly Update, published 2025-07-03, `[VERIFIED, primary]`),
with the mainnet/Linea/RLNv2 timeline surfacing only in **`[UNVERIFIED — secondary/search-synthesis]`**
results. The exact production variant (v1 `L=1` vs `messageId`-slotted v2) is **`[UNVERIFIED —
time-evolving]`**. **No Solana/Anchor port exists** — adoption means originating a new circuit + verifier +
registry from scratch, not porting an audited artifact.

### 1.2 Fit to our circuit — a new subsystem, not a parameter tweak

mirror-pool's note secrets `(nullifier, secret)` are **fresh per deposit** (`withdraw.circom:9-24`); RLN's
entire value presupposes a **persistent, cross-round** identity secret reused every epoch. So RLN cannot be
"add a public input to `withdraw.circom`"; it requires, from nothing (deep-dive 1, §B.2–B.3):

- a **second, persistent** identity secret `a₀` per human (distinct from every note's fresh secret);
- a **second depth-20 Merkle tree** of identity commitments + its own root-history ring (the note tree is
  single-spend by construction — the nullifier PDA burns it — so it cannot double as a *repeatable* identity
  proof);
- a **registration/bonding instruction** at the reserved `["member", pool, C_m]` seam (RLN's slash is
  meaningless against an unbonded identity);
- a **circuit extension** adding a second `MerkleProof(20)` + ~4 Poseidon calls — **≈ +24–25 Poseidon-gadget
  calls, roughly doubling the circuit's dominant cost**, so **client-side proving time roughly doubles**
  (a *prover-latency/UX* cost, the one axis the CU/account framing doesn't capture — worth naming separately);
  public inputs 3 → 6, proof size unchanged (Groth16 `a,b,c` = 256 bytes regardless).

### 1.3 Cost — fits the budget, which is *not* the reason to reject it

- **CU:** a 6-input verify ≈ **100–103k CU** vs the current 3-input ≈ 87k CU (both **derived-by-us** by
  interpolation from Light Protocol's already-in-repo `groth16-solana` benchmark table, *not* benchmarked
  here), a **~+13–16k CU delta**, paid **once per `commit_intent`**, never inside `execute_round`'s batch.
- **Account locks:** correctly placed at `commit_intent` (mirroring the nullifier PDA's per-participant
  placement), a per-epoch guard `["rln_signal", pool, round_id, identity_commitment]` (`init`-only, ~72 B)
  costs **zero of `execute_round`'s 64 account-locks** — k stays ≈19 (withdraw) / ≈17 (stake). Mis-placing it
  *inside* `execute_round` would cost +1–2 locks/intent (k → ≈14–15 or ≈10), but there is no design reason to.
- So "it doesn't fit the budget" would be a **false** rejection; stating it would be exactly the overclaim the
  project's honesty discipline forbids. The real objections are structural.

### 1.4 The four structural objections (why it is dominated, not merely redundant)

1. **Prevent > punish, on an ordered chain (deep-dive 1, §B.9).** RLN's reveal is *necessarily after the
   fact* because its home domain (Waku gossipsub) has no global order — a node cannot know "has this identity
   used this slot" before relaying. **Solana `commit_intent` is a single, globally-ordered, atomically-committed
   instruction** — precisely the precondition Waku lacks. A `["round_signal", pool, round_id, member_commitment]`
   `init`-PDA (the *exact* idiom `nullifier.rs` already uses: "the security property is the PDA's existence")
   makes the second attempt **fail atomically at submission** — the violating action never executes. RLN, ported
   as-is, would **let the second valid action execute** and only slash later. For a custody protocol, "the
   violating withdrawal never happens" is strictly stronger than "it happens and might later cost the violator
   money" — and it is what CLAUDE.md's "fail closed" doctrine demands.
2. **Provably orthogonal to the real residual (deep-dive 1, §B.8).** RLN's Shamir reveal ties together only
   points on the **same line**, keyed by one identity's own `a₀`. Two *distinct* Sybil identities give two
   independent lines; a whale who mints `k−1` fresh identities and signals **once each** never exceeds any single
   identity's `L=1` allowance, so the reveal **never fires**. RLN caps *same-identity replay* — which the per-note
   nullifier already prevents — and is **mathematically inert** against *distinct-identity inflation*, which is
   the actual open problem (bucket 1 of §0.3, demonstrated not asserted).
3. **Regresses an identified privacy strength (deep-dive 1, §B.7).** RLN's precondition — a persistent,
   staked, cross-round `a₀` in a public indexed tree — is *exactly* the stable cross-round handle the fresh-nullifier
   design deliberately denies a passive observer ("Repeated participation = literally Alice… fresh nullifier each
   time, so on-chain values don't self-link," survey §2.2). Adopting RLN doesn't just fail to help the composition
   axis; it **reopens the intersection channel** §4 is trying to keep closed.
4. **Substantial new surface for near-zero payoff.** Second circuit (~doubled prover time), second tree + root
   ring (~3.2 KB new state), a bonding/slashing module that doesn't exist and is reserved for a *different, better*
   purpose (§2). And if that bonding seam is ever built for that purpose, "one signal per member per epoch" falls
   out of the *same* PDA idiom for free — with no RLN content at all.

### 1.5 VERDICT

> **REJECT-AS-POOR-FIT** — a sharpening of the survey's "cite as future-work, different surface" (§3.1). The
> only surface where RLN is *not* strictly dominated is the **off-chain coordinator mempool** (no global order
> before a round batches) — but even there, mempool submission already requires a valid Groth16 proof of a real,
> unspent, denomination-locked note, so there is no free-broadcast spam channel like Waku's; ordinary per-IP /
> per-submission-fee HTTP throttling closes that residual far more cheaply than a second circuit + tree + bonded
> identity. **Keep RLN named as a trap to judges** (it is the mechanism most likely to *look* like the obvious
> fit), with language tightened from "future-work" to "considered and rejected — mechanically dominated by the
> existing nullifier-PDA idiom on an ordered chain, and provably orthogonal to the whale-self-fill residual."

---

## 2. Bonding at the `["member",pool,C_m]` seam

**Goal it appears to serve:** price a whale out of self-filling by charging per membership.
**One-line verdict:** **SPLIT — REJECT the bonded-membership-*with-slashing* mechanism as conceived; BUILD
instead the already-half-shipped `Pool.fee` uniformity extension** — because for *this* threat a refundable
bond is economically dominated by an equal-size non-refundable fee by 1–2+ orders of magnitude, and the
"slashing" half is unimplementable against self-fill under any non-custodial trust model.

### 2.1 The ceiling that drives every number (deep-dive 2, §1)

Self-fill is **not a protocol violation** — `m` valid `member_commitment`s from one whale are on-chain
indistinguishable from `m` honest joiners. So **no bond shape converts self-fill into a slashable event; it
can only tax it** (Xim's own framing — Bissias *et al.*, WPES 2014, `[VERIFIED]` in-tree: attacker cost grows
linearly with the fraction occupied while honest cost stays flat). Everything below is about how large that tax
can be, at what cost, without breaking a property the bond doesn't own.

### 2.2 Keying the bond — four of five options are privacy regressions (design rule)

`commit_intent`'s only public inputs are `[root, nullifier_hash, extDataHash]` (`lib.rs:169`,
`withdraw.circom:31`); the commitment `C` is **never** a public input. Against that boundary:

| Keying strategy | Surface | Privacy consequence | Verdict |
|---|---|---|---|
| **(A)** bond keyed to `nullifier_hash` | 1 PDA, 0 circuit | **Catastrophic** — ties the bonding wallet to the *exact future nullifier* before it is spent; worse than not bonding | Reject outright |
| **(B)** bond keyed to `intent.recipient` | 1 acct/intent | **Severe** — gives the deliberately-pristine payout address inbound history *before* the payout, reopening the address-freshness signal (Béres H1, Tang H1) | Reject |
| **(C)** bond keyed to deposit commitment `C`, proven via a 2nd Merkle root over the same hidden leaf | +1 public input + 1 path witness to the *existing* circuit | **Sound** — `C` is already public at deposit and already funder-linked (`deposit`'s `payer: Signer`) | Only clean option; see cost §2.5 |
| **(D)** independent 2nd identity system (spec's literal `C_m`-before-deposit reading) | whole new Poseidon/nullifier/tree | Sound but strictly more surface than (C) for no extra guarantee | Dominated by (C) |
| **(E)** `min_fee` on the existing `Intent.fee` (no bond, no PDA) | 0 new accounts, 0 circuit | None beyond what `fee` already exposes | **The recommendation, §2.6** |

**Design rule this produces (record it even under a reject):** *a membership bond, if ever built, must key to
the **deposit-time commitment** `C`, never to the **execute-time recipient**.* Keying to `recipient` silently
reopens the address-freshness property the exit path depends on — this corrects the spec's own JOIN-before-DEPOSIT
sequencing (option D) toward the cheaper, safer option (C).

### 2.3 Slashing — the negative result (deep-dive 2, §3)

No trust model yields a self-fill-specific slashing condition:

- **Atomic on-chain guard** (the house style) needs a *cryptographically provable* violation; each Sybil intent is
  independently valid — **nothing to guard.**
- **Privileged authority on off-chain clustering evidence** reintroduces the exact **"global auditor / compliance
  backdoor"** the spec's non-goals reject (`2026-07-15-mirror-pool-design.md:250`), and clustering is *probabilistic*
  (two friends funding from one CEX withdrawal are indistinguishable from a whale) — a slashing authority *will* have
  false positives, i.e. can confiscate honest bonded capital on manufactured evidence. **Strictly worse custody
  surface than today.**
- **Permissionless RLN-style reveal** is inert against distinct-identity self-fill (§1.4 obj. 2), verbatim.

The only genuinely slashable/frictionable events in this whole space are ones **already shipped**: double-spend
(atomic, `nullifier` PDA `init`) and commit-then-yank griefing (priced by `cancel_unlock_slot` / `TIMEOUT_SLOTS`,
`invariants.rs:53-67`, commit `3758cd9`). A membership bond adds **zero new slashable surface.**

### 2.4 Economics — a refundable bond is dominated by an equal-size fee (deep-dive 2, §4–5)

Let `m = k−1` slots to collapse the lone honest participant, `B` = bond, `f` = fee, `r ≈ 6–7.5%` Solana staking
APY (opportunity-cost proxy; `[secondary, cross-consistent, not load-bearing]`), `D` = lock duration.

| Mechanism | Attacker cost for `m` slots | Real deterrent? |
|---|---|---|
| `fee` today (withdraw pools allow `fee=0`) | `0` | **No — the live gap** |
| `fee` floored at `f_min`, non-refundable | `m·f_min` | 100% at-risk per honest-SOL spent |
| Refundable bond, short window (Δt ≈ 1h) | `m·B·r·Δt` | **~125,000× weaker** than an equal fee |
| Refundable bond, 21-day unbond (Cosmos) | `m·B·r·D` | **~250× weaker** |
| Refundable bond, 1-year unbond | `m·B·r·D` | **~14× weaker** (and no precedent; even Cosmos is 17× shorter) |
| Bond + slashing | `m·B·P_slash` | `P_slash = 0` for self-fill (§2.3) → collapses to the row above |

Cosmos's 21-day unbond (`x/staking` README, `[VERIFIED, primary]`) exists to preserve a slashing-evidence window
for a *provable* offense (equivocation); §2.3 shows **no provable offense exists for self-fill**, so importing a long
unbond imports a mechanism whose *justification does not transfer.* And the sizing rule `(k−1)·c ≥ V` is **currently
unanchored**: the deanonymization payoff `V` for the actions mirror-pool actually ships (withdraw, stake) is a
*targeted-surveillance* value set by adversary motive, with **no protocol-computable ceiling** — unlike a future
*Swap* action where `V` is slippage/MEV-bounded. At mirror-pool's own `k ≈ 17` and a generous affordability ceiling
`A = 1 SOL`, a bond deters only `V ≤ 16 SOL`, while the real `V` is unbounded — **the feasibility window cannot be
shown non-empty.**

### 2.5 Linear ≻ quadratic ≻ progressive, and the cost of the sound variant (deep-dive 2, §6–7)

**Linear-per-membership** is the correct primitive (Xim): both pay the same per-unit price, but honest needs 1 unit,
whale needs `m`. **Quadratic** (Lalley–Weyl 2018, `[VERIFIED]`) and **progressive** pricing are **not implementable
here** — they require the chain to recognise "this buyer's *n*-th purchase," which is the very distinct-identity signal
Sybil resistance is trying to manufacture; a whale presents `m` fresh unlinked commitments, each a "first buy," so the
progressive price never triggers. **Quadratic pricing presupposes the identity axis it would help build — a circular
dependency**, structurally the same failure shape as RLN. (Scope-correction, deep-dive 2, §6: Mazorra–Della Penna 2023,
arXiv:2301.12813, governs *reward-sharing* mechanisms — Prop. 2.3, "pie shrinking with crowding" — **not entry-cost
bonds**; cite it for the deferred cover-reward module, not here.)

The one sound bond variant (§2.2 option C) costs: +1 public input + 1 Merkle-path witness on the existing circuit ⇒ a
**re-run phase-2 trusted-setup ceremony** (phase-1 reusable — real, non-trivial); a `join_pool`/`leave_pool` surface that
**does not fit `PooledAction`** (it is not a round-executed effect — `action.rs:18`), i.e. a *second* extension seam
outside the one CLAUDE.md sanctions. If instead built as the cheap privacy-broken variant (A/B) checked inside
`execute_round`, the per-intent account count rises 3 → 4, dropping withdraw `k` from `⌊(64−6)/3⌋≈19` to `⌊(64−6)/4⌋≈14`
— a direct, quantified **shrinkage of the very anonymity set the mechanism exists to protect.**

### 2.6 VERDICT + design sketch

> **REJECT** the `["member",pool,C_m]` bonded-membership-with-slashing mechanism as conceived: no slashing condition
> exists for self-fill (§2.3); every privacy-safe keying needs a modified circuit + phase-2 ceremony (§2.2, §2.5);
> refundable bonds are dominated by fees by 1–2+ orders of magnitude (§2.4); it doesn't fit `PooledAction`; the
> feasibility window cannot be shown non-empty. **DEFER-AS-CITED-FUTURE-WORK only the narrow "raise the barrier for a
> capital-*constrained* attacker" framing**, priced as a *linear* bond (Xim / Mazorra–Della Penna for the reward side),
> and **not before a second concrete caller** (a Swap action with a market-priceable `V`) makes §2.4's sizing tractable.
>
> **BUILD instead — the minimal, already-precedented `Pool.fee` extension** (this is the same field §4(d) arrives at from
> the amount-uniformity side; §5 unifies them). `stake_fee` is already mandatory and pool-fixed for stake pools
> (`lib.rs:151-153`); for **withdraw pools `fee` is a free per-intent choice and a whale can set `fee=0` today**. This is
> "the second concrete caller" YAGNI asks for before generalising a pattern — Plan 5 built the first. It also closes an
> adjacent amount-uniformity leak (variable `fee` ⇒ variable `payout = denomination − fee` across a round). Attacker cost
> becomes `m·pool.fee`, non-refundable, 100%-at-risk — the best achievable version of this mechanism class, and it must be
> stated plainly as a **nominal-cost tax, not a real-k mechanism** (§0.3 bucket 2).

```rust
// (a) The REJECTED mechanism, best-engineered form (§2.2 option C) — for the record, NOT recommended:
#[account]
pub struct MemberBond { pub pool: Pubkey, pub bonded_amount: u64, pub bonded_slot: u64 }
// seeds = ["member_bond", pool, commitment]   // keyed to the DEPOSIT commitment, NEVER `recipient` (§2.2 design rule)
// Requires circom changes + a NEW phase-2 ceremony. No slash_member is sketched: §2.3 shows no self-fill-provable
// condition gates one, and a discretionary one is a custody regression, not a fix.
pub fn unbond_unlock_slot(bonded_slot: u64, lockup: u64) -> Result<u64> {   // mirrors cancel_unlock_slot's shape
    bonded_slot.checked_add(lockup).ok_or(error!(PoolError::UnbondTooEarly))
}

// (b) The RECOMMENDATION (§2.6, == §4(d)) — generalise the shipped stake_fee into ONE mandatory pool-wide fee:
// state.rs  — replace `stake_fee` with a single pool-wide `fee: u64` (both u64, 8-aligned → clean rename, no new gap)
// lib.rs    — commit_intent: collapse the action-kind branch into one unconditional check, deleting the withdraw special case:
//   require!(fee == pool.fee, PoolError::WrongActionConfig);   // both action kinds; withdraw pools wanting no relayer fee pass fee = 0
// No new PDA, no new circuit, no new pure fn (the check is a one-line require!, matching lib.rs:151-153's precedent).
```

---

## 3. Cold-start bootstrapping + anonymity mining

**Goal each appears to serve:** deepen a thin pool (decoys/seeding) or reward good behavior (mining).
**One-line verdicts:** anonymity mining **REJECT-AS-POOR-FIT** (leaks by construction); operator-funded decoys
**REJECT-AS-POOR-FIT** (they *are* the whale, and defeat the k-floor's one job); anchor-tenant partner integration
**DEFER-AS-CITED-FUTURE-WORK** (the one pattern that adds *distinct funders*); a harness fixture **BUILD**.

### 3.1 Anonymity mining — the theorem, and the honest number (deep-dive 3, part a)

Tornado's mining (2020-12-18 → 2021-12-18, 10% of TORN over one year) was *not* a naked public claim: claiming AP was
itself a Groth16 proof writing into a shielded account, and the designers **explicitly rejected plain liquidity mining**
because it "would force users to reveal how much time their deposits spent in a Tornado.Cash pool" (Tornado governance
proposal, `[VERIFIED]`). **It leaked anyway**, and the causal chain is a theorem, not an implementation bug:

> the reward rate is a **public constant**, so the instant the claimed VALUE touches any unshielded context (an AP→TORN
> swap, spending it, a counterparty identifying the address), an observer inverts *value → exact dwell-time → the narrow
> set of deposit blocks* — Wu *et al.* 2022 ("Tutela"), §6.5, arXiv:2201.06811, `[VERIFIED]`, quoted via ar5iv (PDF binary
> unparsable this session).

Generalised: **any reward that is (public rate) × (duration/count of participation), realized in a form that ever touches
an unshielded context, is an information leak by construction** — about the reward's mathematical shape, independent of how
well the claim transaction itself is shielded.

**The honest magnitude, resolving the survey's `[UNVERIFIED]` flag.** Tutela §7.2 (`[VERIFIED]`, verbatim): of 97.3k equal
deposits, 42.8k potentially compromised, with the **TORN-mining reveal contributing 358** — the *smallest* of the five
heuristics (linked-address 18.9k; address-match 18.6k; multi-denomination 16.2k; gas-price 102). **This SUPERSEDES the survey's
`[UNVERIFIED]` TORN-mining sub-percentage (`anonymity-frontier-and-antisybil.md` §3.2) with a `[VERIFIED]` 358/97.3k** — an
honest upgrade, and it forces reporting *both* facts: the mechanism is a proven, designer-anticipated-yet-still-realized leak,
*and* the isolated damage was small (it is hardest to compute for pooled multi-deposit claims). The wash-trading half — did
mining inflate nominal deposits — has **no rigorous isolated measurement in the literature (`[UNVERIFIED — gap, not a finding]`);
do not assert a percentage.** 2021 TVL growth (~$339M→~$517M, `[UNVERIFIED — secondary aggregator]`) is confounded by the bull
market.

> **VERDICT: REJECT-AS-POOR-FIT** for any mirror-pool incentive mechanism. Confirms the already-adopted "pay silently"
> house position (`behavioral-privacy-industry-practices.md` §3; Penumbra-style appreciating accrual) at higher confidence.
> No code follows: the deferred incentive module (spec §3.4/§7) must **never** implement a claimable, duration/count-keyed reward.

### 3.2 Operator-funded decoys — the operator IS the whale (deep-dive 3, part b)

Run the §0.2 lens on `d` operator-funded slots out of `k`:

- **Case 1 (decoys perfectly indistinguishable):** `maxᵢ pᵢ = 1/k`, `k_∞ = k` — a **tie with an all-genuine round, never an
  improvement** (a perfectly-hidden decoy is one unit of mass, like a perfectly-hidden genuine participant).
- **Case 2 (the `d` decoy sources cluster to one entity — realistic):** a genuine participant's posterior worsens to `1/(k−d)`,
  and for the operator `maxᵢ pᵢ = d/k` ⇒ **`k_∞ = k/d` — the whale-self-fill formula with `m = d`, identical equation.**

Case 2 is the realistic one because the operator has the **opposite** obfuscation incentive of a real attacker (§0.3): treasury
auditability and honesty-to-a-judge pull toward a *labeled* address, the single easiest entity to cluster under exactly the
production chain-analysis (Chainalysis/Elliptic Solana clustering; Trusta's "same funding source" is its #1 Sybil pattern) this
repo's own threat model names. **And decoys defeat the k-floor's one job:** because `meets_k_floor` counts raw `intent_count`
(`invariants.rs:6`) with no way to tell a decoy from a genuine intent (by the same indistinguishability Case 1 needs), a decoy is
precisely the mechanism that lets `execute_round` succeed on a round whose genuine population is below the floor the gate exists to
block — **actively adversarial to the core safety invariant, worse than doing nothing.**

**No PDA seam exists or should exist.** Indistinguishability *requires* a decoy to traverse the identical `commit_intent` → real
Groth16 proof → `["nullifier", …]` pipeline (`lib.rs:134-193`); a cheaper operator-only bypass would be trivially distinguishable by
transaction shape. There is no invariant arithmetic to encode, so no pure fn belongs in `invariants.rs` — the *absence* of a seam is
itself evidence this is the wrong layer. The costs are real and recurring: each decoy locks a full `denomination` (≥1 SOL
`MIN_STAKE_DELEGATION` for stake, `invariants.rs:32`), pays a full `commit_intent` verify (~78k–109k CU), occupies one of the ~17–19
per-tx intent slots a genuine participant could use, and is largest exactly when the pool is weakest. Above all it is a **trust-model
regression**: nothing on-chain distinguishes a decoy, so nothing stops an operator running a permanently-padded pool and presenting
it as organic — the operator becomes, for the first time, a hidden participant grading its own anonymity metric.

> **VERDICT: REJECT-AS-POOR-FIT** — structurally identical to whale self-fill (`k_∞ = k/d`), realistically clusterable, actively
> defeats the k-floor, introduces a hidden-conflict-of-interest the design has zero surface for. Name it to judges as a **trap**
> alongside RLN and mining.

### 3.3 What actually deepens real k — and the one buildable artifact

- **Anchor-tenant / partner integration is the one pattern in bucket 4** (§0.3) — but *only* if the partner's **own distinct users**
  fund their own genuine actions (each a distinct funder → adds to `g`), not if the partner's treasury pads on their behalf (that is
  §3.2 relabeled). The seam is trivial and needs no new mechanism: `PooledAction` (`action.rs:18`) is already the sanctioned "adding
  a protocol = one adapter" point, and a partner is just **more `commit_intent` callers.** Caveats: a partner too small to bring more
  than a handful of its own users is a differently-labeled whale (evaluate expected volume against the same `m/k` math); its action
  shape must fit the bucketed-denomination model. **No partner currently exists — a business-development dependency, not a code gap.**
- **Protocol-mandated uniformity is already built:** the k-floor *is* mirror-pool's Monero-style cold-start answer (refusing thin/
  non-uniform execution removes the cold-start problem rather than papering over it). Recognise, don't rebuild.
- **BUILD — a test-fixture requirement on the already-planned Plan 6b harness** (`crates/anonymity-harness`, host-side, zero custody
  surface): any funder-clustering partition the `k_∞` pure fn consumes **MUST treat a labeled operator/treasury address as just another
  entry in the same clustering partition** used for the whale-self-fill check — never an exempted category. A property-test fixture where
  *"the whale" is the treasury address* is mandatory, so the harness cannot later be gamed by an operator assuming its own top-ups are
  exempt from the concentration gate. Zero new PDAs, zero on-chain surface — closes the one blind spot §3.2's math exposes.

| Question | Verdict | Why (in the §0.3 lens) |
|---|---|---|
| Anonymity mining as an incentive | **REJECT-AS-POOR-FIT** | Public-rate × secret-duration leak by construction; isolated damage small (358/97.3k `[VERIFIED]`) but the mechanism is unsound; wash-trading question is an honest gap |
| Operator-funded decoys (on-chain) | **REJECT-AS-POOR-FIT** | Bucket 1: `k_∞ = k/d`, the whale re-labeled; defeats the k-floor; trust-model regression; no seam exists or should |
| Anchor-tenant partner (own users fund) | **DEFER-AS-CITED-FUTURE-WORK** | Bucket 4 — genuinely adds `g`; needs no new trait/PDA; business-dev dependency, not code |
| Protocol-mandated uniformity (the k-floor) | **Already built — recognise** | Removes cold-start; zero marginal cost |
| Harness fixture (operator-as-clustered-funder) | **BUILD — when Plan 6b lands** | Host-only, closes the decoy blind spot, fits the specced `k_∞` API exactly |

---

## 4. Timing / intersection mitigations → a Plan 6c

**Axis reminder (§0.3 bucket 3):** timing is the **across-round intersection** axis, orthogonal to the within-round composition
axis §§1–3 live on. It must **never** be sold as a crowd-depth fix. Verdicts: per-intent jitter **DEFER (superseded)**; round-level
scheduling **BUILD (Plan 6c)**; cover/dummy **REJECT-AS-POOR-FIT**; denomination/fee uniformity **BUILD (Plan 6c)**.

### 4.0 The load-bearing correction — what timing control actually defends (deep-dive 4, §0)

The survey framed commit→execute jitter as defending "the #1 short-Δt heuristic," but that is imprecise for *our* architecture and
the imprecision changes the design. `execute_round` is **one atomic transaction** — every intent in a round shares the **exact same
execution slot** (`lib.rs:259-449`, one `Clock::get()?.slot`), so there is no "which output landed soonest" question *within* a round;
batching already answers it identically for everyone. And `committed_slot` is a **plaintext public field** on `Intent` (`round.rs:43`) —
jitter cannot hide what was never hidden. What round-level *timing control* genuinely defends is the **(n−1) / trickle attack**
(Serjantov–Dingledine–Syverson 2002, IH 2002, `[VERIFIED, read in full]`): because `execute_round` is **permissionless** (`cranker: Signer`,
no authority constraint, `lib.rs:633-634`) and fires the instant `intent_count ≥ k_floor`, an attacker owning `k−1` slots can watch for a
victim's `commit_intent` and crank in the next slot, isolating the victim with **posterior = 1** — *stronger* than the static `m/k`
whale-self-fill model. This is the attack (a) and (b) are actually designed against.

### 4.1 (a) Per-intent jitter — as literally specified, and why it collapses into (b)

The literal ask, in the `cancel_unlock_slot` mould (`invariants.rs:63-67`):

```rust
pub fn eligible_slot(commit_slot: u64, jitter: u64) -> Result<u64> {
    commit_slot.checked_add(jitter).ok_or(error!(PoolError::IntentNotYetEligible))   // fail-closed on overflow
}
```

with `execute_round` additionally requiring `current_slot ≥ eligible_slot(intent.committed_slot, jitter_i)` for every live intent.
Because `execute_round` requires **every** live intent present (`rem.len() == count*3`, `lib.rs:312`), the round's earliest execution slot
is `maxᵢ(eligible_slotᵢ)` — driven by whichever intent committed **most recently** under optimal attacker play, which is expressible as
**one round-level timestamp** instead of `k` independent per-intent draws. Worse, `k` i.i.d. draws have an expected maximum that **grows with
`k`** (order statistics) — a liveness cost that worsens as `k` grows, exactly backwards for a k-anonymity system — and pay the entropy-sourcing
tax `k` times for no extra defense. **Do not build the per-intent variant; build the underlying primitive at round granularity.**

### 4.2 (b) Round-level scheduling — the mechanism to build (deep-dive 4, part b)

**New `Round` state** (`round.rs:16-23`, `SPACE` 13 → 29; +16 B):

```rust
pub struct Round {
    pub state: RoundState, pub intent_count: u32,
    pub last_activity_slot: u64,     // NEW — stamped every commit_intent
    pub k_floor_reached_slot: u64,   // NEW — stamped ONLY on the transition count crosses k_floor
}
```

**Pure fn** (`invariants.rs`, `cancel_unlock_slot` mould, host-tested, fail-closed):

```rust
pub const ROUND_DWELL_LOOKAHEAD_SLOTS: u64 = 40;   // ~16s @ 400ms/slot — floor before jitter is even readable
pub const ROUND_DWELL_MAX_SLOTS: u64 = 450;        // ~3min ceiling since k-floor first met — judgment call, not derived
pub fn round_executable_slot(last_activity_slot: u64, k_floor_reached_slot: u64, jitter: u64) -> Result<u64> {
    let quiet_gate = last_activity_slot.checked_add(ROUND_DWELL_LOOKAHEAD_SLOTS)
        .and_then(|s| s.checked_add(jitter)).ok_or(error!(PoolError::RoundNotYetExecutable))?;
    let ceiling_gate = k_floor_reached_slot.checked_add(ROUND_DWELL_MAX_SLOTS)
        .ok_or(error!(PoolError::RoundNotYetExecutable))?;
    Ok(quiet_gate.min(ceiling_gate))
}
```

`execute_round` adds, after the existing `meets_k_floor` check, `require!(current_slot ≥ round_executable_slot(…), …)`; one new
`PoolError` variant **appended after `CancelTooEarly`** (the append-only convention the timeout-cancel plan established,
`timeout-gated-cancel-design.md:175`), double-dutying for the too-early and overflow branches exactly as `CancelTooEarly` does.

**Why AND, not OR — the load-bearing decision (SDS 2002 §3.3–3.4):** a Threshold-*or*-Timed mix inherits the *worst* of both parents — the
timed side alone lets an attacker isolate a target with **zero inserted messages** by delaying everyone else until the clock fires (min
anonymity = 0). A Threshold-*and*-Timed mix guarantees a **minimum anonymity floor of `k`** because the threshold is never waived.
`round_executable_slot` is deliberately AND-shaped: the shipped `meets_k_floor` gate is an un-bypassable prerequisite, and
`ROUND_DWELL_MAX_SLOTS` only bounds the *additional* wait on top of an already-met threshold — it is not an independent trigger.

**Entropy, non-grindably:** read `SlotHashes[last_activity_slot + ROUND_DWELL_LOOKAHEAD_SLOTS]` — a slot *after* the most recent commit,
so it does not exist when any committer submits (Anchor needs an explicit `UncheckedAccount` + manual byte-offset read; `Sysvar<SlotHashes>`
deliberately unimplemented, `MAX_ENTRIES=512`, both `[VERIFIED, primary]`). Reduce via wide-multiply/rejection sampling, **not naive `%`**
(modulo bias would reintroduce the predictability the mechanism removes). **Honest grinding caveat:** non-grindable by an ordinary committer
(a wallet cannot choose its landing slot), but a leader/Jito-bundle-capable attacker who influences which slot their own tx lands in has more
control — the same "leader biases blocks they produce" caveat every Solana on-chain-randomness use accepts (`[UNVERIFIED-SECONDARY]`, consistent
with the primary fact that `SlotHashes` is bank-state-derived). Not a per-participant fingerprint: `SlotHashes` is one global account, and the
result applies uniformly to every intent in the round.

**Budget:** +1 *shared* `SlotHashes` account on `execute_round` (**k-ceiling unchanged both ways** — `⌊(64−7)/3⌋=19`, `⌊(64−13)/3⌋=17`);
`Round` +16 B; est. **<1,000 CU** for the binary search (**unmeasured — recommend a LiteSVM pass exactly like the existing 26,247 / 56,800 CU
measurements**). Zero circuit, zero SDK, zero effect on `cancel_intent`. The reset-on-every-commit design is not a free griefing vector — each
reset costs the resetter a real, fully-funded, nullifier-burning deposit (the same economics the timeout-cancel design already prices), and
`ROUND_DWELL_MAX_SLOTS` caps the total extension.

**Honest claim strength (do not overclaim):** `round_executable_slot` is a **Threshold-and-Timed mix in SDS's own taxonomy — not a pool mix.**
It (1) never fires below `k_floor` (min anonymity = k) and (2) raises the attacker's minimum sustained lock-and-wait from ~1 slot to
`ROUND_DWELL_LOOKAHEAD_SLOTS…MAX_SLOTS`, during which any organic arrival re-extends the window. It is **not** honest to claim it converts the
(n−1) attack from *exact/certain* to *uncertain* — SDS show a Threshold-and-Timed mix's attack is **still exact, just costlier** (max `2(k−1)`
messages, max `2t−ε` time). Only a **true pool mix** (probabilistic multi-round retention) gets "uncertain" + SDS's 5–9× cost multiplier — and it
is a genuinely bigger change (breaks the "every live intent executes the round it reaches k_floor" invariant, needs a carry-over structure,
interacts with `cancel_intent`). **Cite that as future work, not Plan 6c.** A permanent architecture limit worth stating: `Round.intent_count` is a
**public** field, so mirror-pool can obtain SDS's *uncertainty* (timing) lever but **never the *inexactness* lever** (how many are held) — that
is unavailable on any public ledger, not a gap this or a future mechanism closes.

### 4.3 (c) Cover / dummy traffic — reject (deep-dive 4, part c)

The strongest academic rescue is **receiver-bound cover** (Mallesh–Wright, ESORICS 2007, `[VERIFIED, read in full]`): the *system* generates
dummies mimicking real senders — but their own quantified overhead is **100–300% of real traffic volume**, and it is affordable *there* because
their unit of cover is a **message** (bandwidth, silently droppable at an uncredentialed recipient, zero marginal capital). mirror-pool's unit of
cover is a **custodial, fixed-denomination, ZK-membership-gated, value-conserving on-chain action** — a decoy `commit_intent` needs a real,
previously-deposited, fully-funded note (`verify_withdraw`, `lib.rs:169`) and a real `execute_round` transfer; there is no "drop the dummy at the
destination." Sustaining Mallesh–Wright's 100–300% means the **treasury continuously holds 1–3× the pool's real volume in locked principal, cycling
full deposit→commit→execute lifecycles** (each ~26,247–56,800 CU) — and doing so makes the operator the *largest* funder in the pool, i.e. **§3.2's
whale-self-fill with the operator as the whale.** (User-generated cover is even weaker — M&D 2004: sender dummies alone don't protect against a
background-aware adversary, and a public chain's background is always fully known.) The authors themselves call the defense/attack balance unresolved
(§8, `[VERIFIED]`).

> **VERDICT: REJECT-AS-POOR-FIT** — a structural cost-model mismatch, not a "not now." The general claim (system-generated cover can beat naive user
> cover) is real and credited, but the cost structure that makes it affordable does not exist in a custodial fixed-denom pool, and the version that
> would make it affordable recreates the worst-diagnosed residual on file.

### 4.4 (d) Denomination / action-shape uniformity — a real, shipped gap (deep-dive 4, part d)

Fixed `denomination` already forecloses *principal* fingerprinting (`lib.rs:97`). **The payout split is not fixed.** For stake pools `fee` is forced
uniform (`fee == pool.stake_fee`, `lib.rs:151-153`); for **withdraw pools `fee` is a free per-intent choice ≤ denomination**, and
`WithdrawAction::execute` issues **two separate, publicly-visible transfers** per intent (`split_payout(denomination, fee)` → `(payout, fee)` to
`(recipient, relayer)`, `action.rs:34-64`). So **any two withdraw intents in the same round with different `fee` are pairwise distinguishable by output
amount alone, with zero timing analysis** — and a user/relayer pairing that consistently uses a fixed non-default fee becomes a **stable cross-round
fingerprint**, structurally the Zcash 249.9999→250.0001 ZEC value-linkage finding (28.5% of coins linked, `benthamsgaze.org`, in-tree). This is
**exploitable today with no active adversary — just two different fees in one round.**

> **VERDICT: BUILD — Plan 6c.** The fix is *the same field §2.6 recommends from the anti-Sybil side*: generalise `stake_fee` into one mandatory
> pool-wide `Pool.fee` (+8 B, clean rename), and collapse the action-kind branch in `commit_intent` into one unconditional
> `require!(fee == pool.fee, …)` — which also **deletes a special case** (today's withdraw path has no fee constraint), matching CLAUDE.md's
> "match the surrounding code / no dead asymmetry." <100 CU, no PDA, no circuit. **Advisory-only adjacent item, not a build:** relayer priority-fee
> (compute-unit price on `commit_intent`) is a Solana analogue of Tutela's gas-price heuristic, a client choice the protocol cannot constrain
> on-chain — **`[UNVERIFIED — reasoned by analogy to a verified Ethereum-mixer finding]`**; same advisory category as destination-reuse guidance.

---

## 5. Composition — the minimal honest subset, in order

### 5.1 The convergence insight

Two independent deep-dives, from opposite motivations, arrive at the **same one change to `Pool`**: deep-dive 2 wants a mandatory
pool-wide fee as the **cheapest nominal-cost anti-Sybil tax** (§2.6); deep-dive 4 wants it as an **amount-uniformity fix** closing a
*currently-shipped* withdraw-pool fingerprint (§4.4). Both are "generalise the Plan-5 `stake_fee` into one mandatory `Pool.fee`, unify both
action-kind branches, delete the withdraw special case." One +8-byte field, one deleted asymmetry, serves both — and it is already half-shipped
(the stake path is the first caller, so YAGNI's "second concrete caller" bar is met). **This is the strongest single build item in the whole
frontier, and it does not touch the binding constraint** — it must ship labeled as a nominal-cost tax + fingerprint fix, never as a crowd-depth
solution.

### 5.2 Build order (smallest-blast-radius first)

1. **`Pool.fee` uniformity** (§2.6 == §4.4). Smallest surface: +8 B on the zero-copy `Pool`, one unconditional `require!`, no PDA/circuit/pure-fn,
   <100 CU. Closes an exploitable-today amount fingerprint *and* installs the cheapest anti-Sybil tax. (Migration note, per the timeout-cancel spec's
   own discipline: a `Pool` layout change is not backward-compatible with existing accounts — moot pre-launch, must land before live custody.)
2. **`round_executable_slot`** (§4.2), a Plan 6c timing slice. New pure fn in the `cancel_unlock_slot` mould + `Round` +16 B + one shared `SlotHashes`
   account (k-ceiling unchanged). Honestly raises the cost/latency of SDS's *exact* (n−1) isolation; does **not** make it *uncertain*. Needs a LiteSVM
   CU-measurement pass before the claim is finalised.
3. **Plan 6b harness fixture** (§3.3), host-only, zero custody surface: the `k_∞` clustering partition must treat a labeled operator/treasury address
   as just another entry, with a mandatory "the whale is the treasury" property-test. Lands with Plan 6b.

Items 1 and 3 are sub-task-sized and independent; item 2 is a Plan 6c of its own weight. Nothing here adds custody surface beyond item 1's +8 B, and
nothing claims to deepen real k.

### 5.3 What stays cited-future-work (priced, not built)

- **Linear bonding at `["member",pool,C_m]`** (§2.6) — only when a second caller with a market-priceable `V` (a Swap action) makes §2.4's sizing
  non-empty; keyed to the deposit commitment `C` per §2.2's design rule; no discretionary slash path.
- **True pool mix** (§4.2) — the mechanism that actually converts the (n−1) attack from exact to *uncertain* (SDS's 5–9× multiplier); a bigger change
  that breaks the "all live intents execute" invariant.
- **Anchor-tenant partner integration** (§3.3) — the one bucket-4 pattern that adds distinct funders, via the existing `PooledAction` seam; a
  business-development dependency, not a code gap.
- **RLN for the off-chain coordinator mempool only** (§1.5) — the sole surface where it is not strictly dominated, and even there HTTP throttling +
  the mandatory proof-to-submit closes the residual more cheaply.

### 5.4 What stays rejected (name these three traps to judges, with RLN)

Anonymity mining (§3.1, leaks by construction), operator-funded decoys (§3.2, `k_∞ = k/d` — the whale re-labeled, defeats the k-floor), and
cover/dummy traffic (§4.3, cost-model mismatch that recreates the whale). Each *looks* like it deepens the crowd or rewards good behavior and instead
is inert-or-adversarial in the §0.3 lens. RLN (§1) is the fourth, most literate-reviewer-tempting trap.

---

## 6. Honest limitations + what-to-cite-where

### 6.1 Preserved verification flags (load-bearing — do not promote to confident)

- **`WITHDRAWN`:** Cristodaro *et al.* 2025 (arXiv:2510.09433, the "34.7%" figure) — withdrawn 2025-11-18. **Not cited in this document; do not
  reintroduce its numbers anywhere.**
- **Superseded upward, honestly:** the survey's `[UNVERIFIED]` TORN-mining sub-percentage (§3.2) is now `[VERIFIED]` at **358/97.3k** via Tutela §7.2
  (§3.1 here). This is the only flag this doc *upgrades*; all others are preserved as-is.
- **`[UNVERIFIED — gap, not a finding]`:** no rigorous isolated wash-trading/self-participation measurement exists for Tornado's mining window (§3.1) —
  do not assert a percentage. Tornado 2021 TVL growth is `[UNVERIFIED — secondary aggregator]` and bull-market-confounded.
- **`[UNVERIFIED — time-evolving]`:** RLN's exact production variant (v1 vs messageId-v2); the 33%/67% slash split for the *current* Waku deployment;
  the Waku mainnet/Linea/RLNv2 timeline is `[UNVERIFIED — secondary/search-synthesis]` (§1.1).
- **Derived-by-us, not benchmarked:** the RLN 6-input ≈100–103k CU and base 3-input ≈87k CU verify figures are interpolations from Light Protocol's
  `groth16-solana` table (§1.3); the `round_executable_slot` <1,000 CU figure is unmeasured, pending a LiteSVM pass (§4.2).
- **`[UNVERIFIED-SECONDARY]`:** leader/validator grinding of `SlotHashes`-derived values (§4.2, corroborates a structurally-obvious primary fact only).
  **`[UNVERIFIED — reasoned by analogy]`:** the Solana relayer-priority-fee heuristic (§4.4) — advisory, not a build item.
- **`[secondary]`/`[UNVERIFIED — vendor-adjacent]`:** Solana staking APY ~6–7.5% (§2.4, illustrative opportunity-cost, not load-bearing);
  ORE/Privacy-Cash and Wasabi rebuild-liquidity characterizations (deep-dive 3) — not load-bearing to any verdict.

### 6.2 What to cite where (extends the survey's §4 map with this doc's new decisions)

| mirror-pool decision / residual | Cite | Where in the final design docs |
|---|---|---|
| **RLN considered and rejected** (dominated by the nullifier-PDA idiom on an ordered chain; orthogonal to whale self-fill) | barryWhiteHat 2019; Shamir 1979; PSE RLN docs (sunset); Waku 2025 (pre-mainnet); this doc §1.4 | Anti-Sybil future-work note — upgrade "future-work" → "considered/rejected" |
| **Bonded-membership-with-slashing rejected; no self-fill slashing condition** | Bissias *et al.* 2014 (Xim, cost-not-proof); Cosmos `x/staking` (unbond justifies a *provable* offense); this doc §2.3–2.4 | Threat-table "Sybil / set poisoning" row; incentive-module (phase-4) spec |
| **Bond, if ever built, keys to deposit `C`, never `recipient`** (design rule) | Béres 2021 / Tang 2021 (recipient-freshness heuristic); this doc §2.2 | `["member",pool,C_m]` seam note in the spec |
| **Quadratic/progressive pricing is circular here** | Lalley–Weyl 2018; Mazorra–Della Penna 2023 (reward-side, *not* entry-cost) | Incentive-module spec, "why linear only" |
| **`Pool.fee` uniformity** (generalise `stake_fee`; anti-Sybil tax **and** amount-fingerprint fix) | Xim (nominal tax); Kappos *et al.* 2018 / benthamsgaze (Zcash value-linkage) | Plan 6c spec; the `commit_intent` fee-check unification |
| **Anonymity mining / any claimable duration-or-count reward rejected** | Wu *et al.* 2022 (Tutela §6.5 theorem, §7.2 358/97.3k); Tornado governance proposal (designer-anticipated) | Incentive-module spec (silent accrual only) |
| **Operator-funded decoys rejected** (`k_∞ = k/d`; defeats the k-floor) | Mathewson–Dingledine 2004 (padding vs background-aware adversary); this doc §0.3, §3.2 | Cold-start / anchor-tenant note; name as a trap |
| **Harness must treat treasury as a clustered funder** | this doc §3.3 (fixture requirement) | Plan 6b harness API + adversary-model test suite |
| **Round scheduling raises attacker cost, does NOT make the attack uncertain** | Serjantov–Dingledine–Syverson 2002 (Threshold-and-Timed floor = k; pool mix ≠ this) | Plan 6c spec, "exact claim strength" section |
| **`SlotHashes` future-slot jitter; non-grindable-by-committer, leader-influenceable** | anza-xyz/solana-sdk + pinocchio (`MAX_ENTRIES=512`); solana-foundation/anchor (Sysvar restriction) | Plan 6c spec, entropy-source note |
| **Cover/dummy traffic rejected** (cost-model mismatch) | Mallesh–Wright 2007 (RB cover 100–300%); M&D 2004 (sender dummies insufficient) | Timing design, "considered and deprioritised" |
| **Public `intent_count` ⇒ uncertainty available, inexactness never** | Serjantov–Dingledine–Syverson 2002 §5.2 | Timing design, permanent-limits note |

### 6.3 Honest limitations

1. **This doc does not close the binding constraint.** Every mechanism that would deepen *real* (distinct-funder) k is either deferred (bonding),
   an anti-pattern (mining, decoys), a poor architectural fit (RLN), or a business-development dependency (partner). The on-file honest position stands:
   the k-floor buys k *candidates*; Plan 6b's `k_∞` makes the gap *visible and quantified*; closing it is future work, priced but not built.
2. **The buildable subset (§5) is nominal-cost + measurement + timing — not a crowd-depth fix.** `Pool.fee` is a tax and a fingerprint fix; the harness
   fixture measures; `round_executable_slot` addresses the *across-round timing* axis. Presenting any of them as raising distinct-human k would be an
   overclaim.
3. **`k_∞` is a host-side measurement, not an on-chain gate** — it needs a funder-clustering input the chain cannot produce (survey §5.1); on-chain
   distinct-funder counting is already assessed unenforceable. `meets_k_floor` stays a nominal-count liveness gate.
4. **Every CU figure here is derived or unmeasured** (§6.1); the timing slice's <1,000 CU and the RLN verify deltas both need a LiteSVM measurement pass
   before they are quoted as facts, in the mould of the existing 26,247 / 56,800 CU numbers.
5. **The timing verdict's claim strength is deliberately narrow** (§4.2): raises the cost/latency of an *exact* isolation attack; does not make it
   uncertain. The mechanism that would (a true pool mix) is out of scope and cited as future work. Do not let the design docs round this up.

---

## Appendix — citations (consolidated from the four deep-dives, flags preserved)

**RLN strand.**
1. Shamir (1979), "How to Share a Secret," *CACM* 22(11), pp. 612–613, DOI 10.1145/359168.359176 — `[VERIFIED bibliographically]`.
2. barryWhiteHat (2019-02-18), "Semaphore RLN…," ethresear.ch — `[VERIFIED, primary, fetched in full]`.
3. PSE RLN docs (`rate-limiting-nullifier.github.io/rln-docs/`) & project status (`pse.dev/en/projects/rln`, **"Inactive"/sunset**) — `[VERIFIED, primary, fetched]`.
4. Taheri-Boshrooyeh *et al.* (2022), "WAKU-RLN-RELAY," arXiv:2207.00117; Waku Monthly Update (pub. 2025-07-03, **still pre-mainnet**) — `[VERIFIED, primary]`; later mainnet/Linea/RLNv2 timeline — `[UNVERIFIED — secondary/search-synthesis]`.
5. Logos Research (2023-11-07), "Strengthening Anonymous DoS Prevention with RLN in Waku" — `[VERIFIED, primary]` (tree height 20; `nullifier = Poseidon(a₁)`; two-point reveal algebra).
6. Groth (2016), "On the Size of Pairing-based Non-interactive Arguments," EUROCRYPT 2016 — `[bibliographically well-established]` (Groth16 prover cost scales with circuit size).
7. Light Protocol `groth16-solana` CU benchmarks, github.com/Lightprotocol/groth16-solana — reused from in-repo `[VERIFIED]` figure; RLN CU deltas here are **derived-by-us interpolations, not benchmarked.**

**Bonding strand.**
8. Bissias, Ozisik, Levine & Liberatore (2014), "Sybil-Resistant Mixing for Bitcoin" (Xim), WPES 2014 — `[VERIFIED]` in-tree (cost-not-proof).
9. Mazorra & Della Penna (2023), "The Cost of Sybils…," arXiv:2301.12813 — `[VERIFIED via direct fetch]`; **scope-corrected**: reward-sharing (Prop. 2.3), *not* entry-cost bonds.
10. Lalley & Weyl (2018), "Quadratic Voting," AEA P&P 108, DOI 10.1257/pandp.20181002 — `[VERIFIED]`.
11. Buterin, Hitzig & Weyl (2019), "A Flexible Design for Funding Public Goods," *Mgmt Sci* 65(11), arXiv:1809.06421 — `[VERIFIED]` (reward-matching, not entry-cost).
12. Cosmos SDK `x/staking` README (21-day unbond preserves an equivocation-detection window) — `[VERIFIED, primary]`.
13. Béres *et al.* (2021), IEEE DAPPS, arXiv:2005.14051; Tang *et al.* (2021), CNCERT, DOI 10.1007/978-981-16-9229-1_3 — `[VERIFIED]` in-tree (recipient-freshness heuristic).
14. Solana staking APY ~6–7.5% (StakePoint; StakingRewards) — `[secondary, cross-consistent, illustrative only]`.

**Cold-start / anonymity-mining strand.**
15. Tornado Cash governance proposal (Medium) & `anonymity-mining.md` — `[VERIFIED]` (10%/1M TORN, 1-yr linear; explicit rejection of plain LM; two-stage shielded LM; 2020-12-18→2021-12-18).
16. Wu, McTighe, Wang, Seres, Bax *et al.* (2022), "Tutela," arXiv:2201.06811, §§6.5/7.2 — `[VERIFIED]` via ar5iv (PDF unparsable this session); **source of the 358/97.3k figure superseding the survey's `[UNVERIFIED]`.**
17. Kappos, Yousaf, Maller & Meiklejohn (2018), "An Empirical Analysis of Anonymity in Zcash," USENIX Security — `[VERIFIED]` in-tree (concentrated-seed cautionary case; value-linkage).
18. Monero ring-size history (getmonero.org; PR #8178) — `[VERIFIED]` in-tree (protocol-mandated uniformity).
19. zkSNACKs coordinator discontinuation (2024-06-01) — `[VERIFIED]` on the fact; rebuild-liquidity — `[UNVERIFIED — secondary]`.
20. ORE / Privacy Cash shielded-pool seeding — `[UNVERIFIED — vendor-adjacent secondary]`.
21. Tornado Cash TVL growth Apr–Jun 2021 (DefiPulse-sourced) — `[UNVERIFIED — secondary aggregator]`; wash-trading during the AM window — `[UNVERIFIED — gap]`.

**Timing strand.**
22. Serjantov, Dingledine & Syverson (2002), "From a Trickle to a Flood," IH 2002, LNCS 2578 — `[VERIFIED, read in full]` (Threshold-and-Timed floor = k; pool-mix uncertainty + 5–9× multiplier; uncertainty vs inexactness levers).
23. Mallesh & Wright (2007), "Countering Statistical Disclosure with Receiver-bound Cover Traffic," ESORICS 2007, LNCS 4734, DOI 10.1007/978-3-540-74835-9_36 — `[VERIFIED, read in full]` (RB cover 100–300% overhead; §8 unresolved).
24. Mathewson & Dingledine (2004), "Practical Traffic Analysis," PET 2004 — `[VERIFIED, read in full]` in-tree (sender dummies insufficient vs background-aware adversary).
25. `SlotHashes` layout / `MAX_ENTRIES=512` — anza-xyz/solana-sdk, anza-xyz/pinocchio — `[VERIFIED, primary]`; Anchor `Sysvar<SlotHashes>` restriction — solana-foundation/anchor docs — `[VERIFIED]`.
26. Leader/validator grinding of Solana slot hashes — `[UNVERIFIED-SECONDARY]`; relayer priority-fee as a Solana gas-price-heuristic analogue — `[UNVERIFIED — reasoned by analogy]`.

**Withdrawn — do NOT cite.**
27. Cristodaro, Kraner & Tessone (2025), arXiv:2510.09433 / 2510.09443 — **`WITHDRAWN` 2025-11-18**, source of the unsafe "34.7%"; excluded by rule.

**Code loci this synthesis is grounded in (read this session).**
`programs/pool-program/src/invariants.rs:6,12-18,32,53-67`; `round.rs:15-48`; `state.rs:15-59`; `lib.rs:134-193,259-449,633-641`; `nullifier.rs:1-9`; `action.rs:18-20,34-64,80-90,186-190`; `circuits/circom/withdraw.circom:1-31`; `docs/superpowers/specs/2026-07-15-mirror-pool-design.md:152,181,250`; `docs/superpowers/specs/2026-07-17-timeout-gated-cancel-design.md:75-101,175`; `docs/research/anonymity-frontier-and-antisybil.md` §§0–5.
