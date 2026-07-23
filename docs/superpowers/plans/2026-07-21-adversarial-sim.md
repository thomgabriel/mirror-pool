# §6.5 Adversarial Simulation Implementation Plan (F2b)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** A host-side adversarial-simulation harness in `crates/effective-k` that measures how
mirror-pool's behavioral anonymity degrades across three regimes (distinct funders / whale
self-fill / repeated participation) and emits a captured proof artifact `docs/ADVERSARIAL-SIM.md`
— adversarial against ourselves, every number reducing to a verified closed form.

**Architecture:** per the fork-approved spec
`docs/superpowers/specs/2026-07-21-adversarial-sim-design.md` (read §2 regimes, §3 output, §4
honesty ledger — binding). R1/R2 reuse the existing `anonymity_report`; R3 is a new dep-free
`disclosure` module (Danezis 2003 closed form + a seeded cross-check simulation). A small example
binary drives all three and writes the report.

**Tech Stack:** Rust 2021, `crates/effective-k` (dep-free lib; `proptest` dev-dep already present).
No new dependencies — the PRNG is in-crate.

## Global Constraints

- **Every reported number reduces to a verified closed form** (spec §4): `k_∞ = 2^{H_∞} =
  1/maxᵢpᵢ` (min-entropy *definition* = Cachin/DRS/Smith; the `k_∞ = k/m` whale specialization
  stays labeled **derived-by-us**), Danezis `t*` (SEC 2003 eq. 6). No fabricated numbers.
- **The Danezis `t*` formula, transcribed with the ENTIRE bracket squared** (mis-parenthesization
  is the #1 risk — plan-gate check):
  ```
  t* = ( m · l · ( sqrt((m-1)/m^2) + sqrt((N-1)/(N^2*(b-1))) ) )^2
  ```
  The outer square wraps the whole `m·l·(…)` product, not just the inner sum.
- **The simulation's "identified" criterion is the SAME l-sigma separation `t*` is derived from**
  (spec F3, non-negotiable): each real destination's estimate exceeds background by `l` standard
  deviations — NOT a different top-m test. Agreement is asserted over a **seed distribution**, never
  a single seed.
- **Adversarial-honesty (spec F5):** the structured report surfaces the R2 collapse AND the R3
  decay as first-class fields; the doc leads with degradation (R2/R3) before the R1 baseline.
- **m=1 guard (spec F2):** for `m=1` or any `t* < 1`, report "precondition holds ⇒ applies from
  the first rounds", never a fractional-round `t*`. The quantitative curve lives at `m ≥ 2`.
- Crate stays **dep-free** (lib); `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings`
  + `cargo test -p effective-k` green at every commit; `cargo test --workspace` stays green.
- Fail-closed on invalid params (return `Result`, never panic on caller input — the crate's
  existing `CompositionError` idiom).
- Conventional commits. Branch `feat/adversarial-sim` (already checked out); never touch `main`.

---

### Task 1: The `disclosure` module — Danezis closed form + seeded simulation + tests

**Files:**
- Create: `crates/effective-k/src/disclosure.rs`
- Modify: `crates/effective-k/src/lib.rs` (add `pub mod disclosure;` + re-export the public items)

**Interfaces (Task 2 relies on these exact names):**
- `pub struct DisclosureParams { pub m: u32, pub n: u32, pub b: u32, pub l: f64 }` (m = target
  set size, n = destination universe, b = round size, l = confidence sigmas: 2→95%, 3→99%).
- `pub enum DisclosureError { … }` (e.g. `PreconditionUnknownableParam` for b<2 / n=0 / m=0 /
  m>n) — fail-closed, `Display`+`Error` like `CompositionError`.
- `pub fn precondition_holds(p: &DisclosureParams) -> Result<bool, DisclosureError>` — `m < N/(b−1)`.
- `pub fn rounds_to_converge(p: &DisclosureParams) -> Result<f64, DisclosureError>` — eq. 6 above.
- `pub enum ConvergeReport { AppliesImmediately, Rounds(f64) }` + `pub fn converge_report(p) ->
  Result<ConvergeReport, DisclosureError>` — returns `AppliesImmediately` when `m==1` or `t*<1`
  (spec F2), else `Rounds(t*)`.
- `pub struct SplitMix64 { state: u64 }` with `pub fn new(seed: u64)` + `pub fn next_u64(&mut self)`
  + `pub fn next_f64(&mut self)` (uniform [0,1)) — in-crate, dep-free.
- `pub struct DisclosureRun { pub identified: bool, pub rounds_used: u32, pub estimate_hits: u32 }`.
- `pub fn simulate_disclosure(p: &DisclosureParams, max_rounds: u32, seed: u64) ->
  Result<DisclosureRun, DisclosureError>` — see Step 5.

- [ ] **Step 1: SplitMix64 + its determinism test (write the test first)**

Append to a new `disclosure.rs`. SplitMix64 is the standard constant set:
```rust
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1) from the top 53 bits (f64 mantissa).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}
```
Test (the fork's added requirement — the whole seed-distribution argument depends on this):
```rust
#[test]
fn splitmix64_is_deterministic() {
    let a: Vec<u64> = { let mut r = SplitMix64::new(0xDEADBEEF); (0..8).map(|_| r.next_u64()).collect() };
    let b: Vec<u64> = { let mut r = SplitMix64::new(0xDEADBEEF); (0..8).map(|_| r.next_u64()).collect() };
    assert_eq!(a, b, "same seed must produce the identical stream");
    let c: Vec<u64> = { let mut r = SplitMix64::new(0xDEADBEEE); (0..8).map(|_| r.next_u64()).collect() };
    assert_ne!(a, c, "different seed must diverge");
}
```

- [ ] **Step 2: Run it — verify pass**

Run: `cargo test -p effective-k splitmix64_is_deterministic`
Expected: PASS.

- [ ] **Step 3: Write failing tests for the closed forms (the plan-gate oracles, verbatim)**

```rust
#[cfg(test)]
mod disclosure_tests {
    use super::*;

    fn approx(a: f64, b: f64, tol: f64) -> bool { (a - b).abs() <= tol }

    #[test]
    fn precondition_matches_eq4() {
        // m < N/(b-1): m=1, N=400, b=11 -> 1 < 40 -> true
        assert!(precondition_holds(&DisclosureParams { m: 1, n: 400, b: 11, l: 2.0 }).unwrap());
        // m=50, N=400, b=11 -> 50 < 40 -> false
        assert!(!precondition_holds(&DisclosureParams { m: 50, n: 400, b: 11, l: 2.0 }).unwrap());
    }

    #[test]
    fn t_star_matches_eq6_hand_computed() {
        // A hand-computed m>=2 case. m=3, N=400, b=11, l=2:
        //   inner = sqrt((3-1)/9) + sqrt(399/(160000*10))
        //         = sqrt(0.2222222) + sqrt(0.000249375)
        //         = 0.4714045 + 0.0157916 = 0.4871961
        //   t* = (3 * 2 * 0.4871961)^2 = (2.9231768)^2 = 8.5449...
        let t = rounds_to_converge(&DisclosureParams { m: 3, n: 400, b: 11, l: 2.0 }).unwrap();
        assert!(approx(t, 8.5449, 0.01), "eq6 t* mismatch: got {t}");
    }

    #[test]
    fn m_one_reports_applies_immediately() {
        // m=1, N=400, b=11, l=2: t* = (1*2*(0 + sqrt(399/(160000*10))))^2
        //  = (2*0.0157916)^2 = 0.000997 < 1 -> AppliesImmediately, never a fractional t*
        match converge_report(&DisclosureParams { m: 1, n: 400, b: 11, l: 2.0 }).unwrap() {
            ConvergeReport::AppliesImmediately => {}
            ConvergeReport::Rounds(t) => panic!("m=1 must be AppliesImmediately, got {t}"),
        }
    }

    #[test]
    fn invalid_params_fail_closed() {
        assert!(rounds_to_converge(&DisclosureParams { m: 0, n: 400, b: 11, l: 2.0 }).is_err());
        assert!(rounds_to_converge(&DisclosureParams { m: 5, n: 400, b: 1, l: 2.0 }).is_err()); // b-1=0
        assert!(precondition_holds(&DisclosureParams { m: 500, n: 400, b: 11, l: 2.0 }).is_err()); // m>n
    }

    // The Tóth–Hornák–Vajda D2 oracle — the min-entropy-vs-Shannon divergence made executable
    // (plan-gate required test; spec §6). Uses effective_k's own API, not the disclosure module.
    #[test]
    fn thv_d2_oracle_minentropy_vs_shannon() {
        use crate::{anonymity_report, FunderId, RoundComposition};
        fn f(x: u8) -> FunderId { FunderId([x; 32]) }
        // D2: k=200 = one whale with 100 notes (funder 0) + 100 distinct singletons (funders 1..=100).
        // H = 0.5 + 0.5·log2(200) = 4.32193 bits ⇒ shannon_effective_k = 2^H = √(2·200) = 20.0 EXACT,
        // while min-entropy effective_k = 200/100 = 2.0 — the 10× gap Shannon hides.
        let mut funders = vec![f(0); 100];
        funders.extend((1..=100u8).map(f));
        let r = anonymity_report(&RoundComposition::new(funders).unwrap());
        assert_eq!(r.effective_k, 2.0, "min-entropy effective_k = k/m = 200/100");
        assert_eq!(r.max_funder_share, 0.5, "whale holds half the mass");
        assert!((r.shannon_effective_k - 20.0).abs() < 1e-9, "2^H = 20.0 exact; Shannon looks 10x healthier");
    }
}
```
(If `FunderId`'s field or `RoundComposition::new` differ from this shape, adapt to the real API in
`lib.rs` — the assertion *values* (2.0 / 0.5 / 20.0) are the oracle and are non-negotiable.)
Run: `cargo test -p effective-k disclosure_tests` → Expected: FAIL (functions undefined).

- [ ] **Step 4: Implement precondition + t* + converge_report**

```rust
pub fn precondition_holds(p: &DisclosureParams) -> Result<bool, DisclosureError> {
    validate(p)?;
    // m < N / (b - 1); done in f64 to avoid integer-division truncation of the ratio.
    Ok((p.m as f64) < (p.n as f64) / ((p.b - 1) as f64))
}

pub fn rounds_to_converge(p: &DisclosureParams) -> Result<f64, DisclosureError> {
    validate(p)?;
    let (m, n, b, l) = (p.m as f64, p.n as f64, p.b as f64, p.l);
    let inner = ((m - 1.0) / (m * m)).sqrt() + ((n - 1.0) / (n * n * (b - 1.0))).sqrt();
    // ENTIRE bracket squared — the outer square wraps the whole m*l*inner product.
    Ok((m * l * inner).powi(2))
}

pub fn converge_report(p: &DisclosureParams) -> Result<ConvergeReport, DisclosureError> {
    let t = rounds_to_converge(p)?;
    // m=1 (=> first radical 0) or any sub-round t* is not an observable round count (spec F2).
    if p.m == 1 || t < 1.0 {
        Ok(ConvergeReport::AppliesImmediately)
    } else {
        Ok(ConvergeReport::Rounds(t))
    }
}
```
`validate(p)`: `b >= 2`, `n >= 1`, `1 <= m <= n`, `l > 0` — else the matching `DisclosureError`.

- [ ] **Step 5: The seeded cross-check simulation (l-sigma criterion shared with t*)**

`simulate_disclosure` runs rounds until Alice's `m` real destinations are each separated from the
background by `l` sigma (the SAME criterion `t*` encodes), or `max_rounds` is hit:
- Model: each round, Alice sends to one of her `m` real destinations (uniform); the other `b−1`
  slots are background draws uniform over the `N` universe. Accumulate per-destination observation
  counts; the estimator is `v̂ = b·Ō − (b−1)·û` per Danezis eq. 1–2 (Ō = observed rate with Alice,
  û = background rate).
- **"Identified" criterion, pinned concretely (plan-gate — do NOT invent a different spread
  measure, or the shared-criterion guarantee silently breaks):** compute the mean `μ` and the
  empirical standard deviation `σ` of `v̂` over the **non-Alice (background) destinations only**;
  the round is "identified" iff **every** one of Alice's `m` real destinations has `v̂ ≥ μ + l·σ`.
  This is the same `l`-sigma separation `t*` (eq. 6) is derived from — that shared definition is
  the whole point of the cross-check.
- Return `DisclosureRun { identified, rounds_used, estimate_hits }`.
- Determinism: all randomness via one `SplitMix64::new(seed)` — same seed ⇒ identical run.

Tests:
```rust
#[test]
fn simulation_run_is_reproducible() {
    let p = DisclosureParams { m: 3, n: 400, b: 11, l: 2.0 };
    let a = simulate_disclosure(&p, 500, 42).unwrap();
    let b = simulate_disclosure(&p, 500, 42).unwrap();
    assert_eq!((a.identified, a.rounds_used, a.estimate_hits),
               (b.identified, b.rounds_used, b.estimate_hits));
}

// Seed-DISTRIBUTION agreement, not single-seed point equality (spec F3):
// over many seeds, the mean rounds-to-identify tracks t* within a band.
#[test]
fn empirical_convergence_tracks_t_star_over_seeds() {
    let p = DisclosureParams { m: 3, n: 400, b: 11, l: 2.0 };
    let t_star = rounds_to_converge(&p).unwrap();
    let seeds = 200u64;
    let mut hits = 0u32;
    let mut sum = 0u64;
    for s in 0..seeds {
        let run = simulate_disclosure(&p, 2000, s).unwrap();
        if run.identified { hits += 1; sum += run.rounds_used as u64; }
    }
    assert!(hits as f64 / seeds as f64 > 0.8, "most seeds should identify within max_rounds");
    let mean = sum as f64 / hits as f64;
    // Wide, honest band — this is a stochastic cross-check, not a point equality.
    assert!(mean > t_star * 0.25 && mean < t_star * 4.0,
        "mean rounds {mean} should be within a stated band of t*={t_star}");
}
```
(The band is deliberately wide: the point is that the coded estimator and coded `t*` are in the
same ballpark — a mis-parenthesized `t*` or a wrong success criterion would blow past a 4× band.
If the real simulation's mean lands outside, that is a FINDING to surface, not a number to widen
the band around.)

- [ ] **Step 6: fmt/clippy, run all disclosure tests, commit**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test -p effective-k`
Expected: all green (existing 8 + the new disclosure tests).
```bash
git add crates/effective-k/src/disclosure.rs crates/effective-k/src/lib.rs
git commit -m "feat(effective-k): disclosure module — Danezis t* closed form + seeded l-sigma cross-check simulation"
```

---

### Task 2: The three-regime harness + `docs/ADVERSARIAL-SIM.md` + captured run

**Files:**
- Create: `crates/effective-k/examples/adversarial_sim.rs` (the harness binary),
  `docs/ADVERSARIAL-SIM.md`
- Commit: the captured `docs/adversarial-sim-report.md` the run produces.

**Interfaces:** consumes Task 1's `disclosure::*` and the existing
`effective_k::{RoundComposition, FunderId, anonymity_report, AnonymityReport}`.

- [ ] **Step 1: the harness (`cargo run -p effective-k --example adversarial_sim`)**

Computes and prints a structured, stable-format report to `docs/adversarial-sim-report.md`.
Regimes, in the spec's degradation-first order:

- **R2 whale sweep (leads):** for `k = 17` (the withdraw envelope) sweep `m = k … 1` (worst
  first). For each `m`, build a `RoundComposition` of one funder with `m` notes + `(k−m)`
  singleton funders, call `anonymity_report`, and emit a row: `m`, `effective_k`,
  `guessing_advantage`, `max_funder_share`. First column a reader meets = the collapse
  (`m=k ⇒ effective_k=1.0`).
- **R3 decay:** for the two action profiles, **each at its OWN measured round envelope** (plan-gate
  correction — `b` is per-action): withdraw `(N large, e.g. 100_000, b = MAX_K_WITHDRAW = 17)` and
  stake `(N small, e.g. 200, b = MAX_K_STAKE = 10)`. Print `precondition_holds`, `converge_report`
  (m=1 ⇒ "applies immediately"; also show an `m=3` row so the quantitative curve appears), and the
  seeded simulation's seed-distribution summary (success rate + mean rounds over ~200 seeds).
  (Stake precondition at m=1: `1 < 200/9 ≈ 22` — holds.)
  Frame the asymmetry as **forced (stake) vs permitted (withdraw)** per spec §2 R3 — do NOT rank
  by `t*`, do NOT claim large-N shields the precondition.
- **R1 baseline (last):** `k=17` distinct singletons → assert `effective_k == 17`,
  `guessing_advantage == 0`.

The report is a first-class structured artifact: R2's collapse and R3's decay are explicit fields
(spec F5). Fail-closed: any `Result::Err` from the disclosure fns aborts with a non-zero exit and
a marked-failed report (mirror the SOAK's discipline).

- [ ] **Step 2: run it → capture the report**

Run: `cargo run -p effective-k --example adversarial_sim`
Expected: exits 0, writes `docs/adversarial-sim-report.md` with the R2 table, R3 rows, R1 baseline.
Read it — sanity-check `m=17 ⇒ effective_k=1.0`, `m=1 ⇒ effective_k=17`, stake precondition holds,
withdraw m=3 `t*` is a sane round count.

- [ ] **Step 3: write `docs/ADVERSARIAL-SIM.md` (hand-framed, degradation-first)**

Structure per spec §3 (degradation headlines FIRST):
1. *The degradation headlines* — the R2 collapse curve + the R3 decay result lead the doc.
2. *Whale-sweep contrast table (R2→R1, worst first)* — the collapsing effective-k is the first
   number.
3. *R3 decay* — precondition, `t*` (m≥2; "applies immediately" for m=1), the seed-distribution
   agreement, the **forced-vs-permitted** per-action framing (with the eq. 4 note: large N makes
   the precondition *easier*, so exposure is behavioral on `m`, not `N`).
4. *What this measures / does NOT establish* — equal prominence: synthetic model; clustering is an
   **assumed adversary capability, grounded in the empirically-demonstrated §2.3 public-chain
   clustering** (assumed-because-demonstrated, not convenient); neither "your pool IS clusterable"
   nor "synthetic so pools are safe"; closed forms carry their own assumptions; measures disclosed
   residuals, adds no mechanism.
5. *Reproduce it* — `cargo run -p effective-k --example adversarial_sim` + what to compare.
6. The embedded captured `docs/adversarial-sim-report.md` verbatim.
Keep the `2^H`/`k_∞`/`Adv` derived-by-us labels; cite Danezis 2003 + Smith 2009/Cachin/DRS for the
definitions, `k_∞ = k/m` as derived-by-us. No mainnet/overclaim language.

- [ ] **Step 4: embed the captured report byte-exact + gate + commit**

Embed `docs/adversarial-sim-report.md` into SOAK-style §6 of ADVERSARIAL-SIM.md, byte-identical to
the committed report file (verify with an extraction diff, as the SOAK did).
Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace 2>&1 | tail -3`
Expected: all green, workspace unchanged + the new disclosure tests.
```bash
git add crates/effective-k/examples/adversarial_sim.rs docs/ADVERSARIAL-SIM.md docs/adversarial-sim-report.md
git commit -m "feat(effective-k): three-regime adversarial-sim harness + ADVERSARIAL-SIM.md proof doc + captured run"
```
