//! Three-regime adversarial-simulation harness (F2b): the empirical companion to the
//! SOAK. Computes, in the spec's degradation-first order, R2 (whale self-fill sweep),
//! R3 (Danezis 2003 repeated-participation decay, per action profile), and R1 (the
//! distinct-funder baseline), then writes the structured report to
//! `docs/adversarial-sim-report.md`. See `docs/superpowers/specs/2026-07-21-adversarial-sim-design.md`
//! and `docs/ADVERSARIAL-SIM.md` for the honesty framing this report feeds.
//!
//! Run: `cargo run -p effective-k --example adversarial_sim`

use std::fmt::Write as _;
use std::fs;
use std::process::ExitCode;

use effective_k::{
    anonymity_report, converge_report, precondition_holds, simulate_disclosure, ConvergeReport,
    DisclosureError, DisclosureParams, FunderId, RoundComposition,
};

const REPORT_PATH: &str = "docs/adversarial-sim-report.md";

/// R2's sweep uses the withdraw envelope — the larger of the two round sizes, so the
/// collapse curve spans the widest k the protocol schedules.
const R2_K: u32 = 17;

/// The `l`-sigma confidence used throughout R3 (2 -> ~95%, per `DisclosureParams` docs).
const L_SIGMA: f64 = 2.0;

/// R3's quantitative-curve target-set size (m=1 is reported separately as the
/// "applies immediately" case — spec F2 — so the t* curve needs an m>=2 point).
const R3_QUALITATIVE_M: u32 = 3;

const R3_SEED_COUNT: u64 = 200;
const R3_MAX_ROUNDS: u32 = 2000;

#[derive(Debug)]
enum HarnessError {
    Disclosure(DisclosureError),
    /// A hardcoded, fully-controlled invariant (e.g. R1's baseline) failed to hold —
    /// never attacker-influenced input, but still fail-closed rather than assumed.
    Invariant(String),
}

impl std::fmt::Display for HarnessError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HarnessError::Disclosure(e) => write!(out, "disclosure error: {e}"),
            HarnessError::Invariant(msg) => write!(out, "invariant violated: {msg}"),
        }
    }
}
impl std::error::Error for HarnessError {}
impl From<DisclosureError> for HarnessError {
    fn from(e: DisclosureError) -> Self {
        HarnessError::Disclosure(e)
    }
}

fn f(id: u8) -> FunderId {
    FunderId([id; 32])
}

struct WhaleRow {
    m: u32,
    effective_k: f64,
    guessing_advantage: f64,
    max_funder_share: f64,
}

/// R2: one funder (id 0) owns `m` of `k` notes, `(k-m)` singleton funders (ids 1..)
/// fill the rest. Swept worst-first (`m = k .. 1`), so the collapse is the first
/// column a reader meets.
fn whale_sweep(k: u32) -> Vec<WhaleRow> {
    (1..=k)
        .rev()
        .map(|m| {
            let mut funders = vec![f(0); m as usize];
            funders.extend((0..(k - m)).map(|i| f(1 + i as u8)));
            let comp = RoundComposition::new(funders)
                .expect("k >= 1 by construction, never an empty round");
            let r = anonymity_report(&comp);
            WhaleRow {
                m,
                effective_k: r.effective_k,
                guessing_advantage: r.guessing_advantage,
                max_funder_share: r.max_funder_share,
            }
        })
        .collect()
}

struct ActionR3 {
    profile: &'static str,
    n: u32,
    b: u32,
    precondition_m1: bool,
    converge_m1: ConvergeReport,
    precondition_m3: bool,
    converge_m3: ConvergeReport,
    seed_success_rate: f64,
    seed_mean_rounds: f64,
}

fn seed_distribution_summary(
    p: &DisclosureParams,
    max_rounds: u32,
    seeds: u64,
) -> Result<(f64, f64), DisclosureError> {
    let mut hits = 0u32;
    let mut sum = 0u64;
    for s in 0..seeds {
        let run = simulate_disclosure(p, max_rounds, s)?;
        if run.identified {
            hits += 1;
            sum += run.rounds_used as u64;
        }
    }
    let success_rate = hits as f64 / seeds as f64;
    let mean_rounds = if hits > 0 {
        sum as f64 / hits as f64
    } else {
        f64::NAN
    };
    Ok((success_rate, mean_rounds))
}

fn r3_for_profile(profile: &'static str, n: u32, b: u32) -> Result<ActionR3, HarnessError> {
    let p1 = DisclosureParams {
        m: 1,
        n,
        b,
        l: L_SIGMA,
    };
    let p3 = DisclosureParams {
        m: R3_QUALITATIVE_M,
        n,
        b,
        l: L_SIGMA,
    };

    let precondition_m1 = precondition_holds(&p1)?;
    let converge_m1 = converge_report(&p1)?;
    let precondition_m3 = precondition_holds(&p3)?;
    let converge_m3 = converge_report(&p3)?;
    let (seed_success_rate, seed_mean_rounds) =
        seed_distribution_summary(&p3, R3_MAX_ROUNDS, R3_SEED_COUNT)?;

    Ok(ActionR3 {
        profile,
        n,
        b,
        precondition_m1,
        converge_m1,
        precondition_m3,
        converge_m3,
        seed_success_rate,
        seed_mean_rounds,
    })
}

struct R1Baseline {
    k: u32,
    effective_k: f64,
    guessing_advantage: f64,
}

/// R1: `k` distinct singleton funders. The mechanism's happy path — asserted, not just
/// reported, because if this baseline doesn't hold the metric itself is broken.
fn r1_baseline(k: u32) -> Result<R1Baseline, HarnessError> {
    let comp = RoundComposition::new((0..k as u8).map(f).collect())
        .expect("k >= 1 by construction, never an empty round");
    let r = anonymity_report(&comp);
    if r.effective_k != k as f64 {
        return Err(HarnessError::Invariant(format!(
            "R1 baseline: expected effective_k == {k}, got {}",
            r.effective_k
        )));
    }
    if r.guessing_advantage != 0.0 {
        return Err(HarnessError::Invariant(format!(
            "R1 baseline: expected guessing_advantage == 0, got {}",
            r.guessing_advantage
        )));
    }
    Ok(R1Baseline {
        k,
        effective_k: r.effective_k,
        guessing_advantage: r.guessing_advantage,
    })
}

struct HarnessData {
    r2: Vec<WhaleRow>,
    r3: Vec<ActionR3>,
    r1: R1Baseline,
}

fn compute() -> Result<HarnessData, HarnessError> {
    let r2 = whale_sweep(R2_K);
    let r3 = vec![
        // withdraw: N = 100_000, b = MAX_K_WITHDRAW = 17 (pool_program::invariants).
        r3_for_profile("withdraw", 100_000, 17)?,
        // stake: N = 200, b = MAX_K_STAKE = 10 (pool_program::invariants).
        r3_for_profile("stake", 200, 10)?,
    ];
    let r1 = r1_baseline(R2_K)?;
    Ok(HarnessData { r2, r3, r1 })
}

fn format_converge(cr: &ConvergeReport) -> String {
    match cr {
        ConvergeReport::AppliesImmediately => "applies immediately (t* < 1 round)".to_string(),
        ConvergeReport::Rounds(t) => format!("t* = {t:.4} rounds"),
    }
}

fn render(data: &HarnessData) -> String {
    let mut out = String::new();

    writeln!(out, "# Adversarial Simulation Report").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "- Date: {}", now_utc_string()).unwrap();
    writeln!(out, "- Git commit: {}", git_commit_sha()).unwrap();
    writeln!(out).unwrap();

    writeln!(
        out,
        "## R2 — Whale self-fill sweep (k = {}, worst-first)",
        R2_K
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "| m | effective_k | guessing_advantage | max_funder_share |"
    )
    .unwrap();
    writeln!(out, "|---|---|---|---|").unwrap();
    for row in &data.r2 {
        writeln!(
            out,
            "| {} | {:.4} | {:.4} | {:.4} |",
            row.m, row.effective_k, row.guessing_advantage, row.max_funder_share
        )
        .unwrap();
    }
    writeln!(out).unwrap();

    writeln!(
        out,
        "## R3 — Repeated-participation decay (Danezis 2003, per action profile)"
    )
    .unwrap();
    writeln!(out).unwrap();
    for a in &data.r3 {
        writeln!(out, "### {} (N = {}, b = {})", a.profile, a.n, a.b).unwrap();
        writeln!(out).unwrap();
        writeln!(
            out,
            "- precondition_holds(m=1) = {} (m=1 < N/(b-1) = {:.4})",
            a.precondition_m1,
            a.n as f64 / (a.b - 1) as f64
        )
        .unwrap();
        writeln!(
            out,
            "- converge_report(m=1) = {}",
            format_converge(&a.converge_m1)
        )
        .unwrap();
        writeln!(
            out,
            "- precondition_holds(m={}) = {}",
            R3_QUALITATIVE_M, a.precondition_m3
        )
        .unwrap();
        writeln!(
            out,
            "- converge_report(m={}) = {}",
            R3_QUALITATIVE_M,
            format_converge(&a.converge_m3)
        )
        .unwrap();
        writeln!(
            out,
            "- seed-distribution summary (m={}, {} seeds 0..{}, max_rounds={}): success_rate = {:.4}, mean_rounds = {:.4}",
            R3_QUALITATIVE_M, R3_SEED_COUNT, R3_SEED_COUNT, R3_MAX_ROUNDS,
            a.seed_success_rate, a.seed_mean_rounds
        )
        .unwrap();
        writeln!(out).unwrap();
    }

    writeln!(out, "## R1 — Distinct-funder baseline (k = {})", data.r1.k).unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "- effective_k = {:.4} (assert PASS: == k)",
        data.r1.effective_k
    )
    .unwrap();
    writeln!(
        out,
        "- guessing_advantage = {:.4} (assert PASS: == 0)",
        data.r1.guessing_advantage
    )
    .unwrap();
    writeln!(out).unwrap();

    writeln!(out, "**RUN PASSED**").unwrap();
    out
}

fn render_failed(reason: &str) -> String {
    let mut out = String::new();
    writeln!(out, "# Adversarial Simulation Report").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "- Date: {}", now_utc_string()).unwrap();
    writeln!(out, "- Git commit: {}", git_commit_sha()).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "**RUN FAILED**").unwrap();
    writeln!(out, "- Failure: {reason}").unwrap();
    out
}

fn git_commit_sha() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn now_utc_string() -> String {
    std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn main() -> ExitCode {
    let report = match compute() {
        Ok(data) => render(&data),
        Err(e) => {
            eprintln!("adversarial_sim: RUN FAILED: {e}");
            let failed = render_failed(&e.to_string());
            if let Err(write_err) = fs::write(REPORT_PATH, failed) {
                eprintln!("adversarial_sim: also failed to write the report: {write_err}");
            }
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = fs::write(REPORT_PATH, &report) {
        eprintln!("adversarial_sim: failed to write {REPORT_PATH}: {e}");
        return ExitCode::FAILURE;
    }

    println!("adversarial_sim: wrote {REPORT_PATH}");
    ExitCode::SUCCESS
}
