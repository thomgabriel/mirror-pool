# §6.5 Adversarial simulation — the empirical "it actually hides" proof (F2b)

**Date:** 2026-07-21 · **Status:** approved design, pending fork spec-review
**Grounding:** `docs/research/anonymity-frontier-and-antisybil.md` §1 (the three metrics — with
Smith 2009 FoSSaCS as the min-entropy definitional anchor, §6.1 — plus the Tóth–Hornák–Vajda /
Li et al. proofs that Shannon and nominal-k miss whale self-fill), §2 (Danezis 2003
statistical-disclosure closed form) and §2.3 (empirical public-chain clustering — grounds the
assumed adversary capability), the roadmap's F2b guard
(`docs/superpowers/plans/2026-07-18-finish-roadmap.md`); builds on `crates/effective-k`.
**Branch:** `feat/adversarial-sim` off `main` (`b34f515`, post-SOAK).

## 1. What this is — and the one thing that makes it credible

A host-side simulation harness (`crates/effective-k`, extended — or a sibling module) that runs
the deanonymization heuristics **that still bite** against mirror-pool's own model across three
adversarial regimes, and reports the **measured** anonymity outcome of each. Its output is the
empirical companion to the SOAK: the SOAK proves the *mechanism* runs live; this proves *how well
it hides*, and — the load-bearing property — **it is adversarial against ourselves.** It runs the
regimes where the mechanism DEGRADES and reports the degradation at equal prominence with the
regime where it works. A harness that shows only the happy path is worthless (and is exactly the
posture we've criticized in a competitor's proof doc); a harness that surfaces its own residuals
is the credible one.

**What it is NOT:** not a new anonymity claim, not an on-chain change, not a network/RPC
simulation. It is a deterministic, seedable, host-tested computation over synthetic round
compositions and participation traces, reducing to the verified closed forms in the research doc.

## 2. The three regimes (each a measured result, none omitted)

Given the research's own definitions (`§1.1`, `§2.1`), the harness computes and reports:

### R1 — Distinct funders (the mechanism working — the baseline, shown AFTER the degradation)
`k` notes, `k` distinct singleton funders. Feed the composition to
`effective_k::anonymity_report`; assert `effective_k == k` (exact, not approximate — the crate's
`no_whale_gives_nominal_k` test pins `== k`), `guessing_advantage == 0`, `max_funder_share =
1/k`. This is the *baseline that must hold* — if R1 doesn't show nominal anonymity, the metric is
broken. It is the bottom of the doc's contrast, not the top (F5): the reader meets R2/R3
degradation first.

### R2 — Whale self-fill (the composition residual, disclosed)
One funder owns `m` of the `k` notes, sweeping `m = 1 … k`. For each `m`, report the measured
`effective_k = k/m`, `guessing_advantage = (m−1)/k`, `max_funder_share = m/k`. The headline curve:
**effective-k collapses from k toward 1 as the whale grows** — the exact `k_∞ = k/m` shape the
research proves nominal-k and Shannon-k both miss (Tóth–Hornák–Vajda Thm 1; §1.3). The harness
asserts the collapse is monotone and that at `m=k` it bottoms at `effective_k = 1.0` — the honest
statement that a fully self-filled round has zero behavioral anonymity, computed, not assumed.

### R3 — Repeated participation (the cross-round decay, quantified)
A repeat participant ("Alice") directs value to a fixed set of `m` destinations from a universe
`N`, across rounds of size `b`. Implement Danezis 2003's closed form (research §2.1, eqs. 1–6):
- the precondition `m < N/(b−1)` (report whether the attack even applies),
- `t*`, the rounds-to-converge at confidence `l` (eq. 6), **reported only in the `m ≥ 2` regime**
  where it is a meaningful round count. **For `m = 1` (and any `t* < 1`), report "precondition
  holds ⇒ the attack applies from the first rounds" rather than a fractional-round `t*`** — a
  sub-1-round number is not an observable round count and would overstate attack ease (spec-review
  F2). The quantitative decay *curve* therefore lives in `m ≥ 2`; say so in the output.
- and a **seeded cross-check simulation** — NOT independent literature validation, but a
  self-consistency check that the *coded* estimator recovers Alice's set at the confidence the
  *coded* `t*` predicts (spec-review F3): generate `t` synthetic rounds (Alice's `m` real
  destinations + `b−1` background draws from `N`), run the linear estimator `v ≈ b·Ō − (b−1)·u`,
  and declare "identified" by the **same `l`-sigma separation criterion `t*` is derived from**
  (each real destination's estimate exceeds background by `l` standard deviations) — not a
  different top-m test, or the two systematically diverge. Because one seed's rounds-to-identify
  is a high-variance random variable, agreement is asserted **over a distribution of seeds** (the
  success rate at `t*` rounds ≈ the `l`-confidence, or mean rounds-to-identify over N seeds ≈ `t*`
  within a stated band), never as a single-seed "≈ t*".
Report the structural facts the research derives: variable round size `b ≥ k` is a *pool-mix*
favorable property (harder than fixed-b). And the **per-action asymmetry — stake is the more
exposed action type — is driven by the precondition regime + a plausibly small target set
(`m ≈ 1`, from the ~1-epoch stake-deactivation cooldown discouraging rotation) + a small
destination universe `N` (validators receiving pool delegations, hundreds not millions), NOT by
`t*` magnitude** (spec-review F1). `t*` is *decreasing* in `N`, so ranking action exposure by
`t*` would give the opposite, wrong headline — the harness must NOT rank exposure by `t*`. The
honest asymmetry is **behavioral, on the `m` side — forced-vs-permitted, NOT a claim about `N`**
(spec-review, fork gate; checked against eq. 4 `m < N/(b−1)`, whose right side *grows* with `N`,
so large `N` makes the precondition *easier* to satisfy for any fixed `m`, never harder):
- **Stake *forces* the vulnerable shape.** The ~1-epoch stake-deactivation cooldown pins `m ≈ 1`
  — a fixed, stable target — against a small, enumerable `N` (validators receiving pool
  delegations). The Alice model applies structurally, and the identified edge (funder → validator)
  is concrete. This is structural exposure.
- **Withdraw merely *permits* it.** A user who reuses a fixed small recipient set is fully
  attackable at large `N` too — the precondition is trivially satisfied there and convergence is
  *faster*. Withdraw's protection is that fresh-recipient rotation is available and costless, so
  the destination set grows with participation and voids the fixed-small-`m` model. That is
  **user-behavior-conditional safety, not structural safety** — the stronger, more honest
  disclosure. (§3.3 inherits this framing.)

## 3. Output — the proof artifact

A committed `docs/ADVERSARIAL-SIM.md` (hand-framed, like SOAK.md) embedding a captured run, plus
the harness's structured report. **The structured report (not just the prose) must surface the R3
decay result as a first-class field mirroring R2's collapse field** (spec-review F5 — equal
prominence is unenforceable in hand-written prose alone; the machine report is what makes it
real). Doc structure, degradation-first (F5):
1. *The degradation headlines, up front* — the R2 whale-collapse curve AND the R3 decay result
   lead the document, before the R1 baseline. A reader sees where the mechanism fails before where
   it works.
2. *The whale-sweep contrast table (R2 → R1)* — effective-k, guessing-advantage, max-funder-share
   across `m = k … 1`, i.e. **worst case first**; the collapsing number is the first column a
   reader meets, not a footnote.
3. *The R3 decay* — the precondition `m<N/(b−1)`, `t*` (m≥2 regime; "applies immediately" for
   m=1), the seeded cross-check's seed-distribution agreement, and the per-action asymmetry as the
   **forced-vs-permitted** framing above (stake *forces* `m≈1` against small `N` = structural
   exposure; withdraw *permits* it but rotation gives user-behavior-conditional safety) — NOT a
   `t*` ranking, and NOT any claim that large `N` shields the precondition (eq. 4: large `N` makes
   it easier to satisfy).
4. *What this measures, and what it does NOT establish* — at equal prominence: it's a model over
   synthetic compositions; **real funder-clustering is an assumed adversary *capability*, an input
   to the harness — grounded, not fantastical: public-chain clustering is empirically demonstrated
   (research §2.3, Béres/Tang/Tutela/Wang), so the assumption is assumed-because-demonstrated-
   elsewhere, not assumed-because-convenient** (spec-review F4). But the harness neither proves a
   *given* real deposit graph IS clusterable (no "your pool is broken" overclaim) NOR that it is
   NOT (no "synthetic, so real pools are safe" dismissal). The closed forms carry their own
   assumptions (global passive observer, stable behavior). This measures residuals the design
   already discloses; it adds no mechanism that removes them.

## 4. Honesty ledger

- **Adversarial against ourselves** is the whole point (roadmap guard): R2 and R3 are the regimes
  where mirror-pool degrades; they get top billing, not a footnote.
- Every reported number reduces to a **verified** closed form in the research doc: the min-entropy
  *definition* `k_∞ = 2^{H_∞} = 1/maxᵢpᵢ` is Cachin 1997 / Dodis–Reyzin–Smith 2007 / Smith 2009
  (FoSSaCS — the definitional anchor per research §6.1), but the **`k_∞ = k/m` whale
  specialization is derived-by-us, not a cited theorem** — keep that label, don't launder it into
  Cachin/DRS (spec-review F6). The Danezis `t*` is SEC 2003 eqs 1–6. No fabricated numbers; the
  seeded simulation *cross-checks the coded estimator against the coded `t*`* (a self-consistency
  check, not literature validation), and any divergence is a finding, not smoothed over.
- The `2^H`/`k_∞`/`Adv` naming keeps its **derived-by-us, not literature-named** labels (research
  §1.2) wherever it appears in the doc.
- No overclaim: the harness measures the anonymity of a *given* composition; it does not claim to
  prove real deposit-graphs are or aren't clusterable — that clustering is the adversary's
  assumed capability, stated as an assumption, per §2.2's "the adversary is free" framing.

## 5. Non-goals

- No on-chain change; no SDK change; no network/timing/RPC simulation (that's the SOAK's live
  tier — this is the analytic tier).
- No new anonymity *mechanism* (bonding/mixing stay deferred per the roadmap and the mechanism
  research); this is measurement, not defense.
- No ML/deep deanonymization — the research's own verdict is that the closed forms suffice and are
  more honest than an opaque model (§2.1). YAGNI.
- Not a Monte-Carlo with irreproducible randomness: seedable, deterministic, host-tested.

## 6. Build notes for the plan

- Reuse `effective_k::{RoundComposition, FunderId, anonymity_report, AnonymityReport,
  collapses_below}` for R1/R2 directly (the whale sweep is just repeated `anonymity_report` over
  compositions with a growing dominant funder). R3 (Danezis) is new pure code — a `disclosure`
  module: `precondition(m, n, b) -> bool`, `rounds_to_converge(m, n, b, l) -> f64` (eq. 6), and a
  seeded `simulate_disclosure(params, seed) -> DisclosureRun` returning empirical rounds-to-identify.
- Determinism without `Math.random`: a small seeded PRNG (e.g. a `SplitMix64`/`xorshift` in-crate,
  no dep — the crate is dep-free today and should stay so; proptest is already a dev-dep for tests).
- Tests (TDD, the crate's existing discipline): unit tests pinning the closed forms against the
  research doc's worked numbers. The Tóth–Hornák–Vajda D1/D2 example is a **ready-made, verified
  oracle** for the min-entropy-vs-Shannon divergence — spec-review confirmed D2 is representable as
  `k=200, whale=100, +100 singletons` → `effective_k=2, max_share=0.5, shannon_bits=4.3219`, which
  the crate's integer-count posteriors reproduce exactly. Also: Danezis eq. 6 against a
  hand-computed `(m,N,b,l)` case; a proptest that the whale sweep is monotone-collapsing (the crate
  already has `concentration_never_raises_effective_k`). The R3 simulation's agreement test asserts
  over a **seed distribution** (success-rate-at-`t*` ≈ `l`-confidence, or mean-rounds ≈ `t*` within
  a band), NOT a single-seed point equality (F3) — and is framed as a coded-estimator-vs-coded-`t*`
  self-consistency check (F7: this is the simulation's only non-circular value; the closed form +
  the hand-computed oracle carry the honest R3 message on their own, so the simulation earns its
  place solely as that bug-catching cross-check). The harness binary/example produces the captured
  report.
- Likely 2 plan tasks: (1) the `disclosure` module (Danezis closed form + seeded simulation) +
  its unit/property tests; (2) the regime harness (R1/R2/R3 wired to a report) +
  `docs/ADVERSARIAL-SIM.md` + the captured run. R1/R2 need no new metric code — they're the
  existing `anonymity_report` swept — so Task 1 is the real content.

## 7. Process

`feat/adversarial-sim` → internal opus spec review → fork spec gate → plan → fork plan gate → SDD
build → opus whole-branch review (bar: every number reduces to a verified closed form; adversarial
regimes at equal prominence; no overclaim) → fork merge gate → local merge. No push without the
user's explicit yes.
