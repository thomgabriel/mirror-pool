# Effective-k measurement core — design spec

> **Status:** design (spec-only — no implementation; the review session checks this against the
> research + source before the plan). Build item **6b** of the mechanism-research pass (item 1,
> `Pool.fee`, is done; item 3, `round_executable_slot`, is a separate later plan). **Host-only
> analysis, ZERO custody surface** — this never runs on-chain and never touches funds.
>
> **Primary source:** `docs/research/anonymity-frontier-and-antisybil.md` §1.2 (the three metrics,
> precisely defined), §1.3 (why min-entropy — proved, not asserted), §1.4 (what 6b implements),
> §5 (honest limitations — the measure-not-enforce boundary + naming honesty). Secondary:
> `docs/research/crowd-depth-and-timing-mechanisms.md` §3.2 + §5.2 item 3 (the mandatory
> treasury-is-the-whale fixture: an operator filling `d` decoy slots is a SINGLE funder →
> `k_∞ = k/d`, identical to a whale).

## What it is, and why

A **pure, host-side function that measures the REAL anonymity of a round** given ground-truth of
**who funded which note.** The on-chain gate `meets_k_floor(intent_count, k_floor)` counts *notes*,
blind to how many distinct real-world entities funded them (`invariants.rs:6`). When one funder
controls `m` of the `k` notes — whale self-fill, or an operator padding `d` decoys — nominal `k`
**overstates** protection. This core computes the honest number.

**It cannot run on-chain, by construction:** it needs a *funder-clustering signal* (deposit-graph /
timing) that the chain cannot see — which is the privacy guarantee working as intended. So this is a
host analysis instrument over a **model** the caller supplies, not chain data.

## The metric (verbatim from §1.2c / §1.2d, sanity checks from §1.2c)

Over one round of `k` pooled actions, let `p_i` = the adversary's posterior that funding-entity `i`
initiated a given action. Reported number:

```
min-entropy effective-k:   k_∞ = 2^{H_∞(X)} = 1 / max_i p_i      (H_∞ = min-entropy)
whale self-fill (one funder owns m of k, clusterable):   k_∞ = k / m
```

- **Guessing advantage over the 1/k baseline** — the residual-anonymity headline:
  `Adv_guess = max_i p_i − 1/k = (m − 1)/k` (additive); multiplicative form `= m` (the dominant
  funder is `m×` likelier to be pinned than nominal `k` implies).
- **Sanity checks the metric MUST pass (and the tests MUST assert):** `m = 1` (no whale) ⇒
  `k_∞ = k`; `m = k` (one funder fills the round) ⇒ `k_∞ = 1`.
- **Why min-entropy, not Shannon or nominal k (§1.3, proved):** the three sit in a strict hierarchy
  on the *same* distribution — `nominal-k (H₀) ≥ Shannon-k (H₁) ≥ k_∞ (H_∞)`. Tóth–Hornák–Vajda 2004
  (`[VERIFIED]`) construct two distributions of *identical Shannon entropy* where one has 5% and the
  other 50% single-guess success — so nominal-k and Shannon-k can look healthy while `k_∞` is at the
  floor. Min-entropy is the conservative, single-shot-correct measure; it is the one to report.
- **Shannon `k_H = 2^{H(X)}` is a SECONDARY, descriptive/trend statistic only** (§1.4) — never the
  reported/gated anonymity number. It is included so the hierarchy `nominal ≥ k_H ≥ k_∞` is testable
  and trend-visible, and it is labeled non-gating everywhere.

## Honest scoping — LOAD-BEARING (the review WILL reject overclaiming)

This is the entire point of the artifact; the language below is a **ceiling**, not modesty.

1. **`k_∞` is a HOST-SIDE MEASUREMENT / monitoring number, NEVER an on-chain gate.** The chain
   cannot produce the funder-clustering signal (that is the privacy guarantee), so on-chain
   distinct-funder counting is **unenforceable** (frontier §5.1). `meets_k_floor` stays exactly as it
   is — a nominal-count *liveness* gate. Presenting `k_∞` as an *enforced guarantee* is the overclaim
   to avoid. We **measure** the residual honestly; we do not (yet, maybe ever) **enforce** effective-k
   on-chain. This is "measurement before mechanism."
2. **The number is only as good as the clustering signal fed in.** A weak signal *under*-counts `m`
   and reports an optimistic `k_∞`; a paranoid one over-counts and reports pessimistic. We
   deliberately model the *stronger* adversary (the whale's notes ARE clusterable) — a modelling
   choice, not a measured fact about any specific pool. State this.
3. **Naming honesty (§1.2, §5.3):** `2^H` "effective-k" and the guessing-advantage formula are **OUR
   packagings of standard information-theoretic facts, NOT literature-named terms.** A reviewer who
   checks Serjantov–Danezis or Dodis–Reyzin–Smith will not find "effective-k" or "`Adv_guess`" there.
   Doc comments cite Cachin 1997 §2.3 (the `2^H` source-coding move) and Dodis–Reyzin–Smith 2007 §2.1
   (predictability `max_a P[A=a] = 2^{−H_∞}` *is* the single-guess success probability), for the
   underlying facts — **not** for the names. Preserve every `[VERIFIED]`/`[UNVERIFIED]` flag.
4. **Action-agnostic metric ≠ action-independent anonymity.** The metric depends only on the
   funder→note distribution, so given the *same* mapping, withdraw and stake yield the *same* `k_∞`
   — that is all "action-agnostic" means, and it is why "measure both" is free. It must **NOT** be
   read as "withdraw and stake are equally anonymous." They are not: the *clustering feasibility* —
   how easily an adversary builds the funder→note mapping — is action-dependent. Stake delegation has
   a richer observable surface (the validator target, timing) and a structurally small target set,
   making it the more exposed / more clusterable action (frontier §2.2), so a *faithful* stake
   measurement feeds a stronger clustering and typically reports a **lower** `k_∞` at the same nominal
   `k`. The metric is action-agnostic; the anonymity is not. Leaving this implicit would be
   overclaim-by-omission — the exact failure mode this artifact exists to prevent.

## Input / output types (the spec pins these)

**Action-agnostic by construction:** the input carries only the funder→note distribution — no action
kind. So for the *same* mapping, withdraw and stake give the identical number; "measure both" is free,
and there must be **no action-kind field and no special-casing.** (This is metric-level
action-agnosticism — NOT a claim that the actions are equally anonymous; the clustering that *builds*
the mapping is action-dependent. See Honest scoping point 4.)

```rust
/// An opaque clustered-funder label. The metric treats it purely as an equality key
/// (which notes share a funder); it never interprets the bytes. A real caller maps its
/// off-chain clustering (deposit-graph / timing) to a representative id — e.g. a Solana
/// Pubkey via `.to_bytes()`. Kept dependency-free (no solana types) so this stays a pure
/// host-analysis crate.
pub struct FunderId([u8; 32]);   // derive Clone, Copy, PartialEq, Eq, Hash

/// Ground-truth of who funded each of the k notes in one round. This is a HOST MODEL —
/// the chain cannot produce this mapping (that is the privacy guarantee). **Non-empty by
/// construction:** the `funders` field is private and `new` rejects an empty round, so
/// `k = self.funders().len() >= 1` always holds — illegal states are unrepresentable,
/// `anonymity_report` stays total (no empty-round branch, no division by zero), and the
/// fail-closed check lives at the boundary. Deliberately carries NO action kind (see the
/// action-agnostic note in Honest scoping).
pub struct RoundComposition { funders: Vec<FunderId> }   // private — construct via `new`

/// Construction error for a `RoundComposition`. (derive `Debug`; impl `Display` +
/// `std::error::Error` by hand — the crate is dependency-free, no `thiserror`.)
pub enum CompositionError { EmptyRound }

impl RoundComposition {
    /// Rejects an empty round (`k = 0` — nothing to measure). `funders[i]` = the funding
    /// entity of note i.
    pub fn new(funders: Vec<FunderId>) -> Result<Self, CompositionError>;
    pub fn funders(&self) -> &[FunderId];
}

/// The measured anonymity of a round. All fields are MONITORING numbers, never on-chain
/// gates. `effective_k` (min-entropy k_∞) is the headline; `shannon_effective_k` is a
/// descriptive/trend stat only (§1.3: it cannot catch whale self-fill); `nominal_k` is what
/// `meets_k_floor` counts, included so the hierarchy nominal ≥ shannon ≥ effective is visible.
pub struct AnonymityReport {
    pub nominal_k: u32,             // = funders.len()
    pub effective_k: f64,          // k_∞ = 1 / max_i p_i = k / m
    pub shannon_effective_k: f64,  // k_H = 2^{H(X)} — DESCRIPTIVE ONLY, non-gating
    pub guessing_advantage: f64,   // Adv_guess = (m − 1) / k, additive over the 1/k baseline
    pub max_funder_share: f64,     // max_i p_i = m / k (the dominant probability mass)
}

/// Pure: count notes per funder, take the max share m/k, derive the report. O(k).
pub fn anonymity_report(comp: &RoundComposition) -> AnonymityReport;

/// A MONITORING predicate: is the measured effective-k below a caller-chosen floor?
/// The threshold is the CALLER's monitoring policy (typically the pool's k_floor, or a
/// stricter alert level) — NOT an enforced on-chain gate. Provided so "collapse verdict"
/// has a home without implying enforcement. `report.effective_k < floor`.
pub fn collapses_below(report: &AnonymityReport, floor: f64) -> bool;
```

The empty-round contract is pinned in the type, not left to the plan: `RoundComposition::new`
rejects `k = 0` (nothing to measure), so `anonymity_report(&RoundComposition) -> AnonymityReport`
is **total** — a valid composition has `k ≥ 1` hence `m ≥ 1`, so there is no empty-round branch and
no division by zero anywhere downstream. (An `anonymity_report(...) -> Result` that validates on each
call is an acceptable alternative, but the validating constructor keeps the hot path total and makes
illegal states unrepresentable — the fail-closed choice.)

## Decisions this spec makes (and justifies)

### D1 — Where it lives: a new dedicated host crate `crates/effective-k`

Not `programs/pool-program` (that is the on-chain custody crate; this is host-only analysis that must
never compile to SBF or add on-chain surface), and not `crates/sdk` (the client *proving* surface — a
different responsibility). A small dedicated crate makes the "host-only, zero custody surface" boundary
**structural** (it literally cannot be pulled into the SBF build), keeps each crate single-purpose, and
is the home the research already anticipated (`crates/anonymity-harness`, §3.3/§5.2). The workspace uses
`members = ["programs/*", "crates/*"]` (glob), so the crate is picked up automatically — no root
`Cargo.toml` edit. It is **dependency-free** (pure Rust + std; `proptest` as a dev-dependency only; no
solana/anchor deps), `license = "MIT"`, `publish = false`, matching the other host crates. Because it is
a host crate, `cargo-llvm-cov` can truthfully measure its coverage (SBF in-VM lines cannot be measured;
this is exactly the code the coverage doctrine wants).

### D2 — `FunderId` is a dependency-free `[u8; 32]` opaque key

The metric only needs to know which notes *share* a funder (an equality/hash key), never to interpret
the identity. `[u8; 32]` matches the domain (a clustered funder resolves to a representative address)
without dragging in a `solana` dependency; a real caller does `FunderId(pubkey.to_bytes())`. Keeping it
concrete (not generic) is the YAGNI choice — a second representation is a second-caller problem.

### D3 — The "collapse verdict" is a monitoring predicate with a caller-supplied threshold, not a gate

`anonymity_report` emits **numbers**; the collapse verdict is `collapses_below(report, floor)`, where
`floor` is the **caller's** monitoring policy. This satisfies the "collapse verdict" output the task
asks for while keeping the honest-scoping boundary intact: the metric imposes no threshold and enforces
nothing. It is a host-side alerting signal, never an on-chain check. (Baking a fixed threshold into the
core would read as an enforced guarantee — precisely the overclaim §5 forbids.)

### D4 — Include Shannon `k_H` as a secondary descriptive field, clearly non-gating

`shannon_effective_k` is cheap (`H(X) = −Σ p_i log₂ p_i`, `k_H = 2^H`), is the trend statistic §1.4
recommends, and lets a test assert the hierarchy `nominal_k ≥ shannon_effective_k ≥ effective_k`
(a real implementation-validating invariant). It is labeled descriptive-only in the type doc and never
fed to `collapses_below`; §1.3 is explicit that Shannon-k cannot catch whale self-fill.

### D5 — Invariant logic in pure `pub fn`s + host unit tests + proptest

The CLAUDE.md-sanctioned shape (the `meets_k_floor` / `split_payout` / `cancel_unlock_slot` doctrine),
and the only code `cargo-llvm-cov` measures truthfully. No I/O, no state, no chain types.

## Testing

### The MANDATORY test (fix-B-equivalent for this build item): treasury-is-the-whale

A property test that models a **labeled operator/treasury `FunderId`** owning `d` of the `k` notes as
**just another clustered funder** and asserts `effective_k == k / d` — *identical* to a whale of size
`d`. This encodes §3.2's finding directly: an operator funding `d` decoys is one funder, so decoys do
NOT deepen real k, and the metric must never treat a labeled treasury address as an exempt category.
Without this test the harness could later be gamed by an operator assuming its own top-ups don't count.

### Proptest invariants (over random funder partitions of `k` notes)

- `m = 1` (all-distinct funders) ⇒ `effective_k == k` (exact) and `guessing_advantage == 0`;
- `m = k` (one funder) ⇒ `effective_k == 1` (exact) and `guessing_advantage == (k−1)/k`;
- **monotonicity:** increasing the dominant funder's share never *increases* `effective_k`;
- **range:** `1 ≤ effective_k ≤ k`, `0 ≤ guessing_advantage ≤ (k−1)/k`, `max_funder_share ∈ [1/k, 1]`;
- **hierarchy:** `nominal_k ≥ shannon_effective_k ≥ effective_k` (§1.3);
- **exactness:** `effective_k == k / m` and `guessing_advantage == (m−1)/k` (integer-exact cases
  compared exactly; general `f64` ratios compared within a small epsilon — the plan notes the epsilon).

### Unit tests

The two sanity checks as explicit named cases; a mixed case (e.g. `k = 17`, one funder owns `m = 6`
→ `effective_k ≈ 2.833`, `guessing_advantage = 5/17`); `collapses_below` at both sides of a floor.

## Out of scope (YAGNI — do not build, do not drop)

- **A `RoundFixture → RoundComposition` adapter.** Build ONLY if a second concrete caller materialises
  (round-engine tests wanting to assert effective-k on a real fixture). The pure metric core + its own
  tests is the first deliverable; the adapter is a second-caller problem.
- **Any on-chain wiring / gating / a `meets_effective_k_floor` instruction.** `k_∞` is unenforceable
  on-chain (frontier §5.1); this crate adds zero on-chain surface. `meets_k_floor` is unchanged.
- **A recursive `(c,ℓ)`-diversity soft constraint** (Machanavajjhala *et al.*) — the closest template
  for a relative-share bound; cited, not built, until a caller needs it.
- **`round_executable_slot`** (item 3, timing) — a separate later plan.

## Citations (with flags preserved — do not launder into confident)

- **The anonymity-probability-distribution formalism / "effective size" = `H` in bits:** Serjantov &
  Danezis 2002 (PET); Díaz *et al.* 2002 (PET, degree of anonymity `d = H/log₂k`) — both `[VERIFIED]`.
- **The `2^H` source-coding move + the Rényi hierarchy `H₀ ≥ H₁ ≥ H_∞`:** Cachin 1997 §2.3/Prop. 2.4
  (`[VERIFIED]`) — cited for the *fact*, NOT as an "effective-k" name precedent.
- **Predictability = single-guess success probability (`max_a P = 2^{−H_∞}`):** Dodis–Reyzin–Smith
  2007 §2.1 (`[VERIFIED]`) — the load-bearing definition behind `k_∞` and `Adv_guess`.
- **Why Shannon-k fails to catch whale self-fill (the proof):** Tóth–Hornák–Vajda 2004 (`[VERIFIED]`,
  Theorem 1, identical-Shannon / 5%-vs-50% construction).
- **The homogeneity/skewness lineage (max-mass, not average dispersion, is the threat):**
  Machanavajjhala *et al.* 2007 (ℓ-diversity); Li–Li–Venkatasubramanian 2007 (t-closeness); Sweeney
  2002 (k-anonymity) — all `[VERIFIED]`.
- **Treasury/operator decoys = the whale re-labeled (`k_∞ = k/d`):**
  `crowd-depth-and-timing-mechanisms.md` §3.2; Mathewson–Dingledine 2004 (padding vs a
  background-aware adversary, `[VERIFIED]`).
- **Do NOT cite** the withdrawn "34.7%" figure (Cristodaro *et al.* 2025, `WITHDRAWN` 2025-11-18).
- **Preserved `[UNVERIFIED-PRIMARY]`:** Massey 1994's page, Rényi 1961, Reiter–Rubin's "1−p",
  Berthold *et al.*'s "A = log₂N" — not needed by this core, but if referenced, keep the flag.

## Standards (CLAUDE.md / code-craft)

Pure host-tested invariant fns + proptest for the invariants; comment *why*, not *what* (the naming-
honesty note is the one rationale comment worth writing); no overengineering (no generics, no adapter,
no on-chain surface until a second caller needs them); conventional commits; spec → plan → TDD.
