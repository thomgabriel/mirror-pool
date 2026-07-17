#![allow(deprecated)]
mod round_support;
use round_support::{build_round_fixture, commit_intent_tx, program_id, DENOMINATION, FEE};

use anchor_lang::AccountDeserialize;
use pool_program::round::{Round, RoundState};
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

fn execute_round_ix(
    fx: &round_support::RoundFixture,
    round_id: u64,
    cranker: Pubkey,
    intent_triples: &[(Pubkey, Pubkey, Pubkey)], // (intent, recipient, relayer)
) -> Instruction {
    let (round, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &round_id.to_le_bytes()],
        &program_id(),
    );
    let (next_round, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &(round_id + 1).to_le_bytes()],
        &program_id(),
    );
    let mut accounts = vec![
        AccountMeta::new(fx.pool, false),
        AccountMeta::new(round, false),
        AccountMeta::new(next_round, false),
        AccountMeta::new(fx.vault, false),
        AccountMeta::new(cranker, true),
        AccountMeta::new_readonly(system_program::ID, false),
    ];
    for (intent, recipient, relayer) in intent_triples {
        accounts.push(AccountMeta::new(*intent, false));
        accounts.push(AccountMeta::new(*recipient, false));
        accounts.push(AccountMeta::new(*relayer, false));
    }
    let mut data = round_support::disc("execute_round").to_vec();
    data.extend_from_slice(&round_id.to_le_bytes());
    Instruction {
        program_id: program_id(),
        accounts,
        data,
    }
}

#[test]
fn execute_round_pays_the_batch_and_enforces_k_floor() {
    // k_floor = 2, two committed intents.
    let mut fx = build_round_fixture(2, 2);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 1, 0))
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();

    let triples: Vec<(Pubkey, Pubkey, Pubkey)> = fx
        .intents
        .iter()
        .map(|m| (m.intent_pda, m.recipient, m.relayer))
        .collect();

    let vault_before = fx.svm.get_account(&fx.vault).unwrap().lamports();
    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&cranker.pubkey()),
    );
    let meta = fx
        .svm
        .send_transaction(Transaction::new(
            &[&cranker],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect("a full k-round must execute");
    println!("execute_round CU consumed: {}", meta.compute_units_consumed);

    // Every recipient/relayer paid; vault debited exactly k * denomination.
    for m in &fx.intents {
        assert_eq!(
            fx.svm.get_account(&m.recipient).unwrap().lamports(),
            DENOMINATION - FEE,
            "recipient paid denomination - fee"
        );
        assert_eq!(
            fx.svm.get_account(&m.relayer).unwrap().lamports(),
            FEE,
            "relayer paid fee"
        );
    }
    let vault_after = fx.svm.get_account(&fx.vault).unwrap().lamports();
    assert_eq!(
        vault_before - vault_after,
        DENOMINATION * fx.intents.len() as u64,
        "vault debited exactly k * denomination (value conserved)"
    );

    // Round closed; next round opened.
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );
    let (round1, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &1u64.to_le_bytes()],
        &program_id(),
    );
    let r0 = Round::try_deserialize(&mut fx.svm.get_account(&round0).unwrap().data()).unwrap();
    let r1 = Round::try_deserialize(&mut fx.svm.get_account(&round1).unwrap().data()).unwrap();
    assert_eq!(r0.state, RoundState::Executed);
    assert_eq!(r1.state, RoundState::Open);
    assert_eq!(r1.intent_count, 0);

    // Re-executing the same (now Executed) round must fail: NOT because of a
    // `RoundClosed`/`WrongRound` handler check (there is none — see
    // `execute_round`'s doc comment), but because `next_round` (round 1) was
    // already created by the first execution, so its `init` constraint fails
    // "already in use" atomically on the second attempt.
    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&cranker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(
            &[&cranker],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect_err("re-executing an Executed round must fail");
    assert!(matches!(
        outcome.err,
        TransactionError::InstructionError(_, InstructionError::Custom(_))
    ));
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|l| l.contains("already in use")),
        "expected \"already in use\" (next_round's init re-fired); logs: {:?}",
        outcome.meta.logs
    );
}

#[test]
fn execute_round_rejects_sub_k() {
    // k_floor = 2, only one committed intent.
    let mut fx = build_round_fixture(2, 2);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    let triples = vec![(
        fx.intents[0].intent_pda,
        fx.intents[0].recipient,
        fx.intents[0].relayer,
    )];

    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&cranker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(
            &[&cranker],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect_err("a sub-k round must not fire");
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("KFloorNotMet")),
        "expected KFloorNotMet; logs: {:?}",
        outcome.meta.logs
    );
}

#[test]
fn execute_round_rejects_duplicate_padding() {
    // k_floor = 2, ONE real intent duplicated to fake a full round.
    let mut fx = build_round_fixture(2, 2);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    // Force intent_count to 2 by committing a second real intent, then pass the
    // FIRST intent twice (subset padded with a duplicate).
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 1, 0))
        .unwrap();
    let dup = (
        fx.intents[0].intent_pda,
        fx.intents[0].recipient,
        fx.intents[0].relayer,
    );
    let triples = vec![dup, dup];

    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&cranker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(
            &[&cranker],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect_err("a duplicated intent must be rejected");
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|l| l.contains("DuplicateIntent")),
        "expected DuplicateIntent; logs: {:?}",
        outcome.meta.logs
    );
}

// I1 (custody-critical): the fund-redirection guard. Present the CORRECT intent
// PDA but a SUBSTITUTED recipient account — the payout must be refused, proving
// `execute_round` pays only the extDataHash-bound keys stored in the Intent.
#[test]
fn execute_round_rejects_redirected_payout() {
    let mut fx = build_round_fixture(2, 2);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 1, 0))
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();

    let attacker = Pubkey::new_unique();
    let triples = vec![
        // intent 0's real PDA, but the attacker's account swapped in as recipient.
        (fx.intents[0].intent_pda, attacker, fx.intents[0].relayer),
        (
            fx.intents[1].intent_pda,
            fx.intents[1].recipient,
            fx.intents[1].relayer,
        ),
    ];
    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&cranker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(
            &[&cranker],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect_err("a substituted payout account must be rejected");
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|l| l.contains("IntentAccountMismatch")),
        "expected IntentAccountMismatch; logs: {:?}",
        outcome.meta.logs
    );
}

// I2: the cross-pool binding guard `require_keys_eq!(intent.pool, pool_key)`.
// A random pubkey would only trip `Account::try_from` (owner/discriminator), NOT
// this check. To drive it, craft a REAL, program-owned `Intent` (valid
// discriminator) whose `pool` field is some OTHER pool, inject it via
// `set_account`, and present it — only the `intent.pool` check can reject it.
#[test]
fn execute_round_rejects_intent_from_another_pool() {
    use anchor_lang::AccountSerialize;
    use pool_program::round::{ActionKind, Intent};
    use solana_sdk::account::Account;

    let mut fx = build_round_fixture(2, 2);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 1, 0))
        .unwrap();

    // A program-owned Intent that belongs to a DIFFERENT pool but is otherwise
    // well-formed (correct discriminator, round_id matches this round).
    let other_recipient = Pubkey::new_unique();
    let other_relayer = Pubkey::new_unique();
    let foreign = Intent {
        pool: Pubkey::new_unique(), // NOT fx.pool
        round_id: 0,
        recipient: other_recipient,
        relayer: other_relayer,
        fee: FEE,
        action: ActionKind::Withdraw,
        committed_slot: 0,
    };
    let mut data = Vec::new();
    foreign.try_serialize(&mut data).unwrap(); // writes discriminator + fields
    let foreign_addr = Pubkey::new_unique();
    fx.svm
        .set_account(
            foreign_addr,
            Account {
                lamports: 10_000_000,
                data,
                owner: program_id(), // program-owned so try_from's owner check passes
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    // Replace intent 1 with the foreign-pool intent (unique addr, so no dup).
    let triples = vec![
        (
            fx.intents[0].intent_pda,
            fx.intents[0].recipient,
            fx.intents[0].relayer,
        ),
        (foreign_addr, other_recipient, other_relayer),
    ];
    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&cranker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(
            &[&cranker],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect_err("an intent bound to another pool must be rejected");
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|l| l.contains("IntentInvalid")),
        "expected IntentInvalid (intent.pool != pool); logs: {:?}",
        outcome.meta.logs
    );
}

// I4: the `remaining_accounts.len() == intent_count * 3` completeness check.
// count meets the k-floor, but the batch is missing an intent's accounts.
#[test]
fn execute_round_rejects_incomplete_account_set() {
    let mut fx = build_round_fixture(2, 2);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 1, 0))
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    // count == 2 (meets k), but pass only ONE triple → len 3 != 6.
    let triples = vec![(
        fx.intents[0].intent_pda,
        fx.intents[0].recipient,
        fx.intents[0].relayer,
    )];
    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&cranker.pubkey()),
    );
    let outcome = fx
        .svm
        .send_transaction(Transaction::new(
            &[&cranker],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect_err("an incomplete intent-account set must be rejected");
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|l| l.contains("IntentAccountsMismatch")),
        "expected IntentAccountsMismatch; logs: {:?}",
        outcome.meta.logs
    );
}

// The reachable `WrongRound` guard lives on the COMMIT path, not execute_round:
// once round 0 has executed, `current_round_id` is 1, so a late `commit_intent`
// still addressed at round_id 0 is rejected before it ever reaches a closed round.
#[test]
fn commit_to_executed_round_rejects() {
    let mut fx = build_round_fixture(2, 3);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 1, 0))
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    let triples: Vec<(Pubkey, Pubkey, Pubkey)> = fx.intents[..2]
        .iter()
        .map(|m| (m.intent_pda, m.recipient, m.relayer))
        .collect();

    fx.svm.expire_blockhash();
    let ix = execute_round_ix(&fx, 0, cranker.pubkey(), &triples);
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&cranker.pubkey()),
    );
    fx.svm
        .send_transaction(Transaction::new(
            &[&cranker],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect("a full k-round must execute");

    fx.svm.expire_blockhash();
    let outcome = fx
        .svm
        .send_transaction(commit_intent_tx(&fx, 2, 0))
        .expect_err("committing to an executed round (stale round_id) must be rejected");
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("WrongRound")),
        "expected WrongRound; logs: {:?}",
        outcome.meta.logs
    );
}
