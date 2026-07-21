//! Host-side measurement of a round's REAL anonymity from a ground-truth funder→note
//! composition. This is a MONITORING instrument, never an on-chain gate: the chain cannot
//! produce the funder-clustering signal (that is the privacy guarantee), so
//! `meets_k_floor` stays the nominal liveness gate and effective-k is unenforceable
//! on-chain. The "effective-k" (2^H) framing and the guessing-advantage formula are our
//! packagings of standard facts (Cachin 1997 §2.3; Dodis–Reyzin–Smith 2007 §2.1), not
//! literature-named terms.

use std::collections::HashMap;

pub mod disclosure;
pub use disclosure::{
    converge_report, precondition_holds, rounds_to_converge, simulate_disclosure, ConvergeReport,
    DisclosureError, DisclosureParams, DisclosureRun, SplitMix64,
};

/// An opaque clustered-funder label — an equality/hash key only; the metric never
/// interprets the bytes. A real caller maps its off-chain clustering to a representative
/// id (e.g. a Solana `Pubkey` via `.to_bytes()`). Kept a plain `[u8; 32]` so this crate
/// stays dependency-free.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FunderId(pub [u8; 32]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositionError {
    EmptyRound,
}

impl std::fmt::Display for CompositionError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompositionError::EmptyRound => {
                out.write_str("round composition must have at least one note")
            }
        }
    }
}
impl std::error::Error for CompositionError {}

/// Ground-truth of who funded each of the k notes in one round (a HOST MODEL — the chain
/// cannot produce this mapping). Non-empty by construction so `anonymity_report` is total.
/// Deliberately carries NO action kind: the metric is action-agnostic (same mapping → same
/// number). NOTE: action-agnostic ≠ action-independent anonymity — the clustering that
/// builds this mapping is action-dependent (stake is more clusterable, frontier §2.2).
#[derive(Debug, PartialEq)]
pub struct RoundComposition {
    funders: Vec<FunderId>,
}

impl RoundComposition {
    /// `funders[i]` = the funding entity of note i. Rejects an empty round (nothing to measure).
    pub fn new(funders: Vec<FunderId>) -> Result<Self, CompositionError> {
        if funders.is_empty() {
            return Err(CompositionError::EmptyRound);
        }
        Ok(Self { funders })
    }

    pub fn funders(&self) -> &[FunderId] {
        &self.funders
    }
}

/// A round's measured anonymity. Every field is a MONITORING number, never an on-chain
/// gate. `effective_k` (min-entropy k_∞) is the headline; `shannon_effective_k` is a
/// descriptive/trend statistic ONLY (it cannot catch whale self-fill — Tóth–Hornák–Vajda
/// 2004); `nominal_k` is what `meets_k_floor` counts, so the hierarchy nominal ≥ shannon ≥
/// effective is visible.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AnonymityReport {
    pub nominal_k: u32,
    pub effective_k: f64,
    pub shannon_effective_k: f64,
    pub guessing_advantage: f64,
    pub max_funder_share: f64,
}

/// Min-entropy effective-k `k_∞ = 1 / max_i p_i = k / m` (m = the dominant funder's note
/// count), the additive guessing advantage `(m − 1)/k`, and the secondary Shannon `2^H`.
/// Total: a `RoundComposition` has `k ≥ 1 ⇒ m ≥ 1`, so no division by zero. O(k).
pub fn anonymity_report(comp: &RoundComposition) -> AnonymityReport {
    let funders = comp.funders();
    let k = funders.len();
    let k_f = k as f64;

    let mut counts: HashMap<FunderId, u32> = HashMap::new();
    for &funder in funders {
        *counts.entry(funder).or_insert(0) += 1;
    }

    let m = counts
        .values()
        .copied()
        .max()
        .expect("k >= 1 by construction");
    let max_funder_share = m as f64 / k_f; // max_i p_i = m/k
    let effective_k = 1.0 / max_funder_share; // k_∞ = k/m
    let guessing_advantage = max_funder_share - 1.0 / k_f; // (m-1)/k

    // Shannon over the per-funder posterior: p = c/k for a funder holding c notes.
    let shannon_bits: f64 = counts
        .values()
        .map(|&c| {
            let p = c as f64 / k_f;
            -p * p.log2()
        })
        .sum();
    let shannon_effective_k = shannon_bits.exp2(); // 2^H — descriptive only

    AnonymityReport {
        nominal_k: k as u32,
        effective_k,
        shannon_effective_k,
        guessing_advantage,
        max_funder_share,
    }
}

/// A MONITORING predicate: is the measured effective-k below a caller-chosen `floor`?
/// The threshold is the caller's monitoring policy (typically the pool's `k_floor`, or a
/// stricter alert level) — NOT an enforced on-chain gate.
pub fn collapses_below(report: &AnonymityReport, floor: f64) -> bool {
    report.effective_k < floor
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(n: u8) -> FunderId {
        let mut b = [0u8; 32];
        b[0] = n;
        FunderId(b)
    }

    // m = 1 (all distinct funders) → k_∞ = k, and zero guessing advantage.
    #[test]
    fn no_whale_gives_nominal_k() {
        let comp = RoundComposition::new((0..5).map(f).collect()).unwrap();
        let r = anonymity_report(&comp);
        assert_eq!(r.nominal_k, 5);
        assert_eq!(r.effective_k, 5.0); // exact: k/1
        assert_eq!(r.guessing_advantage, 0.0);
        assert_eq!(r.max_funder_share, 1.0 / 5.0);
    }

    // m = k (one funder fills the round) → k_∞ = 1 (total anonymity failure).
    #[test]
    fn one_funder_fills_the_round() {
        let comp = RoundComposition::new(vec![f(7); 4]).unwrap();
        let r = anonymity_report(&comp);
        assert_eq!(r.effective_k, 1.0); // exact: k/k
        assert_eq!(r.max_funder_share, 1.0);
        assert!((r.guessing_advantage - 3.0 / 4.0).abs() < 1e-12); // (k-1)/k
    }

    // Mixed: k = 17, one funder owns m = 6 → k_∞ = 17/6, Adv = 5/17.
    #[test]
    fn mixed_whale_share() {
        let mut funders: Vec<FunderId> = vec![f(1); 6]; // the whale: 6 notes
        funders.extend((10..21).map(f)); // 11 distinct singletons → k = 17
        let comp = RoundComposition::new(funders).unwrap();
        let r = anonymity_report(&comp);
        assert_eq!(r.nominal_k, 17);
        assert!((r.effective_k - 17.0 / 6.0).abs() < 1e-12);
        assert!((r.guessing_advantage - 5.0 / 17.0).abs() < 1e-12);
        assert!((r.max_funder_share - 6.0 / 17.0).abs() < 1e-12);
    }

    #[test]
    fn empty_round_is_rejected() {
        assert_eq!(
            RoundComposition::new(vec![]),
            Err(CompositionError::EmptyRound)
        );
    }

    // `collapses_below` is a MONITORING predicate with a caller-supplied floor — never a gate.
    #[test]
    fn collapses_below_is_a_threshold_check() {
        let comp = RoundComposition::new(vec![f(1); 3]).unwrap(); // k_∞ = 1
        let r = anonymity_report(&comp);
        assert!(collapses_below(&r, 2.0)); // 1.0 < 2.0
        assert!(!collapses_below(&r, 1.0)); // 1.0 < 1.0 is false
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;

    fn f(n: u64) -> FunderId {
        let mut b = [0u8; 32];
        b[..8].copy_from_slice(&n.to_le_bytes());
        FunderId(b)
    }

    // MANDATORY (fix-B-equivalent): a labeled operator/treasury funder owning `d` of `k`
    // notes is ONE funder — k_∞ = k/d, identical to a whale of size d. Never exempt.
    proptest! {
        #[test]
        fn treasury_is_the_whale(d in 1usize..=12, singletons in 0usize..=12) {
            const TREASURY: u64 = 999; // just another clustered funder — a fixed label
                                       // outside the singleton range `1..=singletons`.
            let mut funders: Vec<FunderId> = vec![f(TREASURY); d];
            funders.extend((1..=singletons as u64).map(f)); // distinct honest funders
            let k = d + singletons;
            let comp = RoundComposition::new(funders).unwrap();
            let r = anonymity_report(&comp);
            // The treasury holds the largest mass (d >= 1, singletons hold 1 each), so m = d
            // whenever d >= 1 and no singleton batch exceeds it (singletons are size 1).
            prop_assert!((r.effective_k - k as f64 / d as f64).abs() < 1e-9);
            prop_assert_eq!(r.nominal_k, k as u32);
        }
    }

    // A random funder assignment over k notes: label each of k notes with a funder drawn
    // from `n_funders` distinct ids. Asserts the metric's invariants.
    fn composition_strategy() -> impl Strategy<Value = Vec<FunderId>> {
        (1usize..=40).prop_flat_map(|k| {
            (1usize..=k).prop_flat_map(move |n_funders| {
                proptest::collection::vec(0u64..n_funders as u64, k)
                    .prop_map(|labels| labels.into_iter().map(f).collect::<Vec<_>>())
            })
        })
    }

    proptest! {
        #[test]
        fn invariants(funders in composition_strategy()) {
            let k = funders.len();
            // m = the true dominant funder count, computed independently of the metric.
            let mut counts = std::collections::HashMap::new();
            for &x in &funders { *counts.entry(x).or_insert(0u32) += 1; }
            let m = *counts.values().max().unwrap();

            let comp = RoundComposition::new(funders).unwrap();
            let r = anonymity_report(&comp);
            let k_f = k as f64;

            // exactness: k_∞ = k/m, Adv = (m-1)/k, max share = m/k.
            prop_assert!((r.effective_k - k_f / m as f64).abs() < 1e-9);
            prop_assert!((r.guessing_advantage - (m as f64 - 1.0) / k_f).abs() < 1e-9);
            prop_assert!((r.max_funder_share - m as f64 / k_f).abs() < 1e-9);
            // ranges.
            prop_assert!(r.effective_k >= 1.0 - 1e-9 && r.effective_k <= k_f + 1e-9);
            prop_assert!(r.guessing_advantage >= -1e-9 && r.guessing_advantage <= (k_f - 1.0) / k_f + 1e-9);
            prop_assert!(r.max_funder_share >= 1.0 / k_f - 1e-9 && r.max_funder_share <= 1.0 + 1e-9);
            // hierarchy: nominal_k ≥ shannon_k ≥ effective_k (Cachin 1997 Prop. 2.4).
            prop_assert!(r.nominal_k as f64 >= r.shannon_effective_k - 1e-9);
            prop_assert!(r.shannon_effective_k >= r.effective_k - 1e-9);
        }
    }

    // Monotonicity: concentrating a note onto the dominant funder never RAISES effective_k.
    proptest! {
        #[test]
        fn concentration_never_raises_effective_k(funders in composition_strategy()) {
            let comp = RoundComposition::new(funders.clone()).unwrap();
            let before = anonymity_report(&comp).effective_k;
            // Relabel note 0 to match note with the current dominant funder → m increases or holds.
            let mut counts = std::collections::HashMap::new();
            for &x in &funders { *counts.entry(x).or_insert(0u32) += 1; }
            let dominant = *counts.iter().max_by_key(|(_, &c)| c).unwrap().0;
            let mut concentrated = funders.clone();
            concentrated[0] = dominant;
            let after = anonymity_report(&RoundComposition::new(concentrated).unwrap()).effective_k;
            prop_assert!(after <= before + 1e-9);
        }
    }
}
