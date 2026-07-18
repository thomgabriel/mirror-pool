---
title: "Anonymity frontier & anti-Sybil — measurement, repeated-participation attacks, and crowd-depth for mirror-pool"
date: 2026-07-17
status: research (informational — grounds the Plan 6b effective-k harness; drives the §2.4 citation corrections now being applied to the spec and sibling docs)
companion_to:
  - docs/superpowers/specs/2026-07-15-mirror-pool-design.md
  - docs/research/prior-art.md
  - docs/research/behavioral-privacy-industry-practices.md
  - docs/research/behavioral-rounds-followup-proposal.md
method: >-
  Three literature strands (anonymity METRICS · repeated-participation ATTACKS · ANTI-SYBIL / crowd-depth),
  each fact-checked with primary sources read in full where marked VERIFIED and flagged otherwise. Synthesised
  against a first-hand read of the merged tree (invariants.rs, lib.rs, the design spec).
scope: >-
  What effective-k means and which variant Plan 6b should compute; how anonymity decays under repeated
  participation and how to disclose it honestly; the anti-Sybil menu mapped to our round/PooledAction model
  with a YAGNI build-vs-cite recommendation; and an exact "what to cite where" table for the final design docs.
---

# Anonymity frontier & anti-Sybil for mirror-pool

> **Purpose.** mirror-pool's on-chain gate `meets_k_floor(intent_count, k_floor)`
> (`programs/pool-program/src/invariants.rs:6`) counts **raw intents**, not distinct
> funders. The project already discloses the consequence honestly — "the k-floor buys
> k *candidates*, not k-anonymity" (`docs/research/behavioral-rounds-followup-proposal.md`),
> and the spec's own threat table books *Sybil / set poisoning* as "**Residual:** not fully
> solved" (`docs/superpowers/specs/2026-07-15-mirror-pool-design.md:181`). This document
> supplies the literature to (a) **measure** that residual rigorously (Plan 6b), (b) name the
> **repeated-participation** attacks the single-round k-floor does nothing about, and (c) lay
> out the **anti-Sybil / crowd-depth** options with an honest build-vs-defer call.
>
> Every cited idea below is tethered to a concrete mirror-pool design decision or residual.
> Verification flags from the source research (`[VERIFIED]`, `[UNVERIFIED-PRIMARY]`,
> `WITHDRAWN`) are preserved verbatim — **do not launder any flagged claim into a confident
> one when this lands in the design docs.**
>
> **Grounding files read this session:** `programs/pool-program/src/invariants.rs`,
> `programs/pool-program/src/lib.rs`, `docs/superpowers/specs/2026-07-15-mirror-pool-design.md`,
> `docs/research/prior-art.md`, `docs/research/behavioral-privacy-industry-practices.md`,
> `docs/research/behavioral-rounds-followup-proposal.md`.

---

## 0. The through-line (read this first)

Three properties, three failure axes, three literature strands:

| mirror-pool property | Failure axis it does **not** cover | Strand that governs it |
|---|---|---|
| `k`-floor: a round fires only at ≥ k intents (`invariants.rs:6`) | **Composition** — one funder self-filling m of k notes ("whale self-fill"); nominal k overstates real anonymity | §1 METRICS → measure with min-entropy effective-k `k_∞` |
| Uniform actor: the vault signs every action, so no per-intent signer leaks *within* a round | **Across rounds** — a repeat participant directing value to the same small set of destinations is deanonymised by intersection/statistical-disclosure attacks on public chain data | §2 ATTACKS → quantify with the closed-form Danezis bound over the pool's own (m, N, b) |
| The spec reserves a `["member",pool,C_m]` PDA for bonding | **Crowd depth / Sybil** — nothing is built at that seam yet; the whale can register k−1 fresh identities | §3 ANTI-SYBIL → price a linear bond *when* built; measure first (Plan 6b), don't over-build now |

The single sentence that orders the whole doc: **counting is not hiding.** The k-floor is a
correct, cheap *liveness* gate (never fire a thin round) — it is not, and was never claimed to
be, a measurement of realised anonymity. §1 gives the measurement; §2 gives the decay-over-time
that no single-round metric can see; §3 gives the (deferred) mechanisms that would deepen the
real crowd rather than just the nominal count.

---

## 1. How we measure anonymity — effective-k, min-entropy, and guessing advantage

### 1.1 The setup, in mirror-pool's own terms

Take one executed round of `k` pooled actions (the k intents that cleared `meets_k_floor`).
Let `𝒰` assign to each action the adversary's posterior probability that a **given real-world
funding entity** is the initiator. This is exactly the Serjantov–Danezis / Díaz *et al.*
anonymity-probability-distribution formalism, with "role = initiator of this action" as the
hidden attribute:

- **Honest case** (k distinct singleton funders): `𝒰` is uniform, `pᵢ = 1/k`.
- **Whale self-fill** (one funder controls `m` of the `k` notes, `1 ≤ m ≤ k`): *if* the adversary
  can cluster those m notes to one funder — realistic, and precisely the deposit-graph/timing
  residual §2 is about — the true posterior is `p = m/k` for "the whale did this action" plus
  `(k − m)` singleton masses of `1/k`.

This is structurally an **equivalence class of size k dominated by one value of relative mass
m/k** — the exact shape of Machanavajjhala *et al.*'s homogeneity attack and Li–Li–Venkatasubramanian's
49-positive/1-negative *skewness* attack, with "sensitive attribute value" replaced by "true
funding identity." (Sweeney 2002; Machanavajjhala *et al.* 2007; Li *et al.* 2007 — all `[VERIFIED]`,
full primary text.) The lineage lesson is uniform across all three papers: **k-anonymity counts
group *size*; entropy-ℓ-diversity measures *average* dispersion; neither bounds the *maximum*
probability mass one entity can hold inside a nominally-qualifying group** — which is exactly the
seam whale self-fill exploits.

### 1.2 Three metrics, precisely defined and citable

**(a) Nominal k** — pure count, what the chain enforces today.
`meets_k_floor(intent_count, k_floor)` (`invariants.rs:6`). Information-theoretically this is the
**Hartley entropy** `H₀(X) = log₂|𝒳|` exponentiated — the *loosest*, most optimistic member of
the Rényi family. (Cachin 1997, Prop. 2.4, `[VERIFIED]`.) It is the right tool for its job
(liveness: never fire a thin round) and the wrong tool for measuring realised anonymity.

**(b) Shannon effective-k** — descriptive / monitoring only, **not** a safety gate:

```
k_H := 2^{H(X)},   where  H(X) = −Σᵢ pᵢ log₂ pᵢ
```

Serjantov & Danezis's "effective size" `S` *is* `H(X)` in bits (their Def. 2, `[VERIFIED]`);
Díaz *et al.*'s degree of anonymity is `d = H(X)/log₂k` (normalised, `[VERIFIED]`). The
exponentiation back to an equivalent count of equiprobable participants (`2^H`) is a standard
source-coding move but is **derived-by-us, not a literature-named term** — no verified 2002–2007
privacy paper (Serjantov–Danezis, Díaz *et al.*, Tóth–Hornák–Vajda, Edman *et al.*) names `2^{H(X)}`
"effective anonymity set size." **Cite Cachin 1997 §2.3 for the source-coding justification, not as
an "effective-k" precedent.** Do not present `k_H` as if a paper blessed the name.

**(c) Min-entropy effective-k** — the one to gate/report on:

```
k_∞ := 2^{H_∞(X)} = 1 / maxᵢ pᵢ           (H_∞ = min-entropy, a.k.a. H_inf)

whale self-fill  ⇒  k_∞ = k / m
```

Sanity checks it must pass, and does: `m = 1` (no whale) ⇒ `k_∞ = k` (matches nominal k exactly);
`m = k` (one funder fills the whole round) ⇒ `k_∞ = 1` (correctly flags total anonymity failure).
Grounded in Cachin 1997 (Prop. 2.3/2.4, `H_∞ = −log maxₓ P(x)`) and Dodis–Reyzin–Smith 2007 §2.1,
both `[VERIFIED]` full primary text. The load-bearing fact from Dodis–Reyzin–Smith, verbatim: the
**predictability** of `A` is `maxₐ P[A=a] = 2^{−H_∞(A)}`, and predictability *is*, by construction,
the success probability of the optimal single-guess adversary. This is a definition, not a bound.

**(d) Guessing advantage over the 1/k baseline** — the residual-anonymity headline number:

```
Adv_guess(m, k) := maxᵢ pᵢ − 1/k = (m − 1)/k          (additive, over the nominal-k baseline)
multiplicative form:  maxᵢ pᵢ / (1/k) = m             (the whale is m× likelier to be pinned than nominal k implies)
```

This packaging (predictability minus the uniform baseline, in the style of a cryptographic
distinguishing advantage `Adv = |Pr[real] − Pr[ideal]|`) is **derived-by-us from Dodis–Reyzin–Smith's
predictability definition, not a named literature term** — flag it as such in the design docs.

### 1.3 Why min-entropy, not Shannon or nominal k — and why this is proved, not asserted

The three metrics sit in a strict hierarchy on the **same** distribution (Cachin 1997, Prop. 2.4):

```
nominal-k (H₀)  ≥  Shannon effective-k k_H (H₁)  ≥  min-entropy effective-k k_∞ (H_∞)
   loosest / most optimistic                              strictest / most conservative
```

So nominal k and Shannon-k can both look arbitrarily healthy while `k_∞` is at the floor. This is
not a heuristic worry — it is **formally proved inside the anonymity literature**, with a worked
example isomorphic to whale self-fill:

- **Tóth–Hornák–Vajda 2004** (`[VERIFIED]`, full primary text) construct two distributions with
  **identical Shannon entropy `S = 4.3219` bits** — D1 uniform over 20 users (5% guess success),
  D2 with the true sender at `p = 0.5` and 100 decoys sharing the rest (**50% guess success**).
  Their **Theorem 1**: for source-hiding parameter `Θ = maxᵢ pᵢ` (`= 2^{−H_∞} = 1/k_∞`),
  `S ≥ −log₂ Θ`. This is **one-directional**: a high `Θ` (easy to guess) is fully compatible with
  a high `S` (looks good under Shannon). They show you can push `S` arbitrarily large while pinning
  the dominant mass at 0.5. **A batch can have large nominal k *and* large Shannon `k_H` *and still*
  have `Θ = m/k` dangerously high.** This is the exact, proved reason Shannon-based effective-k
  cannot catch whale self-fill — and nominal k, which ignores probabilities entirely, is weaker still.
- **Li–Li–Venkatasubramanian 2007** (`[VERIFIED]`) confirm the same failure from the k-anonymity
  side: their 49-positive/1-negative class has **higher entropy than the population baseline** yet
  is 98% inferable. Higher average dispersion, near-certain single-shot inference — the structural
  twin of a round that passes the k-floor while one funder owns most of it.
- **Massey 1994** (guessing entropy, `G(X) ≥ 2^{H(X)−2}+1`; inequality `[VERIFIED]` via Rioul 2022,
  page number `[UNVERIFIED-PRIMARY]`) is the one plausible Shannon-side counter-argument, and it
  answers a **different question** — the *expected number of repeated* guesses — not single-shot
  success. mirror-pool's whale-self-fill concern is single-shot ("is *this* action the whale's?"),
  so Massey doesn't rescue Shannon entropy here either.

### 1.4 What Plan 6b should implement, concretely

**Compute and report `k_∞ = 1 / maxᵢ pᵢ` (equivalently, flag when `Adv_guess = (m−1)/k` exceeds a
configured threshold) from a funder-clustering signal over the k notes in a round — not from k
alone.** Use `k_H` only as a secondary descriptive/trend statistic (it is the analogue of
entropy-ℓ-diversity being *one* of Machanavajjhala *et al.*'s three lenses), never as the enforced
bound, per §1.3.

Two honest constraints on where this metric can live, tying §1 to §3's YAGNI call:

1. **It is a host-side measurement, not (yet) an on-chain gate.** `k_∞` needs a funder-clustering
   input (deposit-graph/timing), which the on-chain program cannot see — and on-chain
   distinct-funder counting has already been assessed as unenforceable
   (`behavioral-rounds-followup-proposal.md`, adversarial-critic review). So Plan 6b is
   a **pure, host-testable invariant fn** in the `invariants.rs` mould (the same doctrine that gives
   us `meets_k_floor`, `split_payout`, `cancel_unlock_slot`) that *measures and reports* the
   whale-self-fill collapse from a clustering signal. The on-chain `meets_k_floor` stays exactly as
   it is — a liveness gate. This is "measurement before mechanism" (§3.6), the cheapest move that
   raises the quality bar without adding custody surface.
2. **If a softer relative constraint is ever wanted** instead of the hard `m/k` ratio,
   Machanavajjhala *et al.*'s **recursive (c,ℓ)-diversity** — bound the most-frequent value's share
   relative to the tail — is the closest existing template. Cite it; don't build it until a second
   caller needs it (YAGNI).

---

## 2. Residual attacks over repeated participation

The k-floor is a **single-round** invariant. The attacks in this section operate **across many
rounds** on public chain data and are, by construction, invisible to it. This is the axis
`prior-art.md` already calls the dominant one ("timing correlation is the *dominant* attack on any
pool") — this section makes it rigorous and, in the process, **corrects two load-bearing citations
that do not check out** (§2.4).

### 2.1 Statistical-disclosure / intersection attacks — the mechanism, with the closed form

The disclosure-attack lineage (Kesdogan–Agrawal–Penz 2002; Agrawal–Kesdogan 2003) models a global
passive observer watching a repeat sender ("Alice") who directs traffic to a fixed small set of `m`
destinations, drawn from a universe of `N`, through batches of size `b`. **Danezis 2003**
("Statistical Disclosure Attacks," SEC 2003 — primary source read in full) replaces the original
NP-complete search with a cheap linear-algebra estimator. Over `t` observed rounds, by the law of
large numbers:

```
Ō ≈ (v + (b−1)u)/b     ⇒     v ≈ b·Ō − (b−1)·u          (Danezis 2003, eq. 1–2)
```

with `v` = Alice's destination vector, `u` = background. Two numbers matter for us:

- **Precondition (signal > noise):**  `m < N/(b−1)`  (eq. 4).
- **Rounds to converge, at confidence `l` (l=2→95%, l=3→99%):**

  ```
  t > [ m·l·( √((m−1)/m²) + √((N−1)/(N²·(b−1))) ) ]²        (Danezis 2003, eq. 6)
  ```

This is the honest answer to "how fast does anonymity decay with repeated participation": **not
exponentially, not needing ML — a cheap closed form in (m, N, b), small when m and N are small.**
Modern least-squares variants confirm the rate: estimation error falls as **O(1/ρ)** in the number
of observed rounds and scales with **1/fᵢ**, the inverse of how often the target participates
(Oya–Troncoso–Pérez-González 2019, primary read in full) — the rigorous version of "participate
rarely and irregularly." Mathewson–Dingledine 2004 (primary read in full) extend this to pool
mixes, unknown backgrounds and padding, and frame the whole field's posture in one quote we should
adopt: *"we should attempt to quantify the risk: how long our designs can defend which senders
against an adversary who sees how much."*

### 2.2 Mapping onto mirror-pool — and the sharpest new finding

- **`b` ↔ round size, and it's a *favorable* structural fact.** The spec forms a round at
  `|valid| ≥ k` (`2026-07-15-mirror-pool-design.md:156`) — rounds are **≥ k, variable, not fixed at
  k**. That is exactly Mathewson–Dingledine's **pool-mix** case, which their simulations show is
  *harder* to attack than a fixed-b threshold mix, because variable batch size "spreads" each
  intent's true round-membership across more outputs. **This is a real disclosure-resistance
  property we already have and should state as such in the spec — today it's only framed as a
  liveness property.**
- **`N` ↔ destination universe, and it is action-type dependent — the new finding.** For the
  **withdraw** `PooledAction`, `N` = distinct external addresses across rounds (large once the pool
  matures). For the **stake-delegation** `PooledAction`, `N` = validators that actually receive
  pool delegations — structurally *small* (hundreds, not millions), and Solana's ~1-epoch stake
  deactivation cooldown discourages target rotation, so a user's own target set `m` is very
  plausibly 1. Under Danezis's own precondition `m < N/(b−1)`, **stake delegation is the
  structurally more exposed action type** to this attack family. This is a formula-derived claim,
  not a borrowed percentage — and it is exactly the kind of per-action exposure Plan 6b's harness
  should output.
- **The adversary is free.** The disclosure literature's threat is a *passive observer of the chain
  itself* — on a public ledger, anyone, for free. The spec's "coordinator sees only {proof,
  nullifier, ciphertext, timing}" guarantee (line 165) is about the **coordinator**, which is
  correctly scoped liveness-only (line 113) — but **trusting the coordinator does nothing against
  this attacker.** Only round-scheduling and user behavior do. State the passive on-chain observer
  (not coordinator compromise) as the primary adversary for this whole row of the threat table.
- **Repeated participation = literally Alice.** A user who commits many intents over many rounds
  (fresh nullifier each time, so on-chain values don't self-link) but keeps directing value to the
  same small `m` destinations is precisely the Danezis/Kesdogan Alice — and the design does nothing
  on-chain to prevent it (recipient/relayer are bound *into* the proof for integrity, not hidden
  from chain observers once the action executes).

### 2.3 Empirical Tornado Cash deanonymization — the honest evidence

Every non-withdrawn source below operates on **public chain data only** — no coordinator/relayer
compromise required — and the single most reproducible signal across all of them is **short elapsed
time between an intent's "in" and "out" events**, not amount (Tornado, like us, fixes denomination)
and not a cryptographic break. That independently corroborates the *direction* of our existing
prior-art conclusion even after the specific "34.7%" figure is pulled (§2.4).

| Source | Verified finding | Confidence |
|---|---|---|
| Béres *et al.* 2021, "Blockchain is Watching You," IEEE DAPPS, arXiv:2005.14051 | 218/110/60/7 withdrawals linked in the 0.1/1/10/100 ETH pools (data to 2020-04, TC nascent) | Primary, read in full; early/small-sample |
| Tang *et al.* 2021, "Analysis of Address Linkability in Tornado Cash," CNCERT, DOI 10.1007/978-981-16-9229-1_3 | H1 alone (deposit→withdraw with δ ≤ 180s) clustered 1,168 entities / 2,734 addresses — larger than H2+H3 combined | Primary, read in full |
| Wu *et al.* 2022, "Tutela," arXiv:2201.06811 | 42.8k of 97.3k equal-value deposits potentially compromised; **anonymity set reduced ~37% (±15%) on average** | Primary, read in full |
| Wang *et al.* 2023, "On How ZK Mixers Improve, and Worsen User Privacy," WWW '23, arXiv:2201.09035, DOI 10.1145/3543507.3583217 | Heuristics 1–5 reduce the set **27.34% (ETH) / 46.02% (BSC)**; correct-linkage probability **rises 37.63% (ETH) / 85.26% (BSC)** (their Eq. 15) | Primary, read in full |
| "Attacking Anonymity Set … via Wallet Fingerprints," ACM SAC '25, DOI 10.1145/3672608.3707896 | ~20% (13,203/66,248) txns claimed linkable via wallet gas-suggestion fingerprints | **`[UNVERIFIED]` — secondary summary only, paywalled** |
| Cristodaro–Kraner–Tessone 2025, arXiv:2510.09433 / 2510.09443 | Claimed 5.1–12.6% (+FIFO → 34.7%) | **`WITHDRAWN` 2025-11-18, "mistakes in the references." Do not cite the numbers as stable.** |

**Chainalysis, for the record:** their published, attributable figures are "$7.6B mixed" and "almost
30% of funds tied to illicit actors" — the latter is a **source-of-funds taint** metric (how dirty
the money going in is), a *completely different measurement* from "% of transactions deanonymised."
Conflating the two would be the exact error this section catches. **No Chainalysis-published
clustering/deanonymization-accuracy percentage exists** in their public material; don't cite one.

### 2.4 Two citations in our own docs do not check out — correct them (do not quote as-is)

Surfaced while verifying figures already in the tree — the corrections, since applied to the spec
and sibling docs:

1. **"FIFO 34.7%" is sourced to a withdrawn preprint.** It appears at
   `docs/superpowers/specs/2026-07-15-mirror-pool-design.md:179` (threat-table row "Timing
   correlation (FIFO 34.7%)"), `docs/research/prior-art.md:120-129`, and
   `docs/research/behavioral-rounds-followup-proposal.md:58`. The source is
   **Cristodaro–Kraner–Tessone, arXiv:2510.09433**, **withdrawn 2025-11-18** ("mistakes in the
   references"; no corrected version as of this research; its companion arXiv:2510.09443 was pulled
   the same day for the same reason). **`[UNVERIFIED — do not cite 34.7% as a stable number.]`** The
   *directional* conclusion (timing/FIFO correlation is a powerful attack class) survives on the
   non-withdrawn sources in §2.3; only the specific number is unsafe. **Fix:** replace with either a
   hedge pending a corrected version, or the verified Wang *et al.* figures below, caveated as
   Ethereum-address-reuse-specific.
2. **The Wang *et al.* figures are miscited.** `docs/research/behavioral-privacy-industry-practices.md:321`
   states "51.94% (ETH) / 108.63% (BSC)." The primary source (WWW '23, Eq. 15) says the correct-linkage
   probability **rises 37.63% (ETH) / 85.26% (BSC)** on average. **`[CONFIRMED MISCITATION — correct
   the doc.]`**

Both errors point the *same* direction — they overstate, not understate, how settled the "timing is
dominant" numbers are — so the qualitative thesis is intact. But neither number should reach a
bounty judge as-is. (Methodological note worth carrying: the "34.7%" and a phantom "Chainalysis
(2025) 93% drop" framing both trace to search-engine answer-synthesis inventing specific-sounding
attributions that don't survive checking the cited source directly — the same failure mode that
produced the two doc miscitations. Verify against primary text before quoting a percentage.)

### 2.5 How to disclose and quantify — for OUR multi-round design

- **State the passive on-chain observer as the primary adversary** for the timing/intersection row,
  not coordinator compromise (§2.2). Stronger and more honest than the spec's current framing.
- **Quantify with the closed-form Danezis bound over the pool's own observed (m, N, b)** — not a
  borrowed Tornado percentage. Tornado's numbers are ETH withdrawal-address reuse; they don't share
  our action space (Solana stake delegation, or an unknown-shape SDK action). This is precisely what
  Plan 6b should emit: a **per-pool, per-action-type `t`-to-break estimate**, not an imported
  industry number. The stake-delegation exposure (§2.2) drops straight out of this.

### 2.6 Mitigations, ranked for a cost-bounded, host-testable Solana round engine

**Tier 1 — cheap, primary-source-backed, do these:**

1. **Jitter the commit→eligible-for-round gap per intent.** Directly targets the #1 real-world
   heuristic (short Δt) across every §2.3 source. Implementable as a pure fn
   (`eligible_slot(commit_slot, jitter) -> Slot`) in the exact mould of the current branch's
   `cancel_unlock_slot` / `TIMEOUT_SLOTS` slot arithmetic (`invariants.rs:59-67`) — host-testable,
   fail-closed on overflow, no new custody surface. Backing: Mathewson–Dingledine 2004 (delay
   variability is the most effective single slower of the attack).
2. **Keep and *document* variable (≥k) round sizing as an anonymity property** (§2.2). Already
   built; zero marginal cost; M&D 2004 is direct evidence it measurably helps. Today the spec only
   frames it as liveness.
3. **Fixed denomination per pool.** Already built (`deposit`'s `amount == denomination`). Forecloses
   amount-fingerprinting outright — the reason no §2.3 attack needed an amount heuristic.
4. **SDK/client guidance against destination reuse** (same withdrawal address / same validator
   across many rounds). Cheap (UX only), directly shrinks `m` in the Danezis formula, targets the #1
   heuristic (address reuse: Béres H1, Tutela's 18.6k address-match reveals, Tang H1). **Must be
   disclosed as advisory-only** — it cannot be enforced on-chain without overriding the user's right
   to choose a destination.

**Tier 2 — moderate cost, real value:**

5. **Measure per-action validator/destination concentration for the stake action specifically**
   (§2.2 makes it the weakest action type). Natural output of the Plan 6b harness.
6. **Attach the k-floor's SNR role explicitly to its existing design decision.** Danezis's `m <
   N/(b−1)` means a larger, well-enforced k-floor directly strengthens resistance to *this* attack,
   independent of the whale-self-fill axis. No new code — a second, correctly-sourced justification
   for the k-floor.

**Tier 3 — the textbook answer, deliberately ranked low, and why:**

7. **Cover / dummy traffic.** The standard mixnet answer (Berthold–Langos 2002; Díaz–Preneel 2004;
   Loopix 2017 — last `[UNVERIFIED]`, search-summary only). It transfers poorly here for two
   literature-grounded reasons: **(a)** in a custodial, fixed-denomination pool a decoy intent must
   be a *real, fully-funded, indistinguishable* action, so someone (the treasury) locks real capital
   to pad a thin round — an ongoing cost, not a rounding error; **(b)** M&D 2004's own simulations
   show padding *"regardless of whether she uses padding"* fails against an adversary who **knows the
   background distribution** — and on a public chain the background is *always* fully known. Disclose
   cover traffic as **considered-and-deprioritised with this reasoning**, not silently omitted — a
   reviewer who knows the literature will ask.

---

## 3. Anti-Sybil / crowd-depth menu — mapped to our model, with a YAGNI call

**Grounding:** there is **no `join`/`member`/bond instruction anywhere in
`programs/pool-program/src/`** (grepped: zero matches), despite the spec's aspirational data-flow
step `① JOIN bond X → ["member",pool,C_m]` (`2026-07-15-mirror-pool-design.md:152`) and its threat
row booking Sybil as "**Residual:** not fully solved" (line 181). Everything below is addressed to
that exact gap. For each mechanism: does it **deepen real k** (distinct humans / inflation-proof) or
only **resist nominal inflation** — and what does it cost us.

### 3.1 Rate-Limiting Nullifiers (RLN) — the sharpest "sounds good, doesn't fit" case

**Mechanism.** Semaphore-lineage: `identityCommitment = Poseidon(a₀)`, member stakes into a Merkle
leaf; each per-epoch signal reveals a point on a degree-1 polynomial; a *second* signal in the same
epoch yields a second point on the same line → anyone Lagrange-interpolates and recovers the secret,
enabling a slash (33% to the discoverer, 67% burned). Origin: barryWhiteHat, ethresear.ch,
2019-02-18. **Maturity (checked live):** PSE's standalone RLN project is **sunset / "Inactive"**;
the construction lives on under Vac/Logos, with Waku RLN at **testnet hardening as of Sept 2025**,
not mainnet. **No Solana/Anchor port exists** — adopting it means originating a new circuit.

**Map to mirror-pool, and the verdict.** RLN's whole value is *retroactive, economic* punishment for
a rate violation the system **cannot observe directly** — the right tool for a gossip mempool (Waku)
with no shared ledger. mirror-pool is the opposite: `commit_intent` is an **on-chain, ordered**
instruction. Once a `member_commitment` exists on-chain, "one signal per member per round" is a
**one-line Anchor `init`-constrained PDA** — `["round_signal", pool, round_id, member_commitment]` —
using the *exact idiom the codebase already uses for nullifiers* (CLAUDE.md: "nullifier PDA existing
== spent; init fails atomically on a double-spend"). That is **preventive and atomic** (fails
closed); RLN's Shamir-reveal is **punitive and after-the-fact**. For a chain with cheap ordered
existence checks, the plain PDA **strictly dominates** RLN on cost, complexity, and fail-closed
posture. **Deepens real k? Neither** — RLN caps *replay of the same identity*, but our per-note
nullifier already stops the same *note* twice, and the team's actual residual — a whale registering
**k−1 distinct fresh** `member_commitment`s — is **untouched by RLN** (it never distinguishes many
self-created identities from many genuine ones). **Cite as future-work-only, for a different surface
(rate-limiting the off-chain coordinator mempool before a proof hits the chain).** Name it explicitly
to judges precisely because it *looks* like the obvious fit and isn't.

### 3.2 Anonymity mining — the "inflates the success metric while shrinking real anonymity" case

**Mechanism & why it backfired.** Tornado's anonymity mining (ran 2020-12→2021-12) paid a **public,
fixed AP-per-block** over a deposit's lifetime. Because the rate is public, an observer **inverts the
claimed AP into the exact number of blocks the deposit sat in the pool** — a high-precision timing
side-channel. This is one of Tutela's five heuristics (Wu *et al.* 2022: 42.8k/97.3k deposits
compromised, ~37% aggregate set reduction). **`[UNVERIFIED]`**: the *TORN-mining-isolated*
sub-percentage could not be extracted from the primary PDF — the 42.8k/97.3k *aggregate* is
confirmed.

**Verdict — the cleanest "sounds good, backfires" in this survey.** Anonymity mining **inflates
nominal TVL/deposit-count while actively shrinking real anonymity** for anyone who claims carelessly.
This generalises to a **theorem, not an implementation bug**: *any* public, per-participant,
duration/count-keyed reward is an information leak by construction — the claimed amount is a
deterministic function of exactly the secret (deposit time) the pool exists to hide. It directly
confirms the "pay silently" conclusion already on file (`behavioral-privacy-industry-practices.md`,
Penumbra-style appreciating accrual). **Relevant to mirror-pool's deferred incentive module (spec
§3.4/§7): if built, silent/appreciating accrual only — never a claimable per-membership reward.**

### 3.3 Bonding / staking with slashing — real, but a price, not a proof

Xim (Bissias *et al.*, WPES 2014): attacker cost scales **linearly** with the fraction of the set
they occupy while honest cost stays flat. Formal backing: Mazorra–Della Penna 2023 (arXiv:2301.12813)
unifies the identity-cost model. **Verdict: deepens real k proportional to bond size — but only as a
price.** A well-capitalised adversary is unaffected; this is resistance, not a hard k-of-humans
guarantee. It is the mechanism the spec's reserved `["member",pool,C_m]` seam is for; it matches the
team's existing #3 proposal and its self-flagged limits (bond-vs-fee conflation; on-chain
distinct-funder self-attestation is forgeable).

### 3.4 Proof-of-personhood — the right variable, empirically defeated by rewards

| System | Trust assumption | Documented cost / failure |
|---|---|---|
| **World ID / Worldcoin** | Centralised biometric issuer at enrollment (Orb iris-scan) | Architecturally reintroduces the trusted-issuer pattern our design exists to avoid — independent of a now-**adjudicated** regulatory record (Kenya court-ordered biometric deletion, consent ruled invalid as "influenced by the offer of free tokens"; Hong Kong PCPD ordered a full stop as "unnecessary and excessive") |
| **BrightID** | Decentralised social graph (GroupSybilRank) | Probabilistic, not absolute — documented weakness that "sophisticated small-scale Sybil rings blend into the social graph" |
| **Idena** | Synchronous global validation ceremony | **Empirically degraded to "puppeteering":** by May 2022, **23 entities (<0.6% of identities) controlled ~40% of accounts and ~48% of rewards** — humans renting out their proven-unique slot (Ohlhaver–Nikulin–Berman, Stanford JBLP Vol 8 No 1, Jan 2025, co-authored by Idena's founder). Independent EPFL semester project (Subirà-Nieto 2021, `[not peer-reviewed]`) reached the same conclusion. |

**Cross-cutting finding.** Proof-of-personhood targets the *theoretically correct* variable (distinct
humans, not addresses) — but Idena, a real multi-year deployment, collapsed toward capital-weighted
control **the moment a claimable reward existed**, because nothing stops a human handing their
key to a puppeteer. This is a structural critique of the *category*, and it reinforces §3.2's "pay
silently" conclusion from a different angle: **any legible, claimable reward attached to "membership"
creates a rentable slot, and proof-of-personhood does not survive contact with one.** World ID
additionally fails a first-principles fit test (centralised issuer) independent of its regulatory
record.

### 3.5 Cold-start / bootstrapping — four patterns, ranked by fit

1. **Protocol-mandated uniformity (Monero-style) — strongest, cheapest, *already adopted*.** Monero
   made ring size mandatory and uniform (16 since v0.18, Aug 2022) precisely because *variable* ring
   sizes were themselves a fingerprint — the same lesson as our "standardized action shapes." Refusing
   to let a thin/non-uniform action execute *is* mirror-pool's on-chain k-floor. **This validates the
   k-floor choice; it doesn't just inspire it.**
2. **Anchor-tenant seeding (ORE/Wasabi)** — buys *liveness*, not anonymity (the in-house conclusion),
   **and only if the seed is behaviorally indistinguishable from a real participant** (this strand's
   caveat).
3. **Permissionless multi-coordinator posture** — the spec's §3.2 permissionless-coordinator
   principle is the *direct fix* for the single-point-of-failure that ended the two most successful
   real CoinJoin coordinators (zkSNACKs discontinued 2024-06-01 following the Samourai arrests). Not
   decentralisation-for-its-own-sake — insurance against a specific, documented failure mode.
4. **Do not rely on organic growth under thin/concentrated conditions.** Zcash's protocol-forced
   seeding is the cautionary twin of our whale-self-fill residual: of 2.24M transactions only 6,934
   were fully shielded, and **founders + miners were 65.6% of value drawn from the shielded pool**,
   "significantly eroding the anonymity of other users" (Kappos *et al.*, USENIX Security 2018). **A
   seed that becomes a structurally distinctive, disproportionate share of the pool recreates this
   instead of solving cold-start.**

### 3.6 YAGNI recommendation — build nothing new right now

Matches CLAUDE.md ("no abstraction until a second concrete caller"), project memory ("bonding/join
deferred"), and the spec's own phase order (bonding is phase 4, after `PooledAction` adapters).

1. **Keep the spec's existing disclosure as-is** — "Bond cost per membership… *Residual:* not fully
   solved" is already more honest than what Zcash, Tornado, or Idena shipped with at launch (none
   disclosed their equivalent gap up front; all three paid for the silence empirically, §§3.2/3.4/3.5).
   **Do not soften this language when Plan 6b lands.**
2. **Plan 6b (the effective-k harness) is the correct minimum next artifact.** It *measures* the
   whale-self-fill collapse (`meets_k_floor` counts nominal intents only) and reports it, without yet
   attempting to fix it. **Measurement before mechanism** is the cheapest move that raises the quality
   bar without adding custody surface.
3. **When (not now) a bonding/join module is built,** price it as a **linear-scaling bond**
   (Xim / Cost-of-Sybils, §3.3) at the reserved `["member",pool,C_m]` seam. **Explicitly exclude and
   cite as future-work-only, with reasons:** RLN (§3.1, on-chain ordering makes a plain uniqueness-PDA
   strictly cheaper); public/claimable reward-per-membership schemes (§3.2, leak tenure by
   construction); biometric PoP (§3.4, centralised issuer); social-graph/ceremony PoP (§3.4,
   empirically fails once any reward exists).
4. **Name the two traps to judges explicitly:** RLN (the mechanism most likely to *look* like a fit,
   solving a problem our architecture doesn't have) and anonymity mining (*increases* the metric most
   likely to look like success — TVL/deposit count — while *provably shrinking* real anonymity). Both
   are the ones a literate reviewer will ask about.

### 3.7 Synthesis matrix

| Mechanism | Deepens **real** k, or only resists **nominal** inflation? | Cost to us | Maturity (checked live) |
|---|---|---|---|
| RLN | **Neither** for our architecture — solves same-identity replay; dominated by a plain `init`-PDA once identity is on-chain | High (new circuit; no Solana port) | Crypto mature (2019); deployment: PSE **sunset**, Waku **testnet** Sept 2025 |
| Anonymity mining | **Actively shrinks** real k (Tutela ~37%); inflates nominal TVL only | N/A — anti-pattern to avoid | Deprecated (2020–21); silent-accrual alternative already adopted in-house |
| Bonding/fee + slashing | Resists nominal inflation ∝ bond size; **not** a hard real-k proof | Medium (spec reserves the PDA; needs slash path + sizing) | Established literature (2014–2023); not built here |
| PoP — World ID | Theoretically real-k; architecturally incompatible (centralised biometric issuer) | Very high; conflicts with permissionless ethos | Production, under active regulatory shutdown in multiple jurisdictions |
| PoP — BrightID | Theoretically real-k; empirically probabilistic (collusion rings) | High (external social-graph dependency we don't have) | Production; not audited against this attack |
| PoP — Idena | Theoretically real-k; **empirically puppeteered** once rewards existed | High (synchronous global ceremony) | Production since 2019; founder-co-authored collapse post-mortem, Jan 2025 |
| Cold-start: protocol-mandated uniformity | Removes the *separate* cold-start problem | **Zero — already the k-floor** | Monero production since 2018/2022 |
| Cold-start: anchor-tenant seeding | Buys liveness, not anonymity — only if seed is behaviorally uniform | Low | Production (ORE / historically Wasabi) |

---

## 4. What to cite where — design-decision → citation map

For the final design docs. "Where" is the doc/section the citation belongs in.

| mirror-pool decision / residual | Cite | Where |
|---|---|---|
| **k-floor is a liveness gate, not an anonymity measure** (`invariants.rs:6`) | Sweeney 2002 (k-anonymity = pure count); Machanavajjhala *et al.* 2007 (count blind to composition) | Spec §4 guarantee (c); the "k candidates not k-anonymity" framing |
| **Whale self-fill residual** (spec:181) — nominal k overstates | Li *et al.* 2007 (skewness / 49-vs-1 twin); Tóth–Hornák–Vajda 2004 (Thm 1, S ≥ −log₂Θ; D1/D2 5%-vs-50%) | Threat table "Sybil / set poisoning" row; Plan 6b rationale |
| **Effective-k = min-entropy `k_∞ = 1/maxᵢ pᵢ`** (Plan 6b metric) | Cachin 1997 (H_∞, Rényi hierarchy); Dodis–Reyzin–Smith 2007 (predictability = 2^{−H_∞}) | Plan 6b spec; the harness's pure fn doc comment |
| **Reject Shannon effective-k `k_H` as a gate** | Tóth–Hornák–Vajda 2004 (Thm 1, one-directional); Massey 1994 / Rioul 2022 (guessing-entropy answers a different question) | Plan 6b spec, "why min-entropy" |
| **`k_H` as a *secondary* descriptive stat only** | Serjantov–Danezis 2002 (S); Díaz *et al.* 2002 (d); Cachin 1997 §2.3 (source-coding, for the `2^H` step — **not** as an "effective-k" name) | Plan 6b spec, monitoring outputs |
| **Guessing advantage `(m−1)/k`** (headline residual number) | Derived-by-us from Dodis–Reyzin–Smith 2007 predictability — **flag as derived, not a named term** | Plan 6b outputs; threat-model residual disclosure |
| **Timing/intersection is the dominant cross-round attack** | Danezis 2003 (closed-form t-to-break); Oya *et al.* 2019 (O(1/ρ) decay); Mathewson–Dingledine 2004 | Spec threat table "Timing correlation" row (replace 34.7% per §2.4) |
| **Variable (≥k) rounds are a disclosure-resistance property** (spec:156) | Mathewson–Dingledine 2004 (pool mix harder than threshold mix) | Spec §4 / threat table — reframe from liveness-only |
| **Stake delegation is the more exposed action type** | Danezis 2003 precondition `m < N/(b−1)` (small N, m≈1) | Pooled-stake design threat notes; Plan 6b per-action output |
| **Fixed denomination forecloses amount fingerprinting** | Béres/Tang/Tutela/Wang (none needed an amount heuristic vs fixed-denom TC) | Spec "Amount fingerprinting" row |
| **Jitter commit→eligible slot** (`eligible_slot`, cf. `cancel_unlock_slot`) | Mathewson–Dingledine 2004 (delay variability most effective) | Timeout/round-scheduling design |
| **k-floor also raises the SNR against intersection attacks** | Danezis 2003 `m < N/(b−1)` | Second justification on the existing k-floor decision |
| **Empirical "how bad is deanonymization" evidence** | Wu *et al.* 2022 (Tutela ~37%); Wang *et al.* 2023 (37.63%/85.26%, **corrected**) | Threat model; replaces the withdrawn 34.7% |
| **Bonding priced as a linear bond** (when built, `["member",pool,C_m]`) | Bissias *et al.* 2014 (Xim); Mazorra–Della Penna 2023 | Incentive-module spec (phase 4) |
| **RLN is future-work-only, wrong surface** | barryWhiteHat 2019; PSE RLN docs (sunset); Waku 2025 | Anti-Sybil future-work note |
| **Incentives must accrue silently** (no claimable per-membership reward) | Wu *et al.* 2022 (Tutela AM leak); Ohlhaver *et al.* 2025 (Idena puppeteering) | Incentive-module spec; §3.4 of industry-practices |
| **k-floor as protocol-mandated uniformity (cold-start)** | Monero ring-size history (getmonero.org; PR #8178) | Spec cold-start / design-lesson |
| **Permissionless coordinators fix a real failure mode** | zkSNACKs 2024 shutdown; CoinJoin coordinator history | Spec §3.2 / open questions |
| **A concentrated seed backfires (cold-start)** | Kappos *et al.* 2018 (Zcash founders/miners ~66%) | Cold-start / anchor-tenant note |
| **Opt-in disclosure, no backdoor** (already a decision) | Elusiv/Arcium regulatory lineage (already in prior-art §7.2) | Spec compliance row |

---

## 5. Honest limitations

1. **`k_∞` needs a funder-clustering input the chain cannot produce.** The min-entropy metric is only
   as good as the clustering signal fed to it (deposit-graph / timing). A poor signal *under*-counts
   `m` and reports an optimistic `k_∞`; a paranoid signal over-counts and reports a pessimistic one.
   On-chain distinct-funder counting has already been assessed as unenforceable
   (`behavioral-rounds-followup-proposal.md` critic review). So Plan 6b's `k_∞` is a
   **host-side measurement/monitoring** number, **not** an on-chain safety gate — and the current on-chain
   `meets_k_floor` remains a nominal-count liveness gate. We measure the residual honestly; we do not
   (yet, maybe ever) *enforce* effective-k on-chain. Presenting `k_∞` as an enforced guarantee would
   be overclaiming.
2. **The whale-self-fill posterior assumes the adversary *can* cluster the m notes.** If they cannot,
   `k_∞` is pessimistic (real anonymity is closer to nominal k). We deliberately model the stronger
   adversary — but this is a modelling choice, not a measured fact about any specific pool.
3. **Derived quantities are labelled, not laundered.** `k_H = 2^{H(X)}` and `Adv_guess = (m−1)/k`
   are our packagings of standard information-theoretic facts, **not** literature-named terms. The
   design docs must keep that distinction; a reviewer who checks Serjantov–Danezis or
   Dodis–Reyzin–Smith will not find these names there.
4. **Preserved `[UNVERIFIED]` flags are load-bearing.** Reiter–Rubin's "1−p" and Berthold *et al.*'s
   "A = log₂N" formulas, Rényi 1961, Massey 1994's page number, and Tóth–Hornák 2004 (PET) are
   `[UNVERIFIED-PRIMARY]` (cited via secondary sources). The SAC '25 wallet-fingerprint result is
   `[UNVERIFIED]` (paywalled, secondary summary). **Cristodaro *et al.* 2025 (the "34.7%" source) is
   `WITHDRAWN`** — do not cite its numbers as stable. None of these should be promoted to confident in
   the design docs.
5. **§2.5's `t`-to-break estimate is only as good as the pool's (m, N, b) estimates.** Danezis's bound
   is exact for its model (uniform recipients, known background); real pools violate those
   assumptions, and Mathewson–Dingledine 2004 show the direction (padding helps less than hoped,
   pooling helps more) but not a closed form for our exact case. Report it as an order-of-magnitude
   risk estimate, not a guarantee.
6. **This survey does not resolve the binding constraint — crowd depth.** Every mechanism in §3 that
   would deepen *real* k is either deferred (bonding), an anti-pattern (anonymity mining), or a poor
   architectural fit (RLN, PoP). The honest position for the bounty is the one already on file: the
   k-floor buys k *candidates*; effective-k measurement (Plan 6b) makes the gap *visible and
   quantified*; closing it is future work, priced but not built.

---

## 6. Frontier-delta — the 2026-07-17 validation & up-to-date-methods pass

A later pass re-verified every load-bearing citation above against primary text and scanned the
current (2009–2026) frontier for methods the survey was missing. Two orchestrated research
workflows (anonymity metrics/attacks/anti-Sybil; and Solana execution limits) proposed 40 candidate
sources; an adversarial grounding gate fetched each and **rejected 4 as fabricated/inapplicable and
demoted 14 to companion-only**, leaving exactly one new *grounding* citation. Results below preserve
the `[VERIFIED]` / `[UNVERIFIED-PRIMARY]` / `WITHDRAWN` discipline; the five fabrication/attribution
traps the pass caught are named in §6.9 so they are never re-introduced.

### 6.1 The metric now has a named peer-reviewed anchor — Smith 2009 (QIF)

`crates/effective-k`'s `effective_k = 1/max_funder_share` is **term-for-term** the reciprocal Bayes
vulnerability of **Geoffrey Smith, "On the Foundations of Quantitative Information Flow," FoSSaCS
2009** (`[VERIFIED]`, primary PDF read in full):

- **Def 1:** `V(X) = maxₓ P[X=x]` (vulnerability = worst-case single-guess success);
- **Def 2:** `H∞(X) = log 1/V(X)` (min-entropy); and *"if X is uniformly distributed among n values,
  then V(X)=1/n and H∞(X)=log n."*

So `effective_k = 1/V(X) = 2^{H∞}` is exactly Smith's measure, and it is **category-correct**: QIF is
the noise-free, single-guess-adversary framework — precisely mirror-pool's threat model (guess
which-of-`k`-identical-actions a funder initiated), with no DP/calibrated-noise assumption. This
**upgrades** the crate's existing Cachin-1997 + Dodis–Reyzin–Smith-2007 grounding to the source that
*defines* the measure (Cachin and DRS are downstream/parallel — Smith himself cites Tóth–Hornák–Vajda
and Cachin on the same page). Backed by Alvim et al. 2020 (*The Science of QIF*) / the CSF 2012
g-leakage paper (Prop. 3.1: the identity gain function reduces g-vulnerability to Bayes vulnerability
`V = maxᵢ pᵢ`).

**Honesty guards preserved.** `k∞ = k/m` and `Adv = (m−1)/k` remain **our labeled arithmetic
instantiations** of Smith Def 1 (a dominant funder owning `m` of `k` notes ⇒ `maxᵢ pᵢ = m/k`), **not**
literature-named theorems — no peer-reviewed source names a "`K_e = 2^{H∞}`" metric or proves a
"`k/m` collapse." In particular, **do not** cite Smith's incidental "`m` guesses ⇒ success ≤ `m·V(X)`"
bound for the `k/m` result: Smith's `m` is a *guess count*, ours is the *whale's note count* — the
symbol collision is coincidental and would be a false citation.

### 6.2 Why not differential privacy? (the answer we were missing)

A literate judge will ask; the honest, citable boundary is **category mismatch, not gap.** DP (Dwork)
and its metric/Bayesian generalizations — metric-DP / geo-indistinguishability
(Andrés–Bordenabe–Chatzikokolakis–Palamidessi, CCS'13), Pufferfish (Kifer–Machanavajjhala, TODS 2014),
computational-DP-lineage AnoA (Backes et al., CSF'13) — all bound how much a **randomized** mechanism's
output shifts between two *adjacent* inputs, achieved by adding **calibrated noise**. Mirror-pool adds
no noise and compares no adjacent inputs: its guarantee is that `k` identical vault-signed actions are
indistinguishable **as a set** — a deterministic many-to-one collapse of `k` initiators onto one
observed action, measured combinatorially via min-entropy. None of these is "the thing we forgot to
add" — there is no randomized mechanism for them to bound. The one partial exception is **AnoA**, whose
adjacency apparatus is abstract enough that a `k`-way assignment ambiguity could *in principle* be
recast as an `(ε=0, δ=0)`-style claim — but AnoA's guarantee is still a computational distinguishing-game
bound, no existing AnoA analysis covers our setting, and that recasting would be **our own future
exercise, not an existing citation**. All four are **companion/contrast only**, never grounding.

### 6.3 Legibility to a crypto-privacy reviewer

Adopt "**effective anonymity set (size)**" as the crypto-legible label for `k∞` — explicitly a
**naming choice, not a claim of prior formal equivalence**. The only crypto-domain paper with an actual
anonymity-set-entropy *formula* is Tutela (App. A.2: uniform Shannon `H₁ = ln D` over uncompromised
candidates) — the **special case** of our Rényi hierarchy at `H₁` under a no-whale (uniform) assumption;
our `k∞ = 1/maxᵢ pᵢ` is Cachin's `H∞` rung, strictly stronger and robust to the whale-self-fill
adversary `H₁` cannot represent. Zcash/Kappos and Tornado/Tutela's *primary* metric is a plain
post-heuristic **count**, which maps to our on-chain **k-floor** (liveness), not to `k∞` — so **report
both `k` and `k∞` side by side**, as `crates/effective-k` already does. Companions: Kappos 2018, Möser
2018, Tutela 2022. **Do not** cite Vijayakumaran's Dulmage–Mendelsohn paper as a *metric* source (it is
an attack), and **do not** invent a named Monero "effective ring size" metric — none exists; flag the
absence.

### 6.4 k-anonymity's 2025 standing *reinforces* us (one framing fix)

The sharpest recent "k-anonymity is not enough" work (Domingo-Ferrer & Sánchez 2025; Chhillar et al.
2025) attacks exactly the **naive nominal-`k`** posture mirror-pool already refuses: the k-floor is a
liveness *count*; effective-`k` (min-entropy) is the guarantee. **Framing risk, not an architecture
gap** — a skimming judge could read "k-floor" as a plain-k-anonymity claim. **Fix:** wherever `k-floor`
appears judge-facing (README, spec), pair it immediately with the effective-`k` caveat.

### 6.5 Batch-ordering side-channel — a real, open mechanism gap

`execute_round` pays the batch in one vault-signed tx iterating **cranker-supplied** `remaining_accounts`
in the order the cranker provides, with **no on-chain shuffle** (`committed_slot` is the only timing
datum at commit). If that order tracks commit order — or the cranker simply picks it — batch **position
re-links initiator → action after the crypto succeeds**. An off-chain "the cranker promises to shuffle"
is **not** a guarantee (unenforceable, silently droppable); a SlotHashes/blockhash-seeded on-chain
permutation is **also inadequate** — that source is **grindable** by exactly the party (cranker/leader)
that controls order and timing. **Derived-by-us fix (label as such, like `k∞`/`Adv`):** `require!()`
on-chain that `remaining_accounts` be **sorted by each intent's commitment/nullifier value** — fixed and
hiding at commit, not chosen by the cranker — removing ordering discretion with no beacon/VRF (within
YAGNI). Document as an open gap parallel to `6c`; see `solana-execution-limits.md` §4 for the *chunking*
variant (worse — a further reason not to chunk). **Companion/contrast cites** (we apply *no* permutation,
so these are contrast, not support): Furukawa–Sako (CRYPTO 2001), Neff (CCS 2001), Wadhwa et al. DIOPE
(CCS 2024, why content/origin-independent on-chain ordering is hard against rational collusion).

### 6.6 Shape uniformity — precedent solid, one real stake-path residual

External precedent **validates** `require!(fee == pool.fee)` + fixed denomination: Tornado Cash
(fixed-`N`-per-pool) and Privacy Pools (Buterin–Illum–Nadler–Schär–Soleimani, 2023/24: "same
denomination is the default; arbitrary amounts need an extra SNARK"). The **withdraw** path is *not* a
channel — fee/denomination are pool-uniform at commit (`lib.rs:150`) and both execute arms
(`lib.rs:335/379`), so the CPI count is provably constant. **Real, previously-undocumented residual in
the STAKE path:** `StakeAction::execute` branches on `self.stake_account.lamports()` (`action.rs:121`)
— an empty stake PDA takes a 1-CPI create path, a **pre-funded** one (an attacker *can* pre-fund: the
`nullifier_hash → intent_pda → stake_pda` derivation is public) takes a 2–3-CPI normalize path, and the
per-intent vault debit differs — even though the delegated balance converges to exactly `to_stake`. The
*amount* invariant holds, but the **inner-instruction trace and per-intent vault debit do not**; given
no shuffle, a chain observer reading `getTransaction` innerInstructions can map trace-shape → intent
position. **Fix:** identical CPI sequence regardless of starting balance, or coordinator pre-crank
top-up to a known floor. Do **not** fold this into a "shape uniformity already solved" narrative.

### 6.7 Empirical freshness (2023–2026)

Our newest *verified* crypto-deanonymization source remains Wang WWW'23; no 2024–2026 result changes a
number here. Companions to add, each scoped:

- **RPC/network-timing** — Wang et al., "Time Tells All," arXiv:2508.21440 (2025): a **submission-layer**
  surface (Solana >95% RPC coverage), *distinct* from our on-chain set-unlinkability — out-of-scope-but-real
  caveat, no effective-`k` change.
- **Railgun anonymity-loss** — arXiv:2606.25926 (2026): newest methodology but **category-inapplicable**
  (shielded-balance mixer that hides *amounts*) — do not import its numbers.
- **Solana network analysis** — Alizadeh & Khabbazian, EPJ Data Science 2025: explicitly *not* a privacy
  study — environmental context that **confirms the Solana-specific deanonymization literature is a
  genuine, citable gap**.
- **Wallet-fingerprint** — Soleti et al., ACM SAC 2025: `[UNVERIFIED-PRIMARY]` (author page only; ACM
  403) — motivates uniform-actor + byte-uniform fee, but drop the "~37% when split by pool" phrasing.

### 6.8 Economic fee — the math *backs* "priced, not solved"

No sound `Fee_min` makes whale self-fill net-negative, and this is now argued, not asserted.
Deanonymizing one honest note by self-fill costs the whale exactly `(k−1)·pool.fee` (each intent
enforces `fee == pool.fee`) — a **bounded, known** cost, further capped because `k ≤ ~17–19` (the tx
account-lock limit, `solana-execution-limits.md` §1), so the *maximum possible* deterrent is only ~18×
the per-action fee. The attacker's benefit `V` (private valuation of deanonymizing the target) is
**unobservable and unbounded**; net-negative needs `Fee_min = V/(k−1)`, unsettable without bounding `V`.
Worse, a byte-uniform fee taxes honest funders equally, so **raising it to deter shrinks the honest set
and lowers `k`** — self-defeating. Backing (named in prose, **not** promoted to confident citations —
they lacked grounding verdicts this pass): Douceur 2002 (impossibility without a central identity
authority), Margolin–Levine 2008 (fee raises cost, never prevents), Xim/Bissias 2014 (`O(Y·τ)` — does
**not** transfer; our cost is one-shot `(k−1)·fee`), Mazorra–Della Penna 2023 (reward-sharing, category
mismatch). The externally-suggested "Economic Sybil Resistance in Cryptographic Pools" title is
**unverifiable/likely nonexistent**; Anoma/Penumbra bonds are validator-consensus Sybil resistance
(category mismatch). **Keep price-not-claim; do not overstate even the pricing benefit.**

### 6.9 Corrections applied, and traps caught

**Applied:**

- **Kappos value-linkage `~66%` → `65.6%`** (primary §6.1/§6.3 = 52.1% + 13.5%; rounds to 66%, substance
  unchanged — a submission cites the exact figure). Fixed in §3.5 above and Appendix #41.
- **Rényi 1961 scope:** now read in full — it defines the general `Hα` family (eq. 1.21) and the `α→1`
  Shannon limit (eq. 1.22), **not** the `α→∞` / min-entropy case. Attribute `H∞` to Smith 2009 / Cachin
  1997 / DRS 2007, never to Rényi 1961 alone. (Appendix #8 refined.)

**Traps the grounding gate caught — never re-introduce:**

- **arXiv:2504.20296** (Mariani & Homoliak, a *real* mixing SoK) contains **none** of "Maximum Defender
  Vulnerability", "Min-Entropy Anonymity Set Size", or "`K_e = 2^{H∞}`" — fabricated attribution
  (full-text fetch). Usable at most as a landscape survey, never for our metric.
- **Cristodaro et al. arXiv:2510.09433/09443** remain **`WITHDRAWN`** (a v2 exists but is *not* a
  corrected version; there is currently *no* citable version) — "34.7% FIFO" stays uncited.
- **"Circle Shuffle"** and a **Solana VRF-course** cite (proposed for the ordering fix) — unverifiable /
  source does not state the grinding claim — rejected.
- **Tutela authorship** is Wu, McTighe, Wang, Seres, Bax et al. (as Appendix #26 already has it) — an
  externally-suggested "Quintyne-Collins / Pareto" credit is wrong and was not adopted.

---

## Appendix — consolidated references with verification status

**Metrics strand.**
1. Serjantov & Danezis (2002), "Towards an Information Theoretic Metric for Anonymity," PET 2002, LNCS 2482, pp. 41–53, DOI 10.1007/3-540-36467-6_4 — `[VERIFIED]`.
2. Díaz, Seys, Claessens & Preneel (2002), "Towards Measuring Anonymity," PET 2002, LNCS 2482, pp. 54–68, DOI 10.1007/3-540-36467-6_5 — `[VERIFIED]`.
3. Reiter & Rubin (1998), "Crowds: Anonymity for Web Transactions," ACM TISSEC 1(1), pp. 66–92 — bibliographic verified; "1−p" formula `[UNVERIFIED-PRIMARY]` (via Díaz *et al.*).
4. Berthold, Federrath & Köpsell (2001), "Web MIXes," Designing PETs, LNCS 2009, pp. 115–129 — bibliographic verified; "A=log₂N" formula `[UNVERIFIED-PRIMARY]`.
5. Sweeney (2002), "k-Anonymity," Int. J. Uncertainty, Fuzziness and Knowledge-Based Systems 10(5), pp. 557–570, DOI 10.1142/S0218488502001648 — `[VERIFIED]`.
6. Machanavajjhala, Kifer, Gehrke & Venkitasubramaniam (2007), "ℓ-Diversity," ACM TKDD 1(1), Art. 3, DOI 10.1145/1217299.1217302 — `[VERIFIED]`.
7. Li, Li & Venkatasubramanian (2007), "t-Closeness," IEEE ICDE 2007, pp. 106–115, DOI 10.1109/ICDE.2007.367856 — `[VERIFIED]`.
8. Rényi (1961), "On Measures of Entropy and Information," Proc. 4th Berkeley Symp., Vol. 1, pp. 547–561 — `[VERIFIED]` (primary read in full): defines the general `Hα` family (eq. 1.21) and the `α→1` Shannon limit (eq. 1.22) **only** — the `α→∞` / min-entropy case is *not* in Rényi 1961; attribute `H∞` to Smith 2009 / Cachin 1997 / DRS 2007 (see §6.9).
9. Cachin (1997), "Entropy Measures and Unconditional Security in Cryptography," PhD Thesis, ETH Zürich No. 12187, https://cachin.com/cc/papers/d.pdf — `[VERIFIED]`.
10. Dodis, Reyzin & Smith (2007), "Fuzzy Extractors" (survey; full version SIAM J. Comput. 38(1), pp. 97–139, 2008; orig. EUROCRYPT 2004), https://cs.nyu.edu/~dodis/ps/fuzzy-survey.pdf — `[VERIFIED]`.
11. Massey (1994), "Guessing and Entropy," IEEE ISIT 1994, Trondheim, p. 204 — bibliographic corroborated; page `[UNVERIFIED-PRIMARY]`; inequality verified via #12.
12. Rioul (2022), "Variations on a Theme by Massey," IEEE Trans. Inf. Theory, arXiv:2102.04200 — `[VERIFIED]`.
13. Tóth, Hornák & Vajda (2004), "Measuring Anonymity Revisited," NordSec 2004, Espoo, pp. 85–90 — `[VERIFIED]`.
14. Tóth & Hornák (2004), "Measuring Anonymity in a Non-adaptive, Real-time System," PET 2003, LNCS 3424, pp. 226–241 — `[UNVERIFIED-PRIMARY]`.
15. Edman, Sivrikaya & Yener (2007), "A Combinatorial Approach to Measuring Anonymity," IEEE ISI 2007, pp. 356–363, https://www.cs.rpi.edu/~yener/PAPERS/SECURITY/isi07.pdf — `[VERIFIED]`.

**Attacks strand.**
16. Kesdogan, Agrawal & Penz (2002), "Limits of Anonymity in Open Environments," IH 2002, LNCS 2578, pp. 53–69, DOI 10.1007/3-540-36415-3_4 — bibliographic/abstract, corroborated 3×.
17. Agrawal & Kesdogan (2003), "Measuring Anonymity: The Disclosure Attack," IEEE S&P 1(6), pp. 27–34, DOI 10.1109/MSECP.2003.1253565 — bibliographic.
18. Danezis (2003), "Statistical Disclosure Attacks," SEC 2003, pp. 421–426, http://www0.cs.ucl.ac.uk/staff/G.Danezis/papers/StatDisclosure.pdf — `[VERIFIED]`, read in full.
19. Danezis & Serjantov (2004), "Statistical Disclosure or Intersection Attacks," IH 2004, LNCS 3200, pp. 293–308, DOI 10.1007/978-3-540-30114-1_21 — bibliographic/abstract (paywalled).
20. Mathewson & Dingledine (2004), "Practical Traffic Analysis," PET 2004, LNCS 3424, pp. 17–34, https://www.freehaven.net/doc/e2e-traffic/e2e-traffic.pdf — `[VERIFIED]`, read in full.
21. Berthold & Langos (2002), "Dummy Traffic Against Long Term Intersection Attacks," PET 2002, LNCS 2482, pp. 110–128, DOI 10.1007/3-540-36467-6_9 — abstract (paywalled).
22. Díaz & Preneel (2004), "Reasoning About the Anonymity Provided by Pool Mixes That Generate Dummy Traffic," IH 2004, LNCS 3200, DOI 10.1007/978-3-540-30114-1_22 — abstract (paywalled).
23. Oya, Troncoso & Pérez-González (2019), "Meet the Family of Statistical Disclosure Attacks," arXiv:1910.07603 — `[VERIFIED]`, read in full.
24. Béres, Seres, Benczúr & Quintyne-Collins (2021), "Blockchain is Watching You," IEEE DAPPS, arXiv:2005.14051 — `[VERIFIED]`, read in full.
25. Tang, Xu, Zhang, Wu & Zhu (2021), "Analysis of Address Linkability in Tornado Cash on Ethereum," CNCERT 2021, DOI 10.1007/978-981-16-9229-1_3 — `[VERIFIED]`, read in full.
26. Wu, McTighe, Wang, Seres, Bax *et al.* (2022), "Tutela," arXiv:2201.06811 — `[VERIFIED]`, read in full.
27. Wang, Chaliasos, Qin, Zhou, Gao, Berrang, Livshits & Gervais (2023), "On How ZK Proof Blockchain Mixers Improve, and Worsen User Privacy," WWW '23, arXiv:2201.09035, DOI 10.1145/3543507.3583217 — `[VERIFIED]`, read in full (**source of the corrected 37.63%/85.26% figures**).
28. "Attacking Anonymity Set in Tornado Cash via Wallet Fingerprints," ACM SAC '25, DOI 10.1145/3672608.3707896 — `[UNVERIFIED]`, secondary summary, paywalled.
29. Cristodaro, Kraner & Tessone (2025), arXiv:2510.09433 / 2510.09443 — **`WITHDRAWN` 2025-11-18; do not cite numbers as stable** (source of the unsafe "34.7%").
30. Chainalysis (Aug 2022), "Understanding Tornado Cash…" and "OFAC Sanctions Popular Ethereum Mixer…," https://www.chainalysis.com/blog/tornado-cash-sanctions-challenges/ — fetched directly ($7.6B mixed / ~30% illicit = source-of-funds taint, **not** a deanonymization %).
31. Piotrowska, Hayes, Elahi, Meiser & Danezis (2017), "The Loopix Anonymity System," USENIX Security, arXiv:1703.00536 — `[UNVERIFIED]`, search-summary only.

**Anti-Sybil strand.**
32. barryWhiteHat (2019), "Semaphore RLN…," ethresear.ch, https://ethresear.ch/t/semaphore-rln-rate-limiting-nullifier-for-spam-prevention-in-anonymous-p2p-setting/5009 — primary post.
33. PSE, RLN docs & project status (sunset), https://rate-limiting-nullifier.github.io/rln-docs/ ; https://pse.dev/en/projects/rln — checked live.
34. Taheri-Boshrooyeh *et al.* (2022), "WAKU-RLN-RELAY," arXiv:2207.00117; Waku monthly updates June/Sept 2025 (testnet status) — checked live.
35. Tornado Cash docs, "anonymity-mining.md," https://github.com/tornadocash/docs — primary (TORN-mining-isolated sub-% `[UNVERIFIED]`; Tutela aggregate confirmed via #26).
36. Bissias, Ozisik, Levine & Liberatore (2014), "Sybil-Resistant Mixing for Bitcoin" (Xim), WPES 2014, https://people.cs.umass.edu/~gbiss/mixing.pdf — primary.
37. Mazorra & Della Penna (2023), "The Cost of Sybils, Credible Commitments, and False-Name Proof Mechanisms," arXiv:2301.12813 — abstract confirmed.
38. World / Worldcoin whitepaper & regulatory record (Kenya court order; Hong Kong PCPD), https://whitepaper.world.org/ ; businessdailyafrica.com ; coindesk.com (May 2024) — primary + press.
39. BrightID docs, https://brightid.gitbook.io/brightid ; https://github.com/BrightID/BrightID-AntiSybil — primary.
40. Ohlhaver, Nikulin & Berman (2025), "Compressed to 0: The Silent Strings of Proof of Personhood," Stanford JBLP 8(1), https://stanford-jblp.pubpub.org/pub/compressed-to-0-proof-personhood/release/5 — primary; Subirà-Nieto (2021), EPFL DEDIS semester project — `[not peer-reviewed]`.
41. Kappos, Yousaf, Maller & Meiklejohn (2018), "An Empirical Analysis of Anonymity in Zcash," USENIX Security 2018, arXiv:1805.03180 — primary.
42. Monero ring-size history, https://www.getmonero.org/resources/moneropedia/ring-size.html ; github.com/monero-project/monero/pull/8178 — primary.
43. Maxwell (2013), "CoinJoin," bitcointalk.org/index.php?topic=279249.0 ; zkSNACKs coordinator discontinuation (2024), https://blog.wasabiwallet.io/zksnacks-is-discontinuing-its-coinjoin-coordination-service-1st-of-june/ — primary.

**Frontier-delta references (2026-07-17 pass).** Grounding verdicts: `GROUND-AND-ADOPT` = usable as
the metric's definitional grounding; `COMPANION` = related-work/contrast only, never grounding.

44. Smith (2009), "On the Foundations of Quantitative Information Flow," FoSSaCS 2009, LNCS 5504, pp. 288–302, DOI 10.1007/978-3-642-00596-1_21 — `[VERIFIED]`, primary PDF read in full (Def 1 `V(X)=maxₓ P[X=x]`; Def 2 `H∞=log 1/V(X)`; uniform ⇒ `V=1/n`). **`GROUND-AND-ADOPT` — the definitional anchor for effective-k.**
45. Alvim, Chatzikokolakis, McIver, Morgan, Palamidessi & Smith (2020), *The Science of Quantitative Information Flow*, Springer, ISBN 978-3-319-96129-3 — metadata verified, Ch. 2 behind auth wall; primary for the g-leakage equivalence is Alvim et al. (2012), "Measuring Information Leakage using Generalized Gain Functions," CSF 2012 (Prop. 3.1, `[VERIFIED]`). **`GROUND-AND-ADOPT` (QIF equivalence backing).**
46. Andrés, Bordenabe, Chatzikokolakis & Palamidessi (2013), "Geo-Indistinguishability," ACM CCS 2013, arXiv:1212.1984 — `[VERIFIED]`. **`COMPANION`** (DP-boundary contrast).
47. Kifer & Machanavajjhala (2014), "Pufferfish," ACM TODS 39(1), DOI 10.1145/2514689 — `[VERIFIED]`. **`COMPANION`** (DP-boundary contrast).
48. Backes, Kate, Manoharan, Meiser & Mohammadi (2013), "AnoA," IEEE CSF 2013 — `[VERIFIED]`. **`COMPANION`** (DP-boundary contrast; the one abstract-enough framework, still a computational IND-CDP game).
49. Möser et al. (2018), "An Empirical Analysis of Traceability in the Monero Blockchain," PoPETs 2018(3):143–163, DOI 10.1515/popets-2018-0025 — `[VERIFIED]`. **`COMPANION`** (crypto-domain legibility).
50. Vijayakumaran (2021/2023), "Analysis of CryptoNote Transaction Graphs using the Dulmage–Mendelsohn Decomposition," IACR ePrint 2021/760 / AFT 2023 — `[VERIFIED]`. **`COMPANION`** — an *attack*, cited only as such (defines no anonymity-set metric).
51. Wang et al. (2025), "Time Tells All: Deanonymization of Blockchain RPC Users with Zero Transaction Fee," arXiv:2508.21440 — `[VERIFIED]` (abstract). **`COMPANION`** (submission-layer/RPC-timing caveat).
52. Huseynov, Shahzaib, Seres & Tapolcai (2026), "A Tattered Cloak of Invisibility: Measuring Anonymity Loss in Railgun on Ethereum," arXiv:2606.25926 — `[VERIFIED]` (abstract). **`COMPANION`** — category-inapplicable (hides amounts), do not import its numbers.
53. Alizadeh & Khabbazian (2025), "Solana's transaction network: analysis, insights, and comparison," EPJ Data Science, DOI 10.1140/epjds/s13688-025-00561-x — `[VERIFIED]`. **`COMPANION`** (environmental context; confirms the Solana-deanon gap).
54. Soleti, Gangwal & Conti (2025), "Attacking Anonymity Set in Tornado Cash via Wallet Fingerprints," ACM SAC 2025, DOI 10.1145/3672608.3707896 — `[UNVERIFIED-PRIMARY]` (author page only; ACM 403). **`COMPANION`**.
55. Furukawa & Sako (2001), "An Efficient Scheme for Proving a Shuffle," CRYPTO 2001, LNCS 2139, pp. 368–387 — `[VERIFIED]`. **`COMPANION`** (verifiable-shuffle contrast — we apply *no* permutation).
56. Neff (2001), "A Verifiable Secret Shuffle and its Application to E-Voting," ACM CCS 2001 — `[VERIFIED]`. **`COMPANION`** (verifiable-shuffle contrast).
57. Wadhwa, Zanolini, D'Amato, Asgaonkar, Fang, Zhang & Nayak (2024), "Data Independent Order Policy Enforcement: Limitations and Solutions," ACM CCS 2024 / IACR ePrint 2023/868 — `[VERIFIED]`. **`COMPANION`** (on-chain-ordering-hardness contrast).
58. Douceur (2002), "The Sybil Attack," IPTPS 2002; Margolin & Levine (2008), "Quantifying Resistance to the Sybil Attack," FC 2008 — economic-Sybil backing **named in prose (§6.8), not promoted to grounding this pass**.

**REFUTED — do not cite for the stated purpose:** Mariani & Homoliak (2025), "SoK: A Survey of Mixing Techniques and Mixers for Cryptocurrencies," arXiv:2504.20296 — a *real* paper, but contains **none** of the min-entropy terms ("Maximum Defender Vulnerability" / "Min-Entropy Anonymity Set Size" / "`K_e = 2^{H∞}`") externally attributed to it (full-text fetch).
