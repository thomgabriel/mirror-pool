// `solana_sdk::transaction::TransactionError` is a deprecated re-export (the SDK points at
// the standalone `solana-transaction-error` crate instead), but that crate isn't a direct
// dependency of this test crate. Matching on `TransactionError` variants below is the whole
// point of this test (see `first_mark_succeeds_second_fails`), so allow the deprecation
// warning file-wide rather than pull in an extra dependency for a type alias.
#![allow(deprecated)]

mod common;
use common::{cu_limit_ix, disc, program_id, so_path};
use litesvm::LiteSVM;
use solana_sdk::{
    instruction::{AccountMeta, Instruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::{Transaction, TransactionError},
};

fn setup_pool() -> (LiteSVM, Keypair, Pubkey) {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();
    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: disc("initialize_pool").to_vec(),
    };
    let msg = Message::new(&[cu_limit_ix(), ix], Some(&payer.pubkey()));
    svm.send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .unwrap();
    (svm, payer, pool)
}

fn mark_spent_tx(svm: &LiteSVM, payer: &Keypair, pool: Pubkey, nh: [u8; 32]) -> Transaction {
    let (nullifier, _) =
        Pubkey::find_program_address(&[b"nullifier", pool.as_ref(), nh.as_ref()], &program_id());
    let mut data = disc("mark_spent").to_vec();
    data.extend_from_slice(&nh);
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new_readonly(pool, false),
            AccountMeta::new(nullifier, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    let msg = Message::new(&[ix], Some(&payer.pubkey()));
    Transaction::new(&[payer], msg, svm.latest_blockhash())
}

#[test]
fn first_mark_succeeds_second_fails() {
    let (mut svm, payer, pool) = setup_pool();
    let nh = {
        let mut n = [0u8; 32];
        n[31] = 99;
        n
    };

    svm.send_transaction(mark_spent_tx(&svm, &payer, pool, nh))
        .unwrap();

    // Happy path: the PDA now exists and its `spent` byte (right after the 8-byte Anchor
    // discriminator) is `true`.
    let (nullifier, _) =
        Pubkey::find_program_address(&[b"nullifier", pool.as_ref(), nh.as_ref()], &program_id());
    let account = svm
        .get_account(&nullifier)
        .expect("NullifierRecord PDA must exist after the first mark_spent");
    assert_eq!(
        account.data[8], 1,
        "NullifierRecord.spent must be true after the first mark_spent"
    );

    // The two `mark_spent` transactions must be genuinely distinct so the second one actually
    // reaches program execution instead of being rejected by LiteSVM's signature-history check
    // before dispatch. Expiring the blockhash and building the second tx afterwards gives it a
    // fresh blockhash (and therefore a different signature, since it's signed by the same
    // keypair over otherwise-identical instruction data).
    svm.expire_blockhash();
    let second = mark_spent_tx(&svm, &payer, pool, nh);
    let outcome = svm
        .send_transaction(second)
        .expect_err("re-spending the same nullifier must fail (PDA already exists)");

    // Guard against the tautology this test previously had: a rejection at the signature/history
    // level (`AlreadyProcessed`, dedup of an identical tx) proves nothing about the `init`
    // double-spend guard. The real guard fires *inside* program execution, surfacing as an
    // `InstructionError` — here, the System Program's `Allocate` CPI refusing to touch an
    // account that's already in use (owned by our program, funded from the first mark_spent).
    assert_ne!(
        outcome.err,
        TransactionError::AlreadyProcessed,
        "second tx must be rejected by the init double-spend guard during execution, not by \
         LiteSVM's duplicate-signature check — the test would pass even with a broken guard"
    );
    assert!(
        matches!(
            outcome.err,
            TransactionError::InstructionError(_, InstructionError::Custom(_))
        ),
        "expected an InstructionError from the init double-spend guard, got {:?} (logs: {:?})",
        outcome.err,
        outcome.meta.logs
    );
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|log| log.contains("already in use")),
        "expected the System Program's Allocate 'already in use' guard to fire; logs: {:?}",
        outcome.meta.logs
    );
}
