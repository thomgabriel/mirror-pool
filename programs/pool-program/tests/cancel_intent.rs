#![allow(deprecated)]
mod round_support;
use round_support::{
    build_round_fixture_signer_recipients, commit_intent_tx, program_id, DENOMINATION,
};

use anchor_lang::AccountDeserialize;
use pool_program::round::Round;
use solana_sdk::{
    account::ReadableAccount,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::{Transaction, TransactionError},
};

fn cancel_ix(
    fx: &round_support::RoundFixture,
    i: usize,
    round_id: u64,
    recipient: Pubkey,
) -> Instruction {
    let m = &fx.intents[i];
    let (round, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &round_id.to_le_bytes()],
        &program_id(),
    );
    let mut data = round_support::disc("cancel_intent").to_vec();
    data.extend_from_slice(&round_id.to_le_bytes());
    data.extend_from_slice(&m.nullifier_hash);
    Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new_readonly(fx.pool, false),
            AccountMeta::new(round, false),
            AccountMeta::new(m.intent_pda, false),
            AccountMeta::new(fx.vault, false),
            AccountMeta::new(recipient, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    }
}

#[test]
fn cancel_intent_refunds_and_decrements() {
    let (mut fx, recipients) = build_round_fixture_signer_recipients(2, 1);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();

    let recipient = &recipients[0];
    let before = fx
        .svm
        .get_account(&recipient.pubkey())
        .map(|a| a.lamports())
        .unwrap_or(0);

    fx.svm.expire_blockhash();
    let ix = cancel_ix(&fx, 0, 0, recipient.pubkey());
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&fx.payer.pubkey()),
    );
    fx.svm
        .send_transaction(Transaction::new(
            &[&fx.payer, recipient],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect("recipient may cancel an open-round intent");

    // Refunded denomination (+ closed intent rent), intent PDA gone, count back to 0.
    let after = fx.svm.get_account(&recipient.pubkey()).unwrap().lamports();
    assert!(
        after >= before + DENOMINATION,
        "recipient refunded at least the denomination"
    );
    // LiteSVM 0.6.1 never purges zero-lamport accounts from its in-memory
    // store, so a closed PDA still shows up as `Some` here (unlike a real
    // validator, which drops it). The on-chain signal for "closed" is
    // zero lamports + reassigned to the system program + zeroed data.
    match fx.svm.get_account(&fx.intents[0].intent_pda) {
        None => {}
        Some(acc) => {
            assert_eq!(acc.lamports(), 0, "intent PDA drained");
            assert_eq!(
                acc.owner(),
                &system_program::ID,
                "intent PDA reassigned to system program"
            );
            assert!(acc.data().is_empty(), "intent PDA data cleared");
        }
    }

    let (round0, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );
    let r0 = Round::try_deserialize(&mut fx.svm.get_account(&round0).unwrap().data()).unwrap();
    assert_eq!(r0.intent_count, 0, "intent_count decremented");

    // Nullifier stays burned: re-committing the same note must fail.
    fx.svm.expire_blockhash();
    let outcome = fx
        .svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .expect_err("cancelled note stays spent (nullifier not returned)");
    assert!(outcome
        .meta
        .logs
        .iter()
        .any(|l| l.contains("already in use")));
}

#[test]
fn cancel_intent_rejects_wrong_signer() {
    let (mut fx, _recipients) = build_round_fixture_signer_recipients(2, 1);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();

    // An attacker who does NOT control the bound recipient cannot cancel.
    let attacker = Keypair::new();
    fx.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    fx.svm.expire_blockhash();
    let ix = cancel_ix(&fx, 0, 0, attacker.pubkey());
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&attacker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(
            &[&attacker],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect_err("only the bound recipient may cancel");
    assert!(matches!(
        outcome.err,
        TransactionError::InstructionError(_, InstructionError::Custom(_))
    ));
}
