// `solana_sdk::transaction::TransactionError` is a deprecated re-export (the SDK points at
// the standalone `solana-transaction-error` crate instead), but that crate isn't a direct
// dependency of this test crate. Matching on `TransactionError`/`InstructionError` variants
// below is the whole point of the negative-guard tests, so allow the deprecation warning
// file-wide rather than pull in an extra dependency for a type alias.
#![allow(deprecated)]

mod common;
use common::{cu_limit_ix, disc, program_id, so_path};
use litesvm::LiteSVM;
use pool_program::state::Pool;
use solana_sdk::{
    account::ReadableAccount,
    instruction::{AccountMeta, Instruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::{Transaction, TransactionError},
};

/// Anchor custom program errors start at 6000, assigned in `PoolError` declaration order
/// (see `programs/pool-program/src/lib.rs`): MerkleInit=6000, ZeroDeposit=6001,
/// CommitmentNotInField=6002, TreeFull=6003. Confirmed against `target/idl/pool_program.json`.
const ZERO_DEPOSIT_CODE: u32 = 6001;
const COMMITMENT_NOT_IN_FIELD_CODE: u32 = 6002;

const NEXT_INDEX_OFFSET: usize = 8 + core::mem::offset_of!(Pool, next_index);
const CURRENT_ROOT_OFFSET: usize = 8 + core::mem::offset_of!(Pool, current_root);

fn setup_pool(denomination: u64) -> (LiteSVM, Keypair, Pubkey, Pubkey) {
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

    let mut data = disc("initialize_pool").to_vec();
    data.extend_from_slice(&denomination.to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
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
        data,
    };
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    svm.send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .unwrap();
    (svm, payer, pool, vault)
}

fn deposit_ix(
    pool: Pubkey,
    vault: Pubkey,
    payer: Pubkey,
    commitment: [u8; 32],
    amount: u64,
) -> Instruction {
    let mut data = disc("deposit").to_vec();
    data.extend_from_slice(&commitment);
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    }
}

#[test]
fn deposit_moves_lamports_and_advances_tree() {
    let (mut svm, payer, pool, vault) = setup_pool(1_000_000);

    let root_before = svm.get_account(&pool).unwrap().data()
        [CURRENT_ROOT_OFFSET..CURRENT_ROOT_OFFSET + 32]
        .to_vec();
    let vault_before = svm.get_account(&vault).map(|a| a.lamports()).unwrap_or(0);

    let commitment = {
        let mut c = [0u8; 32];
        c[31] = 42;
        c
    };
    let ix = deposit_ix(pool, vault, payer.pubkey(), commitment, 1_000_000);
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    let meta = svm
        .send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .unwrap();
    println!("deposit CU consumed: {}", meta.compute_units_consumed);

    let vault_after = svm.get_account(&vault).unwrap().lamports();
    assert_eq!(
        vault_after - vault_before,
        1_000_000,
        "vault received the deposit"
    );

    let data_after = svm.get_account(&pool).unwrap().data().to_vec();
    assert_ne!(
        &data_after[CURRENT_ROOT_OFFSET..CURRENT_ROOT_OFFSET + 32],
        root_before.as_slice(),
        "root advanced after deposit"
    );
    let next_index = u32::from_le_bytes(
        data_after[NEXT_INDEX_OFFSET..NEXT_INDEX_OFFSET + 4]
            .try_into()
            .unwrap(),
    );
    assert_eq!(next_index, 1, "one leaf inserted");
}

#[test]
fn deposit_rejects_zero_amount() {
    let (mut svm, payer, pool, vault) = setup_pool(1_000_000);
    let commitment = {
        let mut c = [0u8; 32];
        c[31] = 7;
        c
    };
    let ix = deposit_ix(pool, vault, payer.pubkey(), commitment, 0);
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    let outcome = svm
        .send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .expect_err("zero deposit must fail");

    // A bare `.is_err()` would also pass for an unrelated failure (bad accounts, missing
    // signer, CU exhaustion...). Assert the specific guard fired: an `InstructionError`
    // carrying the `ZeroDeposit` code, with the matching message in the program logs.
    assert!(
        matches!(
            outcome.err,
            TransactionError::InstructionError(_, InstructionError::Custom(code))
                if code == ZERO_DEPOSIT_CODE
        ),
        "expected InstructionError::Custom({ZERO_DEPOSIT_CODE}) (ZeroDeposit), got {:?} (logs: {:?})",
        outcome.err,
        outcome.meta.logs
    );
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|log| log.contains("greater than zero")),
        "expected the ZeroDeposit error message in logs; logs: {:?}",
        outcome.meta.logs
    );
}

#[test]
fn deposit_rejects_out_of_field_commitment() {
    let (mut svm, payer, pool, vault) = setup_pool(1_000_000);
    // Larger than the BN254 scalar field modulus in every leading byte, so it fails the
    // `is_in_field` range check regardless of the exact modulus value.
    let commitment = [0xffu8; 32];
    let ix = deposit_ix(pool, vault, payer.pubkey(), commitment, 1_000_000);
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    let outcome = svm
        .send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .expect_err("out-of-field commitment must fail");

    // Exercises the instruction-level `require!(is_in_field(...))` wiring, not just the
    // pure-fn host test — same non-tautological assertion style as the zero-amount guard.
    assert!(
        matches!(
            outcome.err,
            TransactionError::InstructionError(_, InstructionError::Custom(code))
                if code == COMMITMENT_NOT_IN_FIELD_CODE
        ),
        "expected InstructionError::Custom({COMMITMENT_NOT_IN_FIELD_CODE}) (CommitmentNotInField), got {:?} (logs: {:?})",
        outcome.err,
        outcome.meta.logs
    );
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|log| log.contains("not a valid field element")),
        "expected the CommitmentNotInField error message in logs; logs: {:?}",
        outcome.meta.logs
    );
}
