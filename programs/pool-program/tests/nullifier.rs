mod common;
use common::{cu_limit_ix, disc, program_id, so_path};
use litesvm::LiteSVM;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::Transaction,
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
    assert!(
        svm.send_transaction(mark_spent_tx(&svm, &payer, pool, nh))
            .is_err(),
        "re-spending the same nullifier must fail (PDA already exists)"
    );
}
