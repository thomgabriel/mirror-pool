#![allow(deprecated)]
mod round_support;
use round_support::{
    build_stake_round_fixture, build_stake_round_fixture_signer_recipients, commit_intent_tx,
    program_id, stake_pool_denomination,
};

use anchor_lang::AccountDeserialize;
use pool_program::round::{Round, RoundState};
use solana_sdk::stake::state::StakeStateV2;
use solana_sdk::{
    account::ReadableAccount,
    clock::Clock,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    stake, system_program, sysvar,
    transaction::{Transaction, TransactionError},
};

/// Stake PDA seed = the INTENT PDA key (itself `["intent", pool, nullifier_hash]`),
/// NOT `nullifier_hash` directly — see lib.rs's stake dispatch arm.
fn stake_pda(pool: Pubkey, intent_pda: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"stake", pool.as_ref(), intent_pda.as_ref()],
        &program_id(),
    )
    .0
}

fn triples_for(fx: &round_support::RoundFixture) -> Vec<(Pubkey, Pubkey, Pubkey)> {
    fx.intents
        .iter()
        .map(|m| (m.intent_pda, stake_pda(fx.pool, m.intent_pda), m.relayer))
        .collect()
}

fn execute_round_stake_ix(
    fx: &round_support::RoundFixture,
    round_id: u64,
    cranker: Pubkey,
    triples: &[(Pubkey, Pubkey, Pubkey)], // (intent, stake_account, relayer)
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
    for (intent, stake_account, relayer) in triples {
        accounts.push(AccountMeta::new(*intent, false));
        accounts.push(AccountMeta::new(*stake_account, false));
        accounts.push(AccountMeta::new(*relayer, false));
    }
    // Shared tail: validator, stake_program, stake_config, clock, stake_history, rent.
    accounts.push(AccountMeta::new_readonly(fx.validator, false));
    accounts.push(AccountMeta::new_readonly(stake::program::ID, false));
    accounts.push(AccountMeta::new_readonly(stake::config::ID, false));
    accounts.push(AccountMeta::new_readonly(sysvar::clock::ID, false));
    accounts.push(AccountMeta::new_readonly(sysvar::stake_history::ID, false));
    accounts.push(AccountMeta::new_readonly(sysvar::rent::ID, false));
    let mut data = round_support::disc("execute_round").to_vec();
    data.extend_from_slice(&round_id.to_le_bytes());
    Instruction {
        program_id: program_id(),
        accounts,
        data,
    }
}

/// Deserialize the real bincode-encoded `StakeStateV2` at `pda`, asserting it
/// is Stake-program-owned (native stake accounts are bincode, NOT borsh).
fn stake_state_at(fx: &round_support::RoundFixture, pda: Pubkey) -> StakeStateV2 {
    let acct = fx.svm.get_account(&pda).unwrap();
    assert_eq!(
        acct.owner,
        stake::program::ID,
        "stake account {pda} owned by Stake program"
    );
    bincode::deserialize(&acct.data).unwrap()
}

#[test]
fn execute_round_stakes_the_batch_uniformly() {
    let stake_fee = 5_000u64;
    let mut fx = build_stake_round_fixture(2, 2, stake_fee);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 1, 0))
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();

    let triples = triples_for(&fx);
    let vault_before = fx.svm.get_account(&fx.vault).unwrap().lamports();

    fx.svm.expire_blockhash();
    let ix = execute_round_stake_ix(&fx, 0, cranker.pubkey(), &triples);
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
        .expect("a full k-round stake execution must succeed");
    println!(
        "execute_round (stake, k=2) CU consumed: {}",
        meta.compute_units_consumed
    );

    let mut delegated_amounts = Vec::new();
    for m in &fx.intents {
        let pda = stake_pda(fx.pool, m.intent_pda);
        match stake_state_at(&fx, pda) {
            StakeStateV2::Stake(meta, stake, _) => {
                assert_eq!(
                    stake.delegation.voter_pubkey, fx.validator,
                    "delegated to the pool's validator"
                );
                assert_eq!(
                    meta.authorized.staker, m.recipient,
                    "staker authority handed to the recipient post-Authorize"
                );
                assert_eq!(
                    meta.authorized.withdrawer, m.recipient,
                    "withdrawer authority is the recipient"
                );
                delegated_amounts.push(stake.delegation.stake);
            }
            other => panic!("expected StakeStateV2::Stake, got {other:?}"),
        }
    }
    assert!(
        delegated_amounts.windows(2).all(|w| w[0] == w[1]),
        "delegations must be identical across the round: {delegated_amounts:?}"
    );

    let vault_after = fx.svm.get_account(&fx.vault).unwrap().lamports();
    let denomination = stake_pool_denomination(stake_fee);
    assert_eq!(
        vault_before - vault_after,
        denomination * fx.intents.len() as u64,
        "vault debited exactly k * denomination (value conserved)"
    );

    // Round closed; next round opened (mirrors execute_round.rs's withdraw assertion).
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

    // Re-executing the same (now Executed) round must fail: `next_round`
    // (round 1) already exists, so its `init` fails "already in use".
    fx.svm.expire_blockhash();
    let ix = execute_round_stake_ix(&fx, 0, cranker.pubkey(), &triples);
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
        .expect_err("re-executing an Executed stake round must fail");
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
fn execute_round_stake_rejects_sub_k() {
    let stake_fee = 5_000u64;
    let mut fx = build_stake_round_fixture(2, 2, stake_fee);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    let triples = vec![(
        fx.intents[0].intent_pda,
        stake_pda(fx.pool, fx.intents[0].intent_pda),
        fx.intents[0].relayer,
    )];

    fx.svm.expire_blockhash();
    let ix = execute_round_stake_ix(&fx, 0, cranker.pubkey(), &triples);
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
        .expect_err("a sub-k stake round must not fire");
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("KFloorNotMet")),
        "expected KFloorNotMet; logs: {:?}",
        outcome.meta.logs
    );
}

// The substituted-relayer surface: recipient is CPI data (not a passed
// account), so the only fund-redirection surfaces left are the relayer and
// the stake account (covered separately below).
#[test]
fn execute_round_stake_rejects_substituted_relayer() {
    let stake_fee = 5_000u64;
    let mut fx = build_stake_round_fixture(2, 2, stake_fee);
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
        (
            fx.intents[0].intent_pda,
            stake_pda(fx.pool, fx.intents[0].intent_pda),
            attacker, // substituted relayer, != intent.relayer
        ),
        (
            fx.intents[1].intent_pda,
            stake_pda(fx.pool, fx.intents[1].intent_pda),
            fx.intents[1].relayer,
        ),
    ];
    fx.svm.expire_blockhash();
    let ix = execute_round_stake_ix(&fx, 0, cranker.pubkey(), &triples);
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
        .expect_err("a substituted relayer must be rejected");
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

#[test]
fn execute_round_stake_rejects_wrong_stake_pda() {
    let stake_fee = 5_000u64;
    let mut fx = build_stake_round_fixture(2, 2, stake_fee);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 1, 0))
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();

    // NOT ["stake", pool, intent_pda] for intent 0 — an arbitrary address.
    let bogus_stake_account = Pubkey::new_unique();
    let triples = vec![
        (
            fx.intents[0].intent_pda,
            bogus_stake_account,
            fx.intents[0].relayer,
        ),
        (
            fx.intents[1].intent_pda,
            stake_pda(fx.pool, fx.intents[1].intent_pda),
            fx.intents[1].relayer,
        ),
    ];
    fx.svm.expire_blockhash();
    let ix = execute_round_stake_ix(&fx, 0, cranker.pubkey(), &triples);
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
        .expect_err("a wrong stake-account PDA must be rejected");
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|l| l.contains("StakeAccountInvalid")),
        "expected StakeAccountInvalid; logs: {:?}",
        outcome.meta.logs
    );
}

#[test]
fn execute_round_stake_rejects_intent_from_another_pool() {
    use anchor_lang::AccountSerialize;
    use pool_program::round::{ActionKind, Intent};
    use solana_sdk::account::Account;

    let stake_fee = 5_000u64;
    let mut fx = build_stake_round_fixture(2, 2, stake_fee);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 1, 0))
        .unwrap();

    // A program-owned Intent (correct discriminator) bound to a DIFFERENT pool.
    let foreign = Intent {
        pool: Pubkey::new_unique(), // NOT fx.pool
        round_id: 0,
        recipient: Pubkey::new_unique(),
        relayer: Pubkey::new_unique(),
        fee: stake_fee,
        action: ActionKind::Stake,
        committed_slot: 0,
    };
    let mut data = Vec::new();
    foreign.try_serialize(&mut data).unwrap();
    let foreign_addr = Pubkey::new_unique();
    fx.svm
        .set_account(
            foreign_addr,
            Account {
                lamports: 10_000_000,
                data,
                owner: program_id(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    // Replace intent 1 with the foreign-pool intent; its stake account / relayer
    // slots are unreachable (the pool-binding check fails first), so any
    // well-formed placeholders suffice.
    let triples = vec![
        (
            fx.intents[0].intent_pda,
            stake_pda(fx.pool, fx.intents[0].intent_pda),
            fx.intents[0].relayer,
        ),
        (foreign_addr, Pubkey::new_unique(), Pubkey::new_unique()),
    ];
    fx.svm.expire_blockhash();
    let ix = execute_round_stake_ix(&fx, 0, cranker.pubkey(), &triples);
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

// `commit_intent` enforces `fee == pool.fee` at commit time, so a
// normal flow can never produce a mismatched fee — this crafts an `Intent`
// account directly (bypassing `commit_intent`) to exercise `execute_round`'s
// defense-in-depth re-check, the same way `..._rejects_intent_from_another_pool`
// crafts a foreign-pool intent above.
#[test]
fn execute_round_stake_rejects_wrong_fee() {
    use anchor_lang::AccountSerialize;
    use pool_program::round::{ActionKind, Intent};
    use solana_sdk::account::Account;

    let stake_fee = 5_000u64;
    let mut fx = build_stake_round_fixture(2, 2, stake_fee);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 1, 0))
        .unwrap();

    // A program-owned Intent (correct discriminator), correctly bound to THIS
    // pool/round, but with `fee != pool.fee`.
    let wrong_fee = Intent {
        pool: fx.pool,
        round_id: 0,
        recipient: Pubkey::new_unique(),
        relayer: Pubkey::new_unique(),
        fee: stake_fee + 1, // NOT pool.fee
        action: ActionKind::Stake,
        committed_slot: 0,
    };
    let mut data = Vec::new();
    wrong_fee.try_serialize(&mut data).unwrap();
    let wrong_fee_addr = Pubkey::new_unique();
    fx.svm
        .set_account(
            wrong_fee_addr,
            Account {
                lamports: 10_000_000,
                data,
                owner: program_id(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    // Replace intent 1 with the wrong-fee intent; its stake account / relayer
    // slots are unreachable (the fee check fires before either is read), so
    // any well-formed placeholders suffice.
    let triples = vec![
        (
            fx.intents[0].intent_pda,
            stake_pda(fx.pool, fx.intents[0].intent_pda),
            fx.intents[0].relayer,
        ),
        (wrong_fee_addr, Pubkey::new_unique(), Pubkey::new_unique()),
    ];
    fx.svm.expire_blockhash();
    let ix = execute_round_stake_ix(&fx, 0, cranker.pubkey(), &triples);
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
        .expect_err("an intent with fee != pool.fee must be rejected");
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|l| l.contains("FeeNotUniform")),
        "expected FeeNotUniform; logs: {:?}",
        outcome.meta.logs
    );
}

#[test]
fn execute_round_stake_rejects_duplicate_intent() {
    let stake_fee = 5_000u64;
    let mut fx = build_stake_round_fixture(2, 2, stake_fee);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 1, 0))
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    let dup = (
        fx.intents[0].intent_pda,
        stake_pda(fx.pool, fx.intents[0].intent_pda),
        fx.intents[0].relayer,
    );
    let triples = vec![dup, dup];

    fx.svm.expire_blockhash();
    let ix = execute_round_stake_ix(&fx, 0, cranker.pubkey(), &triples);
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

// C2 (uniformity, both directions): a pre-funded stake PDA must still land at
// EXACTLY `to_stake` before delegation, whichever direction the pre-fund came
// from. Dust one intent's PDA BELOW `to_stake` (vault tops up — the case that
// would also pass with a missing top-up if the assertion were weak) and
// pre-fund another ABOVE `to_stake` (excess must be swept back to the vault —
// the case that WOULD fail if the excess-sweep were missing/buggy).
#[test]
fn execute_round_stake_prefund_uniformity_both_directions() {
    let stake_fee = 5_000u64;
    let mut fx = build_stake_round_fixture(2, 2, stake_fee);

    let to_stake = stake_pool_denomination(stake_fee) - stake_fee;
    let dusted_pda = stake_pda(fx.pool, fx.intents[0].intent_pda);
    let overfunded_pda = stake_pda(fx.pool, fx.intents[1].intent_pda);
    fx.svm.airdrop(&dusted_pda, 1).unwrap();
    fx.svm
        .airdrop(&overfunded_pda, to_stake + 500_000_000)
        .unwrap();

    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 1, 0))
        .unwrap();

    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    let triples = triples_for(&fx);

    fx.svm.expire_blockhash();
    let ix = execute_round_stake_ix(&fx, 0, cranker.pubkey(), &triples);
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
        .expect("a round with both under- and over-funded stake PDAs must still complete");

    let mut delegated_amounts = Vec::new();
    for m in &fx.intents {
        let pda = stake_pda(fx.pool, m.intent_pda);
        match stake_state_at(&fx, pda) {
            StakeStateV2::Stake(_, stake, _) => delegated_amounts.push(stake.delegation.stake),
            other => panic!("expected StakeStateV2::Stake, got {other:?}"),
        }
    }
    assert!(
        delegated_amounts.windows(2).all(|w| w[0] == w[1]),
        "delegations must stay identical despite pre-funding in both directions: {delegated_amounts:?}"
    );
}

// `cancel_intent` is generic across action kinds: it never touches the stake
// account (that's only created at execute), so a stake-pool intent cancels
// exactly like a withdraw-pool intent — refund, decremented count, closed
// intent PDA, nullifier stays burned.
#[test]
fn cancel_intent_works_on_stake_pool() {
    let stake_fee = 5_000u64;
    let (mut fx, recipients) = build_stake_round_fixture_signer_recipients(2, 1, stake_fee);
    fx.svm
        .send_transaction(commit_intent_tx(&fx, 0, 0))
        .unwrap();

    let committed = fx.svm.get_sysvar::<Clock>().slot;
    fx.svm
        .warp_to_slot(committed + pool_program::invariants::TIMEOUT_SLOTS);

    let recipient = &recipients[0];
    let before = fx
        .svm
        .get_account(&recipient.pubkey())
        .map(|a| a.lamports())
        .unwrap_or(0);

    let round_id = 0u64;
    let (round, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &round_id.to_le_bytes()],
        &program_id(),
    );
    let mut data = round_support::disc("cancel_intent").to_vec();
    data.extend_from_slice(&round_id.to_le_bytes());
    data.extend_from_slice(&fx.intents[0].nullifier_hash);
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new_readonly(fx.pool, false),
            AccountMeta::new(round, false),
            AccountMeta::new(fx.intents[0].intent_pda, false),
            AccountMeta::new(fx.vault, false),
            AccountMeta::new(recipient.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    fx.svm.expire_blockhash();
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
        .expect("recipient may cancel an open-round stake intent");

    // Refunded at least the denomination (+ closed intent rent).
    let denomination = stake_pool_denomination(stake_fee);
    let after = fx.svm.get_account(&recipient.pubkey()).unwrap().lamports();
    assert!(
        after >= before + denomination,
        "recipient refunded at least the denomination"
    );

    // Intent PDA closed (LiteSVM keeps a zero-lamport, system-owned, empty-data
    // tombstone rather than purging it — see cancel_intent.rs's identical note).
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

    let r0 = Round::try_deserialize(&mut fx.svm.get_account(&round).unwrap().data()).unwrap();
    assert_eq!(r0.intent_count, 0, "intent_count decremented");

    // No stake account was ever created — cancel is pre-execute.
    let pda = stake_pda(fx.pool, fx.intents[0].intent_pda);
    assert!(
        fx.svm.get_account(&pda).is_none(),
        "cancel never creates a stake account"
    );

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
