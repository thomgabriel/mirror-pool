# §6.5 Adversarial simulation — the empirical "it actually hides" proof (F2b)

**Date:** 2026-07-21 · **Status:** approved design, pending fork spec-review
**Grounding:** `docs/research/anonymity-frontier-and-antisybil.md` §1 (the three metrics +
the Tóth–Hornák–Vajda / Li et al. proofs that Shannon and nominal-k miss whale self-fill),
§2 (Danezis 2003 statistical-disclosure closed form), the roadmap's F2b guard
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

### R1 — Distinct funders (the mechanism working)
`k` notes, `k` distinct singleton funders. Feed the composition to
`effective_k::anonymity_report`; assert `effective_k ≈ k`, `guessing_advantage ≈ 0`,
`max_funder_share = 1/k`. This is the *baseline that must hold* — if R1 doesn't show near-nominal
anonymity, the metric is broken. (It won't be — Task-6b tests already pin `m=1 ⇒ k_∞=k`; here it's
the top of the contrast.)

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
- `t*`, the rounds-to-converge at confidence `l` (eq. 6),
- and a **direct simulation** that generates `t` synthetic rounds (Alice's `m` real destinations
  mixed with `b−1` background draws from `N`), runs the linear estimator `v ≈ b·Ō − (b−1)·u`, and
  reports the empirical rounds-to-identify vs. the closed-form `t*` (they should agree — that
  agreement is the harness validating its own model against the literature).
Report the two structural facts the research derives: variable round size `b ≥ k` is a *pool-mix*
favorable property (harder than fixed-b), and the **stake action's small `N` makes it the more
exposed action type** under `m < N/(b−1)` — a formula-derived, disclosed per-action asymmetry.

## 3. Output — the proof artifact

A committed `docs/ADVERSARIAL-SIM.md` (hand-framed, like SOAK.md) embedding a captured run, plus
the harness's structured report. Structure:
1. *What this measures* — the three regimes, each mapped to its research-doc section + verified
   citation.
2. *The R1→R2 contrast table* — effective-k, guessing-advantage, max-funder-share across the
   whale sweep; the number that collapses is shown, not buried.
3. *The R3 decay* — `t*` vs. empirical rounds-to-identify, the precondition, the per-action `N`
   asymmetry (withdraw large-N vs. stake small-N).
4. *What this does NOT establish* — at equal prominence: it's a model over synthetic compositions
   (real funder-clustering is an *input* the harness assumes, not something it proves the adversary
   can achieve); the closed forms carry their own assumptions (global passive observer, stable
   behavior); this measures the residuals the design already discloses, it does not add a mechanism
   that removes them.

## 4. Honesty ledger

- **Adversarial against ourselves** is the whole point (roadmap guard): R2 and R3 are the regimes
  where mirror-pool degrades; they get top billing, not a footnote.
- Every reported number reduces to a **verified** closed form in the research doc — `k_∞ = k/m`
  (Cachin/DRS), the Danezis `t*` (SEC 2003 eqs). No fabricated numbers; the simulation's job is to
  *reproduce* the closed form empirically, and any divergence is a finding, not smoothed over.
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
  research doc's worked numbers (the Tóth–Hornák–Vajda D1/D2 example — S=4.3219 bits, Θ=0.5 — is a
  ready-made oracle for the min-entropy-vs-Shannon divergence; Danezis eq. 6 against a hand-computed
  `(m,N,b,l)` case); a proptest that the whale sweep is monotone-collapsing; a proptest that the
  simulation's empirical convergence tracks `t*` within a tolerance. The harness binary/example
  produces the captured report.
- Likely 2 plan tasks: (1) the `disclosure` module (Danezis closed form + seeded simulation) +
  its unit/property tests; (2) the regime harness (R1/R2/R3 wired to a report) +
  `docs/ADVERSARIAL-SIM.md` + the captured run. R1/R2 need no new metric code — they're the
  existing `anonymity_report` swept — so Task 1 is the real content.

## 7. Process

`feat/adversarial-sim` → internal opus spec review → fork spec gate → plan → fork plan gate → SDD
build → opus whole-branch review (bar: every number reduces to a verified closed form; adversarial
regimes at equal prominence; no overclaim) → fork merge gate → local merge. No push without the
user's explicit yes.
