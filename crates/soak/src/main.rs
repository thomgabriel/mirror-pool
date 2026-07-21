//! Drives one complete live protocol exercise against a local
//! `solana-test-validator` over real RPC and emits `docs/soak-report.md`.
//! See `docs/superpowers/specs/2026-07-20-soak-design.md` for the run shape.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, signature::Keypair};

use rpc::{Ctx, SoakError, SoakResult};
use soak::{phases, report::Report, rpc};

const DEFAULT_URL: &str = "http://127.0.0.1:8899";
const DEFAULT_REPORT_PATH: &str = "docs/soak-report.md";

struct Args {
    url: String,
    report_path: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut url = DEFAULT_URL.to_string();
    let mut report_path = PathBuf::from(DEFAULT_REPORT_PATH);
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--url" => {
                i += 1;
                url = args
                    .get(i)
                    .ok_or_else(|| "--url requires a value".to_string())?
                    .clone();
            }
            "--report" => {
                i += 1;
                report_path = PathBuf::from(
                    args.get(i)
                        .ok_or_else(|| "--report requires a value".to_string())?,
                );
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }
    Ok(Args { url, report_path })
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("soak: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Finalized commitment on a single-node localnet lags ~30 slots behind
    // confirmed — at finalized, every send/read in a run with dozens of
    // transactions would stall for 15-25s each. Confirmed is the right tier
    // for a devtool run against a local validator.
    let client = RpcClient::new_with_commitment(args.url.clone(), CommitmentConfig::confirmed());
    let operator = Keypair::new();
    let report = Report::new(pool_program::ID);
    let ctx = Ctx {
        client,
        operator,
        report,
    };

    match run(&ctx, &args.report_path) {
        Ok(()) => {
            if ctx.report.all_passed() {
                ExitCode::SUCCESS
            } else {
                eprintln!("soak: assertion failure(s) — see report");
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            ctx.report.mark_failed(e.to_string());
            if let Err(write_err) = ctx.report.finish(&args.report_path) {
                eprintln!("soak: also failed to write the report: {write_err}");
            }
            eprintln!("soak: RUN FAILED: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(ctx: &Ctx, report_path: &Path) -> SoakResult<()> {
    let preflight_start = Instant::now();
    phases::preflight::run(ctx)?;
    ctx.report
        .phase_timing("preflight", preflight_start.elapsed());

    let setup_start = Instant::now();
    let setup = phases::setup::run(ctx)?;
    ctx.report.phase_timing("setup", setup_start.elapsed());
    ctx.report.note(&format!(
        "setup: vote_account={} withdraw_pool={} withdraw_vault={} stake_pool={} stake_vault={} \
         mints=({}, {}) stake_denomination={}",
        setup.vote_account,
        setup.withdraw_pool,
        setup.withdraw_vault,
        setup.stake_pool,
        setup.stake_vault,
        setup.mints.0,
        setup.mints.1,
        setup.stake_denomination,
    ));

    let withdraw_start = Instant::now();
    phases::withdraw_round::run(ctx, &setup)?;
    ctx.report.phase_timing(
        "withdraw round (k = MAX_K_WITHDRAW)",
        withdraw_start.elapsed(),
    );

    ctx.report
        .note("stake round: not yet implemented (Task 2 scope only — lands in Task 3)");

    ctx.report
        .finish(report_path)
        .map_err(|e| SoakError::new(format!("writing report to {}: {e}", report_path.display())))
}
