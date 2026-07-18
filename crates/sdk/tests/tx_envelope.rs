//! The account-lock dimension of MAX_K, guarded programmatically: LiteSVM
//! cannot observe Solana's banking-stage 64-account-lock check, so this test
//! compiles the REAL v0+ALT execute_round transaction at MAX_K — the exact
//! shape a cranker must build — and counts the fully-resolved key set.
//! If a future change adds a per-intent or fixed account, this fails before
//! a production round can become unexecutable.

use pool_program::invariants::{MAX_K_STAKE, MAX_K_WITHDRAW};
use sdk::{build_execute_round_ix, build_execute_stake_round_ix};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::Instruction,
    message::{v0, AddressLookupTableAccount},
    pubkey::Pubkey,
};

/// Static keys + every ALT-loaded address — the set Solana's
/// validate_account_locks counts (locks apply to the fully-resolved set).
fn resolved_key_count(msg: &v0::Message) -> usize {
    msg.account_keys.len()
        + msg
            .address_table_lookups
            .iter()
            .map(|l| l.writable_indexes.len() + l.readonly_indexes.len())
            .sum::<usize>()
}

fn compile_at_max_k(ix: Instruction, per_intent_keys: Vec<Pubkey>, cranker: Pubkey) -> v0::Message {
    let cb = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    let alt = AddressLookupTableAccount {
        key: Pubkey::new_unique(),
        addresses: per_intent_keys,
    };
    v0::Message::try_compile(&cranker, &[cb, ix], &[alt], Hash::default())
        .expect("v0+ALT compile at MAX_K must succeed")
}

#[test]
fn withdraw_execute_round_at_max_k_fits_64_account_locks() {
    let (pool, vault, cranker) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    let intents: Vec<(Pubkey, Pubkey, Pubkey)> = (0..MAX_K_WITHDRAW)
        .map(|_| {
            (
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                Pubkey::new_unique(),
            )
        })
        .collect();
    let ix = build_execute_round_ix(pool, vault, cranker, 0, &intents);
    let per_intent: Vec<Pubkey> = intents.iter().flat_map(|(a, b, c)| [*a, *b, *c]).collect();
    let msg = compile_at_max_k(ix, per_intent, cranker);
    let n = resolved_key_count(&msg);
    assert!(
        n <= 64,
        "withdraw MAX_K={MAX_K_WITHDRAW} resolves {n} keys > 64-lock limit"
    );
}

#[test]
fn stake_execute_round_at_max_k_fits_64_account_locks() {
    let (pool, vault, cranker) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    let validator = Pubkey::new_unique();
    let intents: Vec<(Pubkey, Pubkey, Pubkey)> = (0..MAX_K_STAKE)
        .map(|_| {
            (
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                Pubkey::new_unique(),
            )
        })
        .collect();
    let ix = build_execute_stake_round_ix(pool, vault, cranker, 0, validator, &intents);
    let per_intent: Vec<Pubkey> = intents.iter().flat_map(|(a, b, c)| [*a, *b, *c]).collect();
    let msg = compile_at_max_k(ix, per_intent, cranker);
    let n = resolved_key_count(&msg);
    assert!(
        n <= 64,
        "stake MAX_K={MAX_K_STAKE} resolves {n} keys > 64-lock limit"
    );
}
