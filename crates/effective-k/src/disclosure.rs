//! Danezis 2003 statistical disclosure closed form (eq. 4/6) + a seeded l-sigma
//! cross-check simulation. Host-side only — never an on-chain gate (same posture as
//! the parent crate's `anonymity_report`).

/// m = target set size, n = destination universe, b = round size, l = confidence
/// sigmas (2 → 95%, 3 → 99%).
pub struct DisclosureParams {
    pub m: u32,
    pub n: u32,
    pub b: u32,
    pub l: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisclosureError {
    /// b<2, n=0, m=0, m>n, or l<=0 — the closed form is not evaluable for these params.
    PreconditionUnknownableParam,
}

impl std::fmt::Display for DisclosureError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisclosureError::PreconditionUnknownableParam => {
                out.write_str("disclosure params must satisfy b >= 2, n >= 1, 1 <= m <= n, l > 0")
            }
        }
    }
}
impl std::error::Error for DisclosureError {}

fn validate(p: &DisclosureParams) -> Result<(), DisclosureError> {
    if p.b < 2 || p.n < 1 || p.m < 1 || p.m > p.n || p.l <= 0.0 {
        return Err(DisclosureError::PreconditionUnknownableParam);
    }
    Ok(())
}

/// Danezis 2003 eq. 4: the precondition for the attack to even be meaningful.
pub fn precondition_holds(p: &DisclosureParams) -> Result<bool, DisclosureError> {
    validate(p)?;
    // m < N / (b - 1); done in f64 to avoid integer-division truncation of the ratio.
    Ok((p.m as f64) < (p.n as f64) / ((p.b - 1) as f64))
}

/// Danezis 2003 eq. 6: rounds to converge. The ENTIRE bracket is squared — the outer
/// square wraps the whole `m * l * inner` product, not just the inner sum.
pub fn rounds_to_converge(p: &DisclosureParams) -> Result<f64, DisclosureError> {
    validate(p)?;
    let (m, n, b, l) = (p.m as f64, p.n as f64, p.b as f64, p.l);
    let inner = ((m - 1.0) / (m * m)).sqrt() + ((n - 1.0) / (n * n * (b - 1.0))).sqrt();
    Ok((m * l * inner).powi(2))
}

pub enum ConvergeReport {
    AppliesImmediately,
    Rounds(f64),
}

/// `m==1` or `t*<1` is not an observable round count (spec F2) — reported as immediate,
/// never a fractional round.
pub fn converge_report(p: &DisclosureParams) -> Result<ConvergeReport, DisclosureError> {
    let t = rounds_to_converge(p)?;
    if p.m == 1 || t < 1.0 {
        Ok(ConvergeReport::AppliesImmediately)
    } else {
        Ok(ConvergeReport::Rounds(t))
    }
}

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

pub struct DisclosureRun {
    pub identified: bool,
    pub rounds_used: u32,
    pub estimate_hits: u32,
}

/// Danezis eq. 1-2 estimator for one destination: `v_hat = b*O_hat - (b-1)*u_hat`, where
/// `O_hat` is this destination's observed per-round rate (cumulative count / rounds so
/// far) and `u_hat = (b-1)/N` is the known background rate under the uniform-background
/// model this simulation itself draws from.
fn v_hat(count: u64, b: f64, u_hat: f64, rounds_so_far: f64) -> f64 {
    b * (count as f64 / rounds_so_far) - (b - 1.0) * u_hat
}

/// Runs Alice sending to one of her `m` real destinations (uniform) each round, mixed
/// with `b-1` background messages uniform over the `n` destination universe, until the
/// l-sigma criterion (the same one `rounds_to_converge`'s eq. 6 encodes) separates all
/// `m` real destinations from the background, or `max_rounds` is exhausted. All
/// randomness comes from one `SplitMix64::new(seed)` — same seed, same run.
pub fn simulate_disclosure(
    p: &DisclosureParams,
    max_rounds: u32,
    seed: u64,
) -> Result<DisclosureRun, DisclosureError> {
    validate(p)?;
    let (m, n, b) = (p.m as usize, p.n as usize, p.b as usize);
    let mut rng = SplitMix64::new(seed);
    let mut counts = vec![0u64; n];
    let b_f = b as f64;
    let u_hat = (b - 1) as f64 / n as f64;
    let bg_len = (n - m) as f64;

    let mut last_hits = 0u32;
    for round in 1..=max_rounds {
        let alice_target = ((rng.next_f64() * m as f64) as usize).min(m - 1);
        counts[alice_target] += 1;
        for _ in 0..(b - 1) {
            let d = ((rng.next_f64() * n as f64) as usize).min(n - 1);
            counts[d] += 1;
        }

        let round_f = round as f64;
        let mean: f64 = counts[m..]
            .iter()
            .map(|&c| v_hat(c, b_f, u_hat, round_f))
            .sum::<f64>()
            / bg_len;
        let variance: f64 = counts[m..]
            .iter()
            .map(|&c| (v_hat(c, b_f, u_hat, round_f) - mean).powi(2))
            .sum::<f64>()
            / bg_len;
        let threshold = mean + p.l * variance.sqrt();

        let hits = counts[..m]
            .iter()
            .filter(|&&c| v_hat(c, b_f, u_hat, round_f) >= threshold)
            .count() as u32;
        last_hits = hits;
        if hits as usize == m {
            return Ok(DisclosureRun {
                identified: true,
                rounds_used: round,
                estimate_hits: hits,
            });
        }
    }

    Ok(DisclosureRun {
        identified: false,
        rounds_used: max_rounds,
        estimate_hits: last_hits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_is_deterministic() {
        let a: Vec<u64> = {
            let mut r = SplitMix64::new(0xDEADBEEF);
            (0..8).map(|_| r.next_u64()).collect()
        };
        let b: Vec<u64> = {
            let mut r = SplitMix64::new(0xDEADBEEF);
            (0..8).map(|_| r.next_u64()).collect()
        };
        assert_eq!(a, b, "same seed must produce the identical stream");
        let c: Vec<u64> = {
            let mut r = SplitMix64::new(0xDEADBEEE);
            (0..8).map(|_| r.next_u64()).collect()
        };
        assert_ne!(a, c, "different seed must diverge");
    }
}

#[cfg(test)]
mod disclosure_tests {
    use super::*;

    fn approx(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn precondition_matches_eq4() {
        // m < N/(b-1): m=1, N=400, b=11 -> 1 < 40 -> true
        assert!(precondition_holds(&DisclosureParams {
            m: 1,
            n: 400,
            b: 11,
            l: 2.0
        })
        .unwrap());
        // m=50, N=400, b=11 -> 50 < 40 -> false
        assert!(!precondition_holds(&DisclosureParams {
            m: 50,
            n: 400,
            b: 11,
            l: 2.0
        })
        .unwrap());
    }

    #[test]
    fn t_star_matches_eq6_hand_computed() {
        // A hand-computed m>=2 case. m=3, N=400, b=11, l=2:
        //   inner = sqrt((3-1)/9) + sqrt(399/(160000*10))
        //         = sqrt(0.2222222) + sqrt(0.000249375)
        //         = 0.4714045 + 0.0157916 = 0.4871961
        //   t* = (3 * 2 * 0.4871961)^2 = (2.9231768)^2 = 8.5449...
        let t = rounds_to_converge(&DisclosureParams {
            m: 3,
            n: 400,
            b: 11,
            l: 2.0,
        })
        .unwrap();
        assert!(approx(t, 8.5449, 0.01), "eq6 t* mismatch: got {t}");
    }

    #[test]
    fn m_one_reports_applies_immediately() {
        // m=1, N=400, b=11, l=2: t* = (1*2*(0 + sqrt(399/(160000*10))))^2
        //  = (2*0.0157916)^2 = 0.000997 < 1 -> AppliesImmediately, never a fractional t*
        match converge_report(&DisclosureParams {
            m: 1,
            n: 400,
            b: 11,
            l: 2.0,
        })
        .unwrap()
        {
            ConvergeReport::AppliesImmediately => {}
            ConvergeReport::Rounds(t) => panic!("m=1 must be AppliesImmediately, got {t}"),
        }
    }

    #[test]
    fn invalid_params_fail_closed() {
        assert!(rounds_to_converge(&DisclosureParams {
            m: 0,
            n: 400,
            b: 11,
            l: 2.0
        })
        .is_err());
        assert!(rounds_to_converge(&DisclosureParams {
            m: 5,
            n: 400,
            b: 1,
            l: 2.0
        })
        .is_err()); // b-1=0
        assert!(precondition_holds(&DisclosureParams {
            m: 500,
            n: 400,
            b: 11,
            l: 2.0
        })
        .is_err()); // m>n
    }

    // The Tóth–Hornák–Vajda D2 oracle — the min-entropy-vs-Shannon divergence made executable
    // (plan-gate required test; spec §6). Uses effective_k's own API, not the disclosure module.
    #[test]
    fn thv_d2_oracle_minentropy_vs_shannon() {
        use crate::{anonymity_report, FunderId, RoundComposition};
        fn f(x: u8) -> FunderId {
            FunderId([x; 32])
        }
        // D2: k=200 = one whale with 100 notes (funder 0) + 100 distinct singletons (funders 1..=100).
        // H = 0.5 + 0.5·log2(200) = 4.32193 bits ⇒ shannon_effective_k = 2^H = √(2·200) = 20.0 EXACT,
        // while min-entropy effective_k = 200/100 = 2.0 — the 10× gap Shannon hides.
        let mut funders = vec![f(0); 100];
        funders.extend((1..=100u8).map(f));
        let r = anonymity_report(&RoundComposition::new(funders).unwrap());
        assert_eq!(
            r.effective_k, 2.0,
            "min-entropy effective_k = k/m = 200/100"
        );
        assert_eq!(r.max_funder_share, 0.5, "whale holds half the mass");
        assert!(
            (r.shannon_effective_k - 20.0).abs() < 1e-9,
            "2^H = 20.0 exact; Shannon looks 10x healthier"
        );
    }

    #[test]
    fn simulation_run_is_reproducible() {
        let p = DisclosureParams {
            m: 3,
            n: 400,
            b: 11,
            l: 2.0,
        };
        let a = simulate_disclosure(&p, 500, 42).unwrap();
        let b = simulate_disclosure(&p, 500, 42).unwrap();
        assert_eq!(
            (a.identified, a.rounds_used, a.estimate_hits),
            (b.identified, b.rounds_used, b.estimate_hits)
        );
    }

    // Seed-DISTRIBUTION agreement, not single-seed point equality (spec F3):
    // over many seeds, the mean rounds-to-identify tracks t* within a band.
    #[test]
    fn empirical_convergence_tracks_t_star_over_seeds() {
        let p = DisclosureParams {
            m: 3,
            n: 400,
            b: 11,
            l: 2.0,
        };
        let t_star = rounds_to_converge(&p).unwrap();
        let seeds = 200u64;
        let mut hits = 0u32;
        let mut sum = 0u64;
        for s in 0..seeds {
            let run = simulate_disclosure(&p, 2000, s).unwrap();
            if run.identified {
                hits += 1;
                sum += run.rounds_used as u64;
            }
        }
        assert!(
            hits as f64 / seeds as f64 > 0.8,
            "most seeds should identify within max_rounds"
        );
        let mean = sum as f64 / hits as f64;
        // Wide, honest band — this is a stochastic cross-check, not a point equality.
        assert!(
            mean > t_star * 0.25 && mean < t_star * 4.0,
            "mean rounds {mean} should be within a stated band of t*={t_star}"
        );
    }
}
