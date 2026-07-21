//! The stake round: `MAX_K_STAKE` deposits into the stake pool -> one
//! prove-and-commit loop (same builder as the withdraw round, `SOAK_STAKE_FEE`
//! as the fee argument) -> chunked-ALT v0 execute via
//! `build_execute_stake_round_ix` -> assertions A1/A2/A3/A5/A6/A7 (shared
//! helpers) plus the stake-only A8 final-state check. Blueprint:
//! `crates/sdk/tests/e2e.rs::sdk_driven_stake_round`, RPC-ified. See
//! `docs/superpowers/specs/2026-07-20-soak-design.md` §2.4/§3.
//!
//! No A4 (duplicate-commit / nullifier-spend) probe here — that single-spend
//! property is protocol-generic and already exercised live by the withdraw
//! round; this round's distinguishing claim is the stake-specific A8.

#![allow(deprecated)] // `stake::config::ID` (see `sdk::ix`'s identical allow)

use std::time::Instant;

use pool_program::invariants::MAX_K_STAKE;
use sdk::{
    build_commit_intent_ix, build_deposit_ix, build_execute_stake_round_ix, round_pda,
    stake_account_pda, MembershipArtifacts, MerkleTree, Note,
};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, pubkey::Pubkey, signature::Signer, stake,
    system_program, sysvar,
};

use crate::assertions;
use crate::phases::preflight::workspace_root;
use crate::phases::setup::{SetupOut, SOAK_STAKE_FEE};
use crate::rpc::{create_and_fill_alt, send_ixs, send_v0, Ctx, SoakError, SoakResult};

/// Same headroom rationale as the withdraw round's limit: the stake path runs
/// 4 CPIs + a `find_program_address` per intent (~55,300 CU at k=2, per
/// `sdk::ix::build_execute_stake_round_ix`'s doc comment), well above default
/// but comfortably under the 1.4M cap at k = MAX_K_STAKE.
const EXECUTE_CU_LIMIT: u32 = 1_000_000;

pub fn run(ctx: &Ctx, setup: &SetupOut) -> SoakResult<()> {
    let k = MAX_K_STAKE as usize;
    let pool = setup.stake_pool;
    let vault = setup.stake_vault;
    let validator = setup.vote_account;
    let denomination = setup.stake_denomination;
    let round0 = round_pda(pool, 0);
    let round1 = round_pda(pool, 1);

    let build_dir = workspace_root()?.join("circuits").join("build");
    let wasm_path = build_dir.join("membership_js").join("membership.wasm");
    let r1cs_path = build_dir.join("membership.r1cs");
    let zkey_path = build_dir.join("membership.zkey");
    let artifacts = MembershipArtifacts {
        wasm_path: &wasm_path,
        r1cs_path: &r1cs_path,
        zkey_path: &zkey_path,
    };

    // 1. Deposits — all k land first, so the tree root is final before any proof.
    let deposit_start = Instant::now();
    let mut tree =
        MerkleTree::new().map_err(|e| SoakError::new(format!("stake: MerkleTree::new: {e:?}")))?;
    let mut notes = Vec::with_capacity(k);
    for i in 0..k {
        let note = Note::new();
        tree.insert(note.commitment());
        send_ixs(
            ctx,
            &format!("stake: deposit[{i}]"),
            &[build_deposit_ix(
                pool,
                vault,
                ctx.operator.pubkey(),
                note.commitment(),
                denomination,
            )],
            &[&ctx.operator],
        )?;
        notes.push(note);
    }
    ctx.report
        .phase_timing("stake: deposits", deposit_start.elapsed());
    let root = tree.root();

    // 2. One prove-and-commit loop — the SDK builder proves INTERNALLY.
    let prove_start = Instant::now();
    let mut triples = Vec::with_capacity(k);
    let mut payout_pairs = Vec::with_capacity(2 * k);
    let mut stake_recipient_pairs = Vec::with_capacity(k);
    let mut forbidden = Vec::with_capacity(k);
    for (i, note) in notes.iter().enumerate() {
        let recipient = Pubkey::new_unique();
        let relayer = Pubkey::new_unique();
        let path = tree.authentication_path(i);
        let build = build_commit_intent_ix(
            pool,
            round0,
            recipient,
            relayer,
            ctx.operator.pubkey(),
            note,
            &path,
            root,
            SOAK_STAKE_FEE,
            0,
            artifacts,
        )
        .map_err(|e| SoakError::new(format!("stake: build_commit_intent_ix[{i}]: {e}")))?;

        let (intent_pda, _) = Pubkey::find_program_address(
            &[
                b"intent",
                pool.as_ref(),
                build.public_inputs.nullifier_hash.as_ref(),
            ],
            &pool_program::ID,
        );
        let stake_pda = stake_account_pda(pool, intent_pda);

        send_ixs(
            ctx,
            &format!("stake: commit_intent[{i}]"),
            std::slice::from_ref(&build.instruction),
            &[&ctx.operator],
        )?;

        triples.push((intent_pda, stake_pda, relayer));
        payout_pairs.push((stake_pda, denomination - SOAK_STAKE_FEE));
        payout_pairs.push((relayer, SOAK_STAKE_FEE));
        stake_recipient_pairs.push((stake_pda, recipient));
        forbidden.push(relayer);
    }
    ctx.report.phase_timing(
        "stake: prove + commit_intent (k = MAX_K_STAKE)",
        prove_start.elapsed(),
    );

    // 3. ALT: the round's infra keys, the shared stake tail, and every per-intent triple.
    let mut alt_addresses = vec![
        pool,
        round0,
        round1,
        vault,
        system_program::ID,
        validator,
        stake::program::ID,
        stake::config::ID,
        sysvar::clock::ID,
        sysvar::stake_history::ID,
        sysvar::rent::ID,
    ];
    for (intent, stake_pda, relayer) in &triples {
        alt_addresses.push(*intent);
        alt_addresses.push(*stake_pda);
        alt_addresses.push(*relayer);
    }
    let alt = create_and_fill_alt(ctx, &alt_addresses)?;

    // 4. Execute — vault balance snapshotted PRE-execute for A2.
    let vault_pre = ctx
        .client
        .get_balance(&vault)
        .map_err(|e| SoakError::new(format!("stake: get_balance(vault) pre-execute: {e}")))?;
    let exec_sig = send_v0(
        ctx,
        "stake: execute_round",
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(EXECUTE_CU_LIMIT),
            build_execute_stake_round_ix(
                pool,
                vault,
                ctx.operator.pubkey(),
                0,
                validator,
                &triples,
            ),
        ],
        &alt,
        &[&ctx.operator],
    )?;

    // 5. Assertions A1/A2/A3/A5/A6/A7 (shared helpers) + A8 (stake-only).
    let tx = assertions::fetch_tx(ctx, exec_sig)?;
    assertions::assert_signer_set(ctx, &tx, exec_sig, &[ctx.operator.pubkey()], &forbidden)?;
    assertions::assert_vault_delta(ctx, vault, vault_pre, k as u64, denomination)?;
    assertions::assert_uniform_payouts(ctx, &payout_pairs)?;
    assertions::assert_round_lifecycle(ctx, pool, 0)?;
    assertions::effective_k_section(ctx, k, ctx.operator.pubkey())?;
    assertions::assert_envelope(ctx, &tx, exec_sig)?;
    assertions::assert_stake_final_state(ctx, &stake_recipient_pairs, validator)?;

    Ok(())
}
