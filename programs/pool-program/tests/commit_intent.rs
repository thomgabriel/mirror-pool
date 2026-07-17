#![allow(deprecated)]
mod round_support;
use round_support::{build_round_fixture, build_stake_round_fixture, commit_intent_tx, program_id};

use anchor_lang::AccountDeserialize;
use pool_program::round::{Intent, Round, RoundState};
use solana_sdk::{
    account::ReadableAccount,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signature::Signer,
    system_program,
    transaction::{Transaction, TransactionError},
};

#[test]
fn commit_intent_records_intent_and_burns_nullifier() {
    let mut fx = build_round_fixture(2, 1);
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    let tx = commit_intent_tx(&fx, 0, 0);
    fx.svm
        .send_transaction(tx)
        .expect("valid commit must succeed");

    // Intent recorded with the bound payout keys, under round 0.
    let m0_recipient = fx.intents[0].recipient;
    let m0_relayer = fx.intents[0].relayer;
    let intent_acct = fx.svm.get_account(&fx.intents[0].intent_pda).unwrap();
    let intent = Intent::try_deserialize(&mut intent_acct.data()).unwrap();
    assert_eq!(intent.pool, fx.pool);
    assert_eq!(intent.round_id, 0);
    assert_eq!(intent.recipient, m0_recipient);
    assert_eq!(intent.relayer, m0_relayer);

    // Round count incremented; nullifier PDA now exists.
    let round_acct = fx.svm.get_account(&round0).unwrap();
    let round = Round::try_deserialize(&mut round_acct.data()).unwrap();
    assert_eq!(round.state, RoundState::Open);
    assert_eq!(round.intent_count, 1);
    assert!(fx.svm.get_account(&fx.intents[0].nullifier_pda).is_some());
}

#[test]
fn commit_intent_rejects_double_commit() {
    let mut fx = build_round_fixture(2, 1);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();
    fx.svm.expire_blockhash();
    let outcome = fx
        .svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .expect_err("re-committing the same nullifier must fail");
    assert_ne!(outcome.err, TransactionError::AlreadyProcessed);
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|l| l.contains("already in use")),
        "nullifier/intent PDA init must reject the second commit; logs: {:?}",
        outcome.meta.logs
    );
}

#[test]
fn commit_intent_rejects_unknown_root() {
    let mut fx = build_round_fixture(2, 1);
    // Corrupt the root in the tx data (byte at the proof-length offset).
    let m = &fx.intents[0];
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );
    let mut bad_root = m.root;
    bad_root[0] ^= 0x01;
    let mut data = round_support::disc("commit_intent").to_vec();
    data.extend_from_slice(&m.proof.a);
    data.extend_from_slice(&m.proof.b);
    data.extend_from_slice(&m.proof.c);
    data.extend_from_slice(&bad_root);
    data.extend_from_slice(&m.nullifier_hash);
    data.extend_from_slice(&m.fee.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new_readonly(fx.pool, false),
            AccountMeta::new(round0, false),
            AccountMeta::new(m.intent_pda, false),
            AccountMeta::new(m.nullifier_pda, false),
            AccountMeta::new_readonly(m.recipient, false),
            AccountMeta::new_readonly(m.relayer, false),
            AccountMeta::new(fx.payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&fx.payer.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(
            &[&fx.payer],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect_err("unknown root must fail");
    // M4: assert the SPECIFIC guard fired (UnknownRoot), not just "some error" —
    // `UnknownRoot` stays a stable variant (new errors are appended, never
    // reordered — see Global Constraints), so a log-substring check is non-tautological.
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("UnknownRoot")),
        "expected UnknownRoot; logs: {:?}",
        outcome.meta.logs
    );
}

// I4: `fee > denomination` is a reachable value guard on the commit path (it
// fires BEFORE proof verification), and must fail closed. Reuse intent 0's real
// proof but set an out-of-range fee in the instruction data.
#[test]
fn commit_intent_rejects_fee_over_denomination() {
    let mut fx = build_round_fixture(2, 1);
    let m = &fx.intents[0];
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );
    let bad_fee = round_support::DENOMINATION + 1;
    let mut data = round_support::disc("commit_intent").to_vec();
    data.extend_from_slice(&m.proof.a);
    data.extend_from_slice(&m.proof.b);
    data.extend_from_slice(&m.proof.c);
    data.extend_from_slice(&m.root);
    data.extend_from_slice(&m.nullifier_hash);
    data.extend_from_slice(&bad_fee.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new_readonly(fx.pool, false),
            AccountMeta::new(round0, false),
            AccountMeta::new(m.intent_pda, false),
            AccountMeta::new(m.nullifier_pda, false),
            AccountMeta::new_readonly(m.recipient, false),
            AccountMeta::new_readonly(m.relayer, false),
            AccountMeta::new(fx.payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&fx.payer.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(
            &[&fx.payer],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect_err("fee exceeding the denomination must fail closed");
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|l| l.contains("FeeExceedsDenomination")),
        "expected FeeExceedsDenomination; logs: {:?}",
        outcome.meta.logs
    );
}

// Plan 5 Task 1: a stake pool requires every intent's fee to exactly equal the
// pool's configured `stake_fee` (uniform delegation amount — see lib.rs's
// commit_intent doc note on the privacy/liveness rationale). Reuse intent 0's
// real proof (the fee guard fires before proof verification) but set a
// mismatched fee in the instruction data.
#[test]
fn commit_intent_rejects_wrong_stake_fee() {
    let stake_fee = 5_000u64;
    let mut fx = build_stake_round_fixture(2, 1, stake_fee);
    let m = &fx.intents[0];
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );
    let bad_fee = stake_fee + 1;
    let mut data = round_support::disc("commit_intent").to_vec();
    data.extend_from_slice(&m.proof.a);
    data.extend_from_slice(&m.proof.b);
    data.extend_from_slice(&m.proof.c);
    data.extend_from_slice(&m.root);
    data.extend_from_slice(&m.nullifier_hash);
    data.extend_from_slice(&bad_fee.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new_readonly(fx.pool, false),
            AccountMeta::new(round0, false),
            AccountMeta::new(m.intent_pda, false),
            AccountMeta::new(m.nullifier_pda, false),
            AccountMeta::new_readonly(m.recipient, false),
            AccountMeta::new_readonly(m.relayer, false),
            AccountMeta::new(fx.payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&fx.payer.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(
            &[&fx.payer],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect_err("a stake-pool fee that doesn't match pool.stake_fee must fail closed");
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|l| l.contains("WrongActionConfig")),
        "expected WrongActionConfig; logs: {:?}",
        outcome.meta.logs
    );
}

#[test]
fn commit_intent_accepts_matching_stake_fee() {
    let stake_fee = 5_000u64;
    let mut fx = build_stake_round_fixture(2, 1, stake_fee);
    // The fixture sets intents[0].fee == stake_fee, so the plain builder's
    // encoded fee already matches — this must succeed.
    let tx = commit_intent_tx(&fx, 0, 0);
    fx.svm
        .send_transaction(tx)
        .expect("fee == pool.stake_fee must be accepted");
}
