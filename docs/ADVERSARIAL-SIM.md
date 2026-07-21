# Adversarial simulation — the empirical "it actually hides" proof (F2b)

`crates/effective-k`'s `disclosure` module plus the `adversarial_sim` example
(`cargo run -p effective-k --example adversarial_sim`) run the deanonymization heuristics that
still bite against mirror-pool's own model, across three regimes, and report the **measured**
outcome of each — never a hand-written number. This is the empirical companion to the SOAK: the
SOAK (`docs/SOAK.md`) proves the mechanism runs live; this proves how well it hides, and — the
load-bearing property — **it is adversarial against ourselves**. It reports the regimes where the
mechanism degrades at equal prominence with the regime where it works. Design:
`docs/superpowers/specs/2026-07-21-adversarial-sim-design.md`.

## 1. The degradation headlines

Two results lead this document, before anything else, because a proof doc that shows only the
happy path is worthless:

- **Whale self-fill collapses effective-k to its floor.** One funder self-filling all `k = 17`
  notes in a round drives the min-entropy `effective_k = k_∞ = k/m` from `17.0` down to exactly
  `1.0` — total behavioral-anonymity failure, computed, not assumed. The captured run's full curve
  is §2 below.
- **Repeated participation converges, on a measurable schedule.** A participant who reuses a fixed
  small set of `m` destinations across rounds is de-anonymizable in a bounded number of rounds
  (Danezis 2003's statistical-disclosure closed form). For mirror-pool's own stake action, the
  structural shape that makes this attack apply (`m ≈ 1`, forced by the stake-deactivation
  cooldown) is present *today* — see §3.

## 2. Whale-sweep contrast table (R2 → R1, worst case first)

`k = 17` (the withdraw envelope), one funder owns `m` notes and `(k−m)` singleton funders own the
rest, swept `m = 17 … 1`. `effective_k = k_∞ = 1/maxᵢpᵢ = k/m` is the min-entropy definition
(Cachin 1997 §2.3; Dodis–Reyzin–Smith 2007 §2.1; Smith 2009, FoSSaCS, as the definitional anchor)
specialized to a single dominant funder — that `k/m` specialization, and the `guessing_advantage =
(m−1)/k` and `max_funder_share = m/k` labels, are **derived-by-us**, not literature-named terms.
The collapsing number is the first column a reader meets:

| m | effective_k | guessing_advantage | max_funder_share |
|---|---|---|---|
| 17 | 1.0000 | 0.9412 | 1.0000 |
| 16 | 1.0625 | 0.8824 | 0.9412 |
| 15 | 1.1333 | 0.8235 | 0.8824 |
| 14 | 1.2143 | 0.7647 | 0.8235 |
| 13 | 1.3077 | 0.7059 | 0.7647 |
| 12 | 1.4167 | 0.6471 | 0.7059 |
| 11 | 1.5455 | 0.5882 | 0.6471 |
| 10 | 1.7000 | 0.5294 | 0.5882 |
| 9 | 1.8889 | 0.4706 | 0.5294 |
| 8 | 2.1250 | 0.4118 | 0.4706 |
| 7 | 2.4286 | 0.3529 | 0.4118 |
| 6 | 2.8333 | 0.2941 | 0.3529 |
| 5 | 3.4000 | 0.2353 | 0.2941 |
| 4 | 4.2500 | 0.1765 | 0.2353 |
| 3 | 5.6667 | 0.1176 | 0.1765 |
| 2 | 8.5000 | 0.0588 | 0.1176 |
| 1 | 17.0000 | 0.0000 | 0.0588 |

`m = 17` (one funder self-fills the round) bottoms out at `effective_k = 1.0` — the honest
statement that a fully self-filled round has zero behavioral anonymity. `m = 1` (the R1 baseline,
every funder distinct) is the bottom row, not the top: it's the mechanism *working*, shown last
because this document leads with where it fails.

## 3. R3 — Repeated-participation decay

A repeat participant ("Alice") sends to a fixed set of `m` real destinations from a universe of
size `N`, across rounds of size `b`. Danezis 2003's statistical-disclosure closed form gives:

- the **precondition** `m < N/(b−1)` (eq. 4) — whether the attack is even meaningful;
- **`t*`**, the rounds-to-converge at confidence `l` (eq. 6, entire bracket squared) —
  **reported only for `m ≥ 2`**; for `m = 1` (or any `t* < 1`) the harness reports "applies
  immediately" rather than a fractional round count, since a sub-1-round number isn't an
  observable round count and would overstate attack ease;
- a **seeded cross-check simulation**: a coded estimator (`v̂ = b·Ō − (b−1)·û`, Danezis eqs. 1–2)
  run against synthetic rounds, declaring a destination "identified" by the *same* `l`-sigma
  separation criterion `t*` is derived from. This is a self-consistency check between the coded
  estimator and the coded `t*` — not independent literature validation — so agreement is asserted
  over a **distribution of seeds** (200 fixed seeds), never a single run.

The harness runs this for mirror-pool's two action profiles, **each at its own measured round
envelope** — `b` is per-action, not a single number:

| Profile | N | b | precondition(m=1) | converge(m=1) | precondition(m=3) | converge(m=3) | seed success rate | seed mean rounds |
|---|---|---|---|---|---|---|---|---|
| withdraw | 100,000 | 17 | holds (1 < 6250.0) | applies immediately | holds | t* = 8.0269 | 1.0000 | 5.5550 |
| stake | 200 | 10 | holds (1 < 22.2222) | applies immediately | holds | t* = 8.8179 | 1.0000 | 7.8150 |

Both profiles' seed-mean rounds land within the honest cross-check band around their own `t*`
(withdraw: 5.555 vs 8.03; stake: 7.815 vs 8.82 — both comfortably inside 0.25×–4×), confirming the
coded estimator and the coded `t*` agree with each other.

**The per-action asymmetry is forced-vs-permitted on `m`, not a ranking by `t*`, and not a claim
that large `N` shields the precondition.** Eq. 4's right-hand side `N/(b−1)` *grows* with `N`, so a
larger destination universe makes the precondition *easier* to satisfy for any fixed `m`, never
harder — ranking exposure by `t*` magnitude would read backwards (`t*` is smaller, i.e. faster
convergence, at *larger* `N` here, not slower). The honest asymmetry is behavioral, on `m`:

- **Stake *forces* the vulnerable shape.** The ~1-epoch stake-deactivation cooldown pins `m ≈ 1` —
  a fixed, stable target — against a small, enumerable `N` (validators receiving pool delegations,
  hundreds not millions). The Alice model applies structurally, and the identified edge
  (funder → validator) is concrete. This is **structural exposure**.
- **Withdraw merely *permits* it.** A user who reuses a fixed small recipient set is fully
  attackable at large `N` too, and — per eq. 4 — the precondition is trivially satisfied and
  convergence is *faster* there, not slower. Withdraw's protection is that fresh-recipient rotation
  is available and costless, so the destination set grows with participation and voids the
  fixed-small-`m` model. That is **user-behavior-conditional safety**, not structural safety — a
  weaker, more honest claim than "withdraw is safe."

## 4. What this measures — and what it does NOT establish

At equal prominence with the results above:

- This is a **synthetic model** over generated round compositions and participation traces, not a
  measurement of any real deployment's deposit graph.
- **Funder clustering is an assumed adversary capability**, an input to the harness — not
  fantastical: public-chain address clustering is empirically demonstrated elsewhere (Béres et
  al., Tang et al., Tutela, Wang et al. — see the research doc §2.3), so the assumption is
  **assumed-because-demonstrated, not assumed-because-convenient**. This harness does not run that
  clustering itself.
- This does **not** establish that any given real mirror-pool deposit graph *is* clusterable — no
  "your pool is broken" overclaim.
- It also does **not** establish that a synthetic model means real pools are safe — no "synthetic,
  so nothing to worry about" dismissal either. Both overclaims are wrong in the same way: this
  measures a model, not a deployment.
- The closed forms carry their own assumptions (a global passive observer, stable per-round
  behavior). They are cited definitions (Danezis 2003; min-entropy per Cachin/DRS/Smith 2009), not
  re-derived here — only the `k/m` whale specialization and the `2^H`/`Adv` labels are ours.
- This measures residuals the design already discloses (independent funder clustering degrades
  effective-k; repeated fixed-destination behavior is statistically convergent). **It adds no
  mechanism that removes them** — no bonding, no mixing, no decoy traffic. Measurement, not
  defense.

## 5. Reproduce it

```bash
cargo run -p effective-k --example adversarial_sim
```

Writes `docs/adversarial-sim-report.md`. Compare the freshly written file against the copy
embedded in §6 below: the R2 table (17 rows, `m=17 ⇒ effective_k=1.0`, `m=1 ⇒ effective_k=17.0`),
the R3 rows per action profile, and the R1 baseline assertions should all match exactly — the
harness is deterministic (fixed compositions, fixed `DisclosureParams`, fixed seeds 0..200); only
the date and git-commit lines will differ on a rerun from a different commit.

## 6. The captured report

The following is `docs/adversarial-sim-report.md` as committed alongside this document, byte-exact
to the file produced by the run above.

```markdown
# Adversarial Simulation Report

- Date: 2026-07-21T18:38:01Z
- Git commit: ac78f9fc1b384619e0ceace48123e57b54fca0ee

## R2 — Whale self-fill sweep (k = 17, worst-first)

| m | effective_k | guessing_advantage | max_funder_share |
|---|---|---|---|
| 17 | 1.0000 | 0.9412 | 1.0000 |
| 16 | 1.0625 | 0.8824 | 0.9412 |
| 15 | 1.1333 | 0.8235 | 0.8824 |
| 14 | 1.2143 | 0.7647 | 0.8235 |
| 13 | 1.3077 | 0.7059 | 0.7647 |
| 12 | 1.4167 | 0.6471 | 0.7059 |
| 11 | 1.5455 | 0.5882 | 0.6471 |
| 10 | 1.7000 | 0.5294 | 0.5882 |
| 9 | 1.8889 | 0.4706 | 0.5294 |
| 8 | 2.1250 | 0.4118 | 0.4706 |
| 7 | 2.4286 | 0.3529 | 0.4118 |
| 6 | 2.8333 | 0.2941 | 0.3529 |
| 5 | 3.4000 | 0.2353 | 0.2941 |
| 4 | 4.2500 | 0.1765 | 0.2353 |
| 3 | 5.6667 | 0.1176 | 0.1765 |
| 2 | 8.5000 | 0.0588 | 0.1176 |
| 1 | 17.0000 | 0.0000 | 0.0588 |

## R3 — Repeated-participation decay (Danezis 2003, per action profile)

### withdraw (N = 100000, b = 17)

- precondition_holds(m=1) = true (m=1 < N/(b-1) = 6250.0000)
- converge_report(m=1) = applies immediately (t* < 1 round)
- precondition_holds(m=3) = true
- converge_report(m=3) = t* = 8.0269 rounds
- seed-distribution summary (m=3, 200 seeds 0..200, max_rounds=2000): success_rate = 1.0000, mean_rounds = 5.5550

### stake (N = 200, b = 10)

- precondition_holds(m=1) = true (m=1 < N/(b-1) = 22.2222)
- converge_report(m=1) = applies immediately (t* < 1 round)
- precondition_holds(m=3) = true
- converge_report(m=3) = t* = 8.8179 rounds
- seed-distribution summary (m=3, 200 seeds 0..200, max_rounds=2000): success_rate = 1.0000, mean_rounds = 7.8150

## R1 — Distinct-funder baseline (k = 17)

- effective_k = 17.0000 (assert PASS: == k)
- guessing_advantage = 0.0000 (assert PASS: == 0)

**RUN PASSED**
```
