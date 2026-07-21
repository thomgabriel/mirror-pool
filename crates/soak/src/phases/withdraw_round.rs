//! The withdraw round: `MAX_K_WITHDRAW` deposits -> one prove-and-commit loop
//! -> the A4 duplicate-commit probe (round still Open) -> chunked-ALT v0
//! execute -> assertions A1-A7. Blueprint: `crates/sdk/tests/e2e.rs`'s
//! withdraw path, RPC-ified. See
//! `docs/superpowers/specs/2026-07-20-soak-design.md` §2-3.

use std::time::Instant;

use pool_program::invariants::MAX_K_WITHDRAW;
use sdk::{
    build_commit_intent_ix, build_deposit_ix, build_execute_round_ix, round_pda,
    MembershipArtifacts, MerkleTree, Note,
};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, pubkey::Pubkey, signature::Signer, system_program,
};

use crate::assertions;
use crate::phases::preflight::workspace_root;
use crate::phases::setup::{SetupOut, WITHDRAW_DENOMINATION, WITHDRAW_FEE};
use crate::rpc::{create_and_fill_alt, send_ixs, send_v0, Ctx, SoakError, SoakResult};

/// Well above the ~400k the LiteSVM helpers use, comfortably under the 1.4M cap.
const EXECUTE_CU_LIMIT: u32 = 1_000_000;

pub fn run(ctx: &Ctx, setup: &SetupOut) -> SoakResult<()> {
    let k = MAX_K_WITHDRAW as usize;
    let pool = setup.withdraw_pool;
    let vault = setup.withdraw_vault;
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
    let mut tree = MerkleTree::new()
        .map_err(|e| SoakError::new(format!("withdraw: MerkleTree::new: {e:?}")))?;
    let mut notes = Vec::with_capacity(k);
    for i in 0..k {
        let note = Note::new();
        tree.insert(note.commitment());
        send_ixs(
            ctx,
            &format!("withdraw: deposit[{i}]"),
            &[build_deposit_ix(
                pool,
                vault,
                ctx.operator.pubkey(),
                note.commitment(),
                WITHDRAW_DENOMINATION,
            )],
            &[&ctx.operator],
        )?;
        notes.push(note);
    }
    ctx.report
        .phase_timing("withdraw: deposits", deposit_start.elapsed());
    let root = tree.root();

    // 2. One prove-and-commit loop — the SDK builder proves INTERNALLY.
    let prove_start = Instant::now();
    let mut triples = Vec::with_capacity(k);
    let mut nullifier_pdas = Vec::with_capacity(k);
    let mut payout_pairs = Vec::with_capacity(2 * k);
    let mut forbidden = Vec::with_capacity(2 * k);
    let mut probe = None;
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
            WITHDRAW_FEE,
            0,
            artifacts,
        )
        .map_err(|e| SoakError::new(format!("withdraw: build_commit_intent_ix[{i}]: {e}")))?;

        let (intent_pda, _) = Pubkey::find_program_address(
            &[
                b"intent",
                pool.as_ref(),
                build.public_inputs.nullifier_hash.as_ref(),
            ],
            &pool_program::ID,
        );
        let (nullifier_pda, _) = Pubkey::find_program_address(
            &[
                b"nullifier",
                pool.as_ref(),
                build.public_inputs.nullifier_hash.as_ref(),
            ],
            &pool_program::ID,
        );

        send_ixs(
            ctx,
            &format!("withdraw: commit_intent[{i}]"),
            std::slice::from_ref(&build.instruction),
            &[&ctx.operator],
        )?;

        triples.push((intent_pda, recipient, relayer));
        nullifier_pdas.push(nullifier_pda);
        payout_pairs.push((recipient, WITHDRAW_DENOMINATION - WITHDRAW_FEE));
        payout_pairs.push((relayer, WITHDRAW_FEE));
        forbidden.push(recipient);
        forbidden.push(relayer);

        if i == 0 {
            probe = Some((build, intent_pda, nullifier_pda));
        }
    }
    ctx.report.phase_timing(
        "withdraw: prove + commit_intent (k = MAX_K_WITHDRAW)",
        prove_start.elapsed(),
    );

    // 3. A4 negative probe NOW, while round0 is still Open with round_id = 0:
    // re-send intent #0's retained instruction verbatim (no re-proving).
    let (probe_build, probe_intent_pda, probe_nullifier_pda) =
        probe.expect("MAX_K_WITHDRAW >= 1, so intent #0 was committed above");
    let intent_before = ctx
        .client
        .get_account(&probe_intent_pda)
        .map_err(|e| SoakError::new(format!("A4 probe: get_account(intent) before: {e}")))?
        .data;
    let nullifier_before = ctx
        .client
        .get_account(&probe_nullifier_pda)
        .map_err(|e| SoakError::new(format!("A4 probe: get_account(nullifier) before: {e}")))?
        .data;
    let probe_send_failed = send_ixs(
        ctx,
        "withdraw: A4 probe (duplicate commit_intent, expected to fail)",
        std::slice::from_ref(&probe_build.instruction),
        &[&ctx.operator],
    )
    .is_err();
    let intent_after = ctx
        .client
        .get_account(&probe_intent_pda)
        .map_err(|e| SoakError::new(format!("A4 probe: get_account(intent) after: {e}")))?
        .data;
    let nullifier_after = ctx
        .client
        .get_account(&probe_nullifier_pda)
        .map_err(|e| SoakError::new(format!("A4 probe: get_account(nullifier) after: {e}")))?
        .data;

    // 4. ALT: the round's infra keys plus every per-intent triple.
    let mut alt_addresses = vec![pool, round0, round1, vault, system_program::ID];
    for (intent, recipient, relayer) in &triples {
        alt_addresses.push(*intent);
        alt_addresses.push(*recipient);
        alt_addresses.push(*relayer);
    }
    let alt = create_and_fill_alt(ctx, &alt_addresses)?;

    // 5. Execute — vault balance snapshotted PRE-execute for A2.
    let vault_pre = ctx
        .client
        .get_balance(&vault)
        .map_err(|e| SoakError::new(format!("withdraw: get_balance(vault) pre-execute: {e}")))?;
    let exec_sig = send_v0(
        ctx,
        "withdraw: execute_round",
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(EXECUTE_CU_LIMIT),
            build_execute_round_ix(pool, vault, ctx.operator.pubkey(), 0, &triples),
        ],
        &alt,
        &[&ctx.operator],
    )?;

    // 6. Assertions A1-A7, all from RPC reads.
    let tx = assertions::fetch_tx(ctx, exec_sig)?;
    assertions::assert_signer_set(ctx, &tx, exec_sig, &[ctx.operator.pubkey()], &forbidden)?;
    assertions::assert_vault_delta(ctx, vault, vault_pre, k as u64, WITHDRAW_DENOMINATION)?;
    assertions::assert_uniform_payouts(ctx, &payout_pairs)?;
    assertions::assert_nullifiers_spent_and_probe(
        ctx,
        &nullifier_pdas,
        probe_send_failed,
        intent_before == intent_after,
        nullifier_before == nullifier_after,
    )?;
    assertions::assert_round_lifecycle(ctx, pool, 0)?;
    assertions::effective_k_section(ctx, k, ctx.operator.pubkey())?;
    assertions::assert_envelope(ctx, &tx, exec_sig)?;

    Ok(())
}
