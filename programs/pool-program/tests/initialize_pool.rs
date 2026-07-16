mod common;
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

    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false), // writable: receives rent-exempt funding
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data: {
            let mut d = disc("initialize_pool").to_vec();
            d.extend_from_slice(&1_000_000u64.to_le_bytes());
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
