//! Chain-read assertions A1-A7 (`docs/superpowers/specs/2026-07-20-soak-design.md`
//! §3), shared verbatim by the withdraw and stake rounds. Nothing here trusts
//! client-side bookkeeping: every assertion recomputes its claim from an RPC
//! read and records the evidence into the report, pass or fail. An `Err`
//! return here means the RPC read itself failed — the caller must bubble it
//! so the run fails closed instead of a helper silently leaving the report
//! readable as a pass.

use std::collections::BTreeMap;

use anchor_lang::AccountDeserialize;
use effective_k::{anonymity_report, FunderId, RoundComposition};
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::{account::ReadableAccount, pubkey::Pubkey, signature::Signature};
use solana_transaction_status_client_types::{
    EncodedConfirmedTransactionWithStatusMeta, UiLoadedAddresses, UiTransactionEncoding,
};

use pool_program::round::{Round, RoundState};

use crate::rpc::{Ctx, SoakError, SoakResult};

/// Fetches the landed transaction with full meta at the commitment `ctx` is
/// configured for, encoded so `EncodedTransaction::decode()` can recover the
/// `VersionedTransaction` (A1 needs the signer set; A7 needs the resolved
/// key/CU counts) — one RPC read shared by both.
pub fn fetch_tx(
    ctx: &Ctx,
    sig: Signature,
) -> SoakResult<EncodedConfirmedTransactionWithStatusMeta> {
    ctx.client
        .get_transaction_with_config(
            &sig,
            RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::Base64),
                commitment: Some(ctx.client.commitment()),
                max_supported_transaction_version: Some(0),
            },
        )
        .map_err(|e| SoakError::new(format!("fetch execute tx {sig}: {e}")))
}

/// A1 — the headline: the execute transaction's on-chain signer set is
/// exactly `expected_signers` (the operator/cranker) and contains none of
/// `forbidden` (every recipient/relayer/depositor role).
pub fn assert_signer_set(
    ctx: &Ctx,
    tx: &EncodedConfirmedTransactionWithStatusMeta,
    sig: Signature,
    expected_signers: &[Pubkey],
    forbidden: &[Pubkey],
) -> SoakResult<()> {
    let versioned = tx
        .transaction
        .transaction
        .decode()
        .ok_or_else(|| SoakError::new(format!("A1: could not decode tx {sig}")))?;
    let num_signers = versioned.message.header().num_required_signatures as usize;
    let keys = versioned.message.static_account_keys();
    let signers: Vec<Pubkey> = keys
        .get(..num_signers)
        .ok_or_else(|| {
            SoakError::new(format!(
                "A1: tx {sig} has fewer static keys ({}) than num_required_signatures ({num_signers})",
                keys.len()
            ))
        })?
        .to_vec();

    let matches_expected = signers.len() == expected_signers.len()
        && signers.iter().all(|s| expected_signers.contains(s));
    let no_forbidden = !signers.iter().any(|s| forbidden.contains(s));
    let pass = matches_expected && no_forbidden;

    ctx.report.assertion(
        "A1",
        "execute_round's on-chain signer set is exactly the operator/cranker — no recipient, \
         relayer, or depositor signs",
        pass,
        format!("signers = {signers:?}"),
    );
    Ok(())
}

/// A2 — value conservation: the vault's balance drops by exactly
/// `k * denomination` across the execute (pre-execute vs post-execute).
pub fn assert_vault_delta(
    ctx: &Ctx,
    vault: Pubkey,
    pre: u64,
    k: u64,
    denomination: u64,
) -> SoakResult<()> {
    let post = ctx
        .client
        .get_balance(&vault)
        .map_err(|e| SoakError::new(format!("A2: get_balance(vault {vault}): {e}")))?;
    let expected_delta = k * denomination;
    let actual_delta = pre.saturating_sub(post);
    let pass = pre >= post && actual_delta == expected_delta;

    ctx.report.assertion(
        "A2",
        "vault balance drops by exactly k * denomination across execute",
        pass,
        format!("pre={pre} post={post} delta={actual_delta} expected={expected_delta}"),
    );
    Ok(())
}

/// A3 — byte-uniform settlement: every `(account, expected_balance)` pair
/// matches exactly (fresh keys, so absolute balance == the credit).
pub fn assert_uniform_payouts(ctx: &Ctx, pairs: &[(Pubkey, u64)]) -> SoakResult<()> {
    let mut mismatches = Vec::new();
    let mut groups: BTreeMap<u64, u32> = BTreeMap::new();
    for (key, expected) in pairs {
        let bal = ctx
            .client
            .get_balance(key)
            .map_err(|e| SoakError::new(format!("A3: get_balance({key}): {e}")))?;
        *groups.entry(*expected).or_insert(0) += 1;
        if bal != *expected {
            mismatches.push(format!("{key}: got {bal}, want {expected}"));
        }
    }
    let pass = mismatches.is_empty();
    let evidence = if pass {
        let summary: Vec<String> = groups
            .iter()
            .map(|(amount, n)| format!("{n}x {amount} lamports"))
            .collect();
        format!(
            "{} accounts checked, all match: {}",
            pairs.len(),
            summary.join(", ")
        )
    } else {
        format!("{} mismatches: {}", mismatches.len(), mismatches.join("; "))
    };

    ctx.report.assertion(
        "A3",
        "every recipient/relayer credited exactly its uniform bucketed amount",
        pass,
        evidence,
    );
    Ok(())
}

/// A4 — single-spend: every nullifier PDA exists and is pool-owned, folded
/// with the duplicate-`commit_intent` probe result (fired earlier, while the
/// round was still Open) — the probe must have failed the send AND left the
/// pre-existing intent/nullifier PDAs byte-unchanged.
pub fn assert_nullifiers_spent_and_probe(
    ctx: &Ctx,
    nullifier_pdas: &[Pubkey],
    probe_send_failed: bool,
    probe_intent_unchanged: bool,
    probe_nullifier_unchanged: bool,
) -> SoakResult<()> {
    let mut failures = Vec::new();
    for pda in nullifier_pdas {
        match ctx.client.get_account(pda) {
            Ok(acct) if acct.owner == pool_program::ID => {}
            Ok(acct) => failures.push(format!(
                "{pda}: owned by {} (want {})",
                acct.owner,
                pool_program::ID
            )),
            Err(e) => failures.push(format!("{pda}: get_account failed: {e}")),
        }
    }
    let all_spent = failures.is_empty();
    let probe_pass = probe_send_failed && probe_intent_unchanged && probe_nullifier_unchanged;
    let pass = all_spent && probe_pass;

    let evidence = format!(
        "{}/{} nullifier PDAs present & pool-owned{}; duplicate commit_intent probe: \
         send_failed={probe_send_failed}, intent_pda_unchanged={probe_intent_unchanged}, \
         nullifier_pda_unchanged={probe_nullifier_unchanged}",
        nullifier_pdas.len() - failures.len(),
        nullifier_pdas.len(),
        if failures.is_empty() {
            String::new()
        } else {
            format!(" (failures: {})", failures.join("; "))
        }
    );
    ctx.report.assertion(
        "A4",
        "all k nullifiers spent (single-spend) and the duplicate-commit probe fails without \
         mutating existing PDAs",
        pass,
        evidence,
    );
    Ok(())
}

/// A5 — round lifecycle: the executed round is `Executed`; the next round
/// exists, `Open`, with `intent_count == 0`.
pub fn assert_round_lifecycle(ctx: &Ctx, pool: Pubkey, round_id: u64) -> SoakResult<()> {
    let round_pda = sdk::round_pda(pool, round_id);
    let next_round_pda = sdk::round_pda(pool, round_id + 1);

    let round_acct = ctx
        .client
        .get_account(&round_pda)
        .map_err(|e| SoakError::new(format!("A5: get_account(round {round_id}): {e}")))?;
    let next_acct = ctx
        .client
        .get_account(&next_round_pda)
        .map_err(|e| SoakError::new(format!("A5: get_account(round {}): {e}", round_id + 1)))?;
    let round = Round::try_deserialize(&mut round_acct.data())
        .map_err(|e| SoakError::new(format!("A5: deserialize round {round_id}: {e}")))?;
    let next = Round::try_deserialize(&mut next_acct.data())
        .map_err(|e| SoakError::new(format!("A5: deserialize round {}: {e}", round_id + 1)))?;

    let pass = round.state == RoundState::Executed
        && next.state == RoundState::Open
        && next.intent_count == 0;

    ctx.report.assertion(
        "A5",
        "executed round is Executed; the next round exists, Open, intent_count=0",
        pass,
        format!(
            "round{round_id}.state={:?}; round{}.state={:?} intent_count={}",
            round.state,
            round_id + 1,
            next.state,
            next.intent_count
        ),
    );
    Ok(())
}

/// A6 — the live effective-k report: feeds the run's true funding composition
/// (every note funded by the same solo operator) into `crates/effective-k`
/// and prints its `AnonymityReport` verbatim. This is a MONITORING number,
/// always reported and never gated on — the solo run's collapse to
/// `effective_k = 1.0` is the expected, disclosed maximal-whale case
/// (`crates/effective-k`'s own `one_funder_fills_the_round` case), not a
/// failure of the soak.
pub fn effective_k_section(ctx: &Ctx, k: usize, solo_funder: Pubkey) -> SoakResult<()> {
    let comp = RoundComposition::new(vec![FunderId(solo_funder.to_bytes()); k])
        .map_err(|e| SoakError::new(format!("A6: RoundComposition::new: {e}")))?;
    let report = anonymity_report(&comp);

    let evidence = format!(
        "AnonymityReport {{ nominal_k: {}, effective_k: {}, shannon_effective_k: {}, \
         guessing_advantage: {}, max_funder_share: {} }} — EXPECTED AND DISCLOSED: a solo \
         operator funds every note in this soak, so this is the maximal-whale case and \
         collapses to effective_k=1.0 by construction; a real deployment's effective-k depends \
         on independent funder clustering, which a solo run cannot exercise (see docs/SOAK.md).",
        report.nominal_k,
        report.effective_k,
        report.shannon_effective_k,
        report.guessing_advantage,
        report.max_funder_share
    );
    ctx.report.assertion(
        "A6",
        "live effective-k, computed by crates/effective-k from the run's true funding \
         composition (reported, never gated)",
        true,
        evidence,
    );
    Ok(())
}

/// A7 — envelope facts: the resolved account-key count (static + ALT-loaded)
/// stays within the 64-lock ceiling; compute units consumed is recorded.
pub fn assert_envelope(
    ctx: &Ctx,
    tx: &EncodedConfirmedTransactionWithStatusMeta,
    sig: Signature,
) -> SoakResult<()> {
    let versioned = tx
        .transaction
        .transaction
        .decode()
        .ok_or_else(|| SoakError::new(format!("A7: could not decode tx {sig}")))?;
    let static_count = versioned.message.static_account_keys().len();

    let meta = tx
        .transaction
        .meta
        .as_ref()
        .ok_or_else(|| SoakError::new(format!("A7: tx {sig} missing meta")))?;
    let loaded: Option<UiLoadedAddresses> = meta.loaded_addresses.clone().into();
    let loaded_count = loaded
        .as_ref()
        .map(|l| l.writable.len() + l.readonly.len())
        .unwrap_or(0);
    let resolved = static_count + loaded_count;

    let cu: Option<u64> = meta.compute_units_consumed.clone().into();
    let cu =
        cu.ok_or_else(|| SoakError::new(format!("A7: tx {sig} missing compute_units_consumed")))?;

    let pass = resolved <= 64;
    ctx.report.assertion(
        "A7",
        "resolved account-key count (static + ALT-loaded) <= 64; compute units consumed recorded",
        pass,
        format!(
            "resolved_keys = {static_count} static + {loaded_count} loaded = {resolved} (<=64); \
             compute_units_consumed = {cu}"
        ),
    );
    Ok(())
}
