//! Preflight: everything the run needs before it touches the protocol,
//! checked up front with actionable failure messages — RPC reachability,
//! the deployed program, the circuit artifacts `commit_intent` needs to
//! prove against, and operator funding for the whole run's budget.

use std::path::{Path, PathBuf};
use std::time::Duration;

use solana_sdk::{native_token::LAMPORTS_PER_SOL, signature::Signature, signature::Signer};

use crate::rpc::{Ctx, SoakError, SoakResult};

/// ~11-12 SOL of protocol spend (10 stake deposits at ~1.003 SOL dominate) —
/// 13 SOL leaves headroom for fees.
const MIN_OPERATOR_LAMPORTS: u64 = 13 * LAMPORTS_PER_SOL;
const AIRDROP_LAMPORTS: u64 = 2 * LAMPORTS_PER_SOL;
const MAX_AIRDROPS: u32 = 20;

pub fn run(ctx: &Ctx) -> SoakResult<()> {
    check_rpc_and_version(ctx)?;
    check_program_executable(ctx)?;
    check_circuit_artifacts()?;
    fund_operator(ctx)?;
    Ok(())
}

fn check_rpc_and_version(ctx: &Ctx) -> SoakResult<()> {
    let version = ctx.client.get_version().map_err(|e| {
        SoakError::new(format!(
            "preflight: RPC endpoint unreachable at {}: {e} (is `solana-test-validator` running?)",
            ctx.client.url()
        ))
    })?;
    ctx.report.set_validator_version(version.solana_core);
    Ok(())
}

fn check_program_executable(ctx: &Ctx) -> SoakResult<()> {
    let account = ctx.client.get_account(&pool_program::ID).map_err(|e| {
        SoakError::new(format!(
            "preflight: pool_program account {} not found: {e} (deploy it: `anchor build` then \
             `solana-test-validator --reset --bpf-program {} target/deploy/pool_program.so`)",
            pool_program::ID,
            pool_program::ID
        ))
    })?;
    if !account.executable {
        return Err(SoakError::new(format!(
            "preflight: account {} exists but is not executable — the wrong program is deployed \
             at this address",
            pool_program::ID
        )));
    }
    Ok(())
}

/// Two levels below `crates/soak` — shared with `phases::withdraw_round` (and
/// Task 3's stake round), which need the same `circuits/build/*` paths to
/// forward to `sdk::MembershipArtifacts`.
pub(crate) fn workspace_root() -> SoakResult<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            SoakError::new("preflight: crates/soak must be two levels below the workspace root")
        })
}

fn check_circuit_artifacts() -> SoakResult<()> {
    let build_dir = workspace_root()?.join("circuits").join("build");
    let required = [
        build_dir.join("membership_js").join("membership.wasm"),
        build_dir.join("membership.r1cs"),
        build_dir.join("membership.zkey"),
        build_dir.join("verification_key.json"),
    ];
    for path in &required {
        if !path.exists() {
            return Err(SoakError::new(format!(
                "preflight: missing circuit artifact {} — run `bash circuits/scripts/setup.sh` first",
                path.display()
            )));
        }
    }
    Ok(())
}

fn fund_operator(ctx: &Ctx) -> SoakResult<()> {
    let operator = ctx.operator.pubkey();
    for _ in 0..MAX_AIRDROPS {
        let balance = ctx
            .client
            .get_balance(&operator)
            .map_err(|e| SoakError::new(format!("preflight: get_balance(operator): {e}")))?;
        if balance >= MIN_OPERATOR_LAMPORTS {
            return Ok(());
        }
        let sig = ctx
            .client
            .request_airdrop(&operator, AIRDROP_LAMPORTS)
            .map_err(|e| {
                SoakError::new(format!(
                "preflight: request_airdrop(operator, 2 SOL) failed: {e} (the validator faucet \
                 may be capped per call or unfunded — the loop retries on the next pass)"
            ))
            })?;
        wait_for_confirmation(ctx, sig)?;
    }
    Err(SoakError::new(format!(
        "preflight: operator {operator} is still under the {} SOL run budget after {MAX_AIRDROPS} \
         airdrops",
        MIN_OPERATOR_LAMPORTS / LAMPORTS_PER_SOL
    )))
}

fn wait_for_confirmation(ctx: &Ctx, sig: Signature) -> SoakResult<()> {
    const MAX_POLLS: u32 = 40;
    for _ in 0..MAX_POLLS {
        let statuses = ctx.client.get_signature_statuses(&[sig]).map_err(|e| {
            SoakError::new(format!("preflight: get_signature_statuses({sig}): {e}"))
        })?;
        if let Some(Some(status)) = statuses.value.into_iter().next() {
            return match status.err {
                None => Ok(()),
                Some(err) => Err(SoakError::new(format!(
                    "preflight: airdrop transaction {sig} failed on-chain: {err:?}"
                ))),
            };
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    Err(SoakError::new(format!(
        "preflight: airdrop transaction {sig} did not confirm in time"
    )))
}
