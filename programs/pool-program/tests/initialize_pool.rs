mod common;
use anchor_lang::AccountDeserialize;
use common::{cu_limit_ix, disc, program_id, so_path};
use litesvm::LiteSVM;
use pool_program::state::Pool;
use solana_sdk::{
    account::ReadableAccount,
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::Transaction,
};

#[test]
fn initialize_pool_creates_account_with_nonzero_root() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false), // writable: receives rent-exempt funding
            AccountMeta::new(round, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: {
            let mut d = disc("initialize_pool").to_vec();
            d.extend_from_slice(&1_000_000u64.to_le_bytes());
            d.extend_from_slice(&2u16.to_le_bytes());
            d.push(0u8);
            d.extend_from_slice(&Pubkey::default().to_bytes());
            d.extend_from_slice(&0u64.to_le_bytes());
            d
        },
    };
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    let tx = Transaction::new(&[&payer], msg, svm.latest_blockhash());
    let meta = svm.send_transaction(tx).unwrap();
    println!(
        "initialize_pool CU consumed: {}",
        meta.compute_units_consumed
    );

    let acct = svm.get_account(&pool).unwrap();
    assert!(acct.data().len() > 8, "pool account allocated");
    // `Pool` is `#[account(zero_copy)]` (repr(C) with an explicit alignment-padding
    // field): compute the real byte offset rather than hardcoding it, so this test
    // can't silently drift from the account's actual layout.
    let offset = 8 + core::mem::offset_of!(Pool, current_root);
    let current_root = &acct.data()[offset..offset + 32];
    assert_ne!(current_root, &[0u8; 32], "empty-tree root must be nonzero");
}

#[test]
fn initialize_pool_opens_round_zero() {
    use pool_program::round::{Round, RoundState};
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(round, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: {
            let mut d = disc("initialize_pool").to_vec();
            d.extend_from_slice(&1_000_000u64.to_le_bytes());
            d.extend_from_slice(&2u16.to_le_bytes());
            d.push(0u8);
            d.extend_from_slice(&Pubkey::default().to_bytes());
            d.extend_from_slice(&0u64.to_le_bytes());
            d
        },
    };
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    svm.send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .unwrap();

    let acct = svm.get_account(&round).unwrap();
    let parsed = Round::try_deserialize(&mut acct.data()).unwrap();
    assert_eq!(parsed.state, RoundState::Open, "round 0 opens");
    assert_eq!(parsed.intent_count, 0, "round 0 starts empty");
}

#[test]
fn initialize_pool_rejects_k_floor_below_min() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(round, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: {
            let mut d = disc("initialize_pool").to_vec();
            d.extend_from_slice(&1_000_000u64.to_le_bytes());
            d.extend_from_slice(&1u16.to_le_bytes());
            d.push(0u8);
            d.extend_from_slice(&Pubkey::default().to_bytes());
            d.extend_from_slice(&0u64.to_le_bytes());
            d
        },
    };
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    let outcome = svm
        .send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .expect_err("k_floor below MIN_K_FLOOR must be rejected");
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("KFloorTooLow")),
        "expected KFloorTooLow; logs: {:?}",
        outcome.meta.logs
    );
}

/// Same rent sysvar the on-chain `Rent::get()` reads inside `initialize_pool`'s
/// stake-config validation, computed independently so these tests never
/// hardcode a value that could drift from LiteSVM's actual rent parameters.
fn stake_account_rent() -> u64 {
    solana_sdk::rent::Rent::default().minimum_balance(pool_program::invariants::STAKE_ACCOUNT_SIZE)
}

#[allow(clippy::too_many_arguments)]
fn initialize_pool_ix(
    pool: Pubkey,
    vault: Pubkey,
    round: Pubkey,
    mint: Pubkey,
    payer: Pubkey,
    denomination: u64,
    k_floor: u16,
    action_kind: u8,
    validator: Pubkey,
    fee: u64,
) -> Instruction {
    let mut d = disc("initialize_pool").to_vec();
    d.extend_from_slice(&denomination.to_le_bytes());
    d.extend_from_slice(&k_floor.to_le_bytes());
    d.push(action_kind);
    d.extend_from_slice(&validator.to_bytes());
    d.extend_from_slice(&fee.to_le_bytes());
    Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(round, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: d,
    }
}

#[test]
fn initialize_stake_pool_stores_config() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    let validator = Pubkey::new_unique();
    let stake_fee = 5_000u64;
    let rent = stake_account_rent();
    // denomination clears fee + rent + MIN_STAKE_DELEGATION with slack to spare.
    let denomination =
        pool_program::invariants::MIN_STAKE_DELEGATION + rent + stake_fee + 1_000_000;

    let ix = initialize_pool_ix(
        pool,
        vault,
        round,
        mint,
        payer.pubkey(),
        denomination,
        2,
        1,
        validator,
        stake_fee,
    );
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    svm.send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .expect("valid stake-pool config must succeed");

    let acct = svm.get_account(&pool).unwrap();
    let data = acct.data();

    let action_kind_offset = 8 + core::mem::offset_of!(Pool, action_kind);
    assert_eq!(data[action_kind_offset], 1, "action_kind == Stake");

    let validator_offset = 8 + core::mem::offset_of!(Pool, validator);
    assert_eq!(
        &data[validator_offset..validator_offset + 32],
        validator.as_ref(),
        "validator stored"
    );

    let fee_offset = 8 + core::mem::offset_of!(Pool, fee);
    let stored_fee = u64::from_le_bytes(data[fee_offset..fee_offset + 8].try_into().unwrap());
    assert_eq!(stored_fee, stake_fee, "fee stored");
}

#[test]
fn initialize_stake_pool_rejects_below_delegation_floor() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    let validator = Pubkey::new_unique();
    let stake_fee = 5_000u64;
    let rent = stake_account_rent();
    // One lamport short of the floor: delegated == MIN_STAKE_DELEGATION - 1.
    let denomination = pool_program::invariants::MIN_STAKE_DELEGATION - 1 + rent + stake_fee;

    let ix = initialize_pool_ix(
        pool,
        vault,
        round,
        mint,
        payer.pubkey(),
        denomination,
        2,
        1,
        validator,
        stake_fee,
    );
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    let outcome = svm
        .send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .expect_err("denomination below the delegation floor must be rejected");
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|l| l.contains("StakeDenominationTooLow")),
        "expected StakeDenominationTooLow; logs: {:?}",
        outcome.meta.logs
    );
}

#[test]
fn initialize_withdraw_pool_rejects_stake_params() {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    // action_kind = 0 (Withdraw) but a nonzero validator — invalid config.
    let ix = initialize_pool_ix(
        pool,
        vault,
        round,
        mint,
        payer.pubkey(),
        1_000_000,
        2,
        0,
        Pubkey::new_unique(),
        0,
    );
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    let outcome = svm
        .send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .expect_err("withdraw pool with stake params must be rejected");
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
