use std::path::Path;

use solana_sdk::{
    hash::hash as sha256,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    stake, system_program, sysvar,
};

use crate::{compute_ext_data_hash, MerklePath, Note, ProverError, PublicInputs, WithdrawInputs};

/// Anchor instruction discriminator = `sha256("global:<name>")[..8]`
/// (matches `programs/pool-program/tests/common.rs::disc`).
pub(crate) fn discriminator(name: &str) -> [u8; 8] {
    let h = sha256(format!("global:{name}").as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h.to_bytes()[..8]);
    d
}

/// Builds the `initialize_pool` instruction. Account order/writability
/// matches `programs/pool-program/src/lib.rs`'s `InitializePool` context.
#[allow(clippy::too_many_arguments)]
pub fn build_initialize_pool_ix(
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
    let mut data = discriminator("initialize_pool").to_vec();
    data.extend_from_slice(&denomination.to_le_bytes());
    data.extend_from_slice(&k_floor.to_le_bytes());
    data.push(action_kind);
    data.extend_from_slice(&validator.to_bytes());
    data.extend_from_slice(&fee.to_le_bytes());
    Instruction {
        program_id: pool_program::ID,
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(round, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    }
}

/// The PDA for a pool's round `round_id` (`["round", pool, round_id_le]`).
pub fn round_pda(pool: Pubkey, round_id: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &round_id.to_le_bytes()],
        &pool_program::ID,
    )
    .0
}

/// Builds the `deposit` instruction. Account order/writability matches
/// `programs/pool-program/src/lib.rs`'s `Deposit` context.
pub fn build_deposit_ix(
    pool: Pubkey,
    vault: Pubkey,
    payer: Pubkey,
    commitment: [u8; 32],
    amount: u64,
) -> Instruction {
    let mut data = discriminator("deposit").to_vec();
    data.extend_from_slice(&commitment);
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction {
        program_id: pool_program::ID,
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    }
}

/// Filesystem paths to the compiled withdraw circuit artifacts
/// (`circuits/build/withdraw_js/withdraw.wasm`, `circuits/build/withdraw.r1cs`,
/// `circuits/build/withdraw.zkey` — see `circuits/scripts/setup.sh`),
/// forwarded verbatim to `prover::prove_withdraw`.
#[derive(Debug, Clone, Copy)]
pub struct WithdrawArtifacts<'a> {
    pub wasm_path: &'a Path,
    pub r1cs_path: &'a Path,
    pub zkey_path: &'a Path,
}

#[derive(Debug, Clone)]
pub struct CommitIntentBuild {
    pub instruction: Instruction,
    pub public_inputs: PublicInputs,
}

/// Builds `commit_intent`: generates a real Groth16 proof for `note` bound to
/// `(recipient, relayer, fee)` via extDataHash, then encodes the instruction.
/// Account order matches `programs/pool-program/src/lib.rs`'s `CommitIntent`.
#[allow(clippy::too_many_arguments)]
pub fn build_commit_intent_ix(
    pool: Pubkey,
    round: Pubkey,
    recipient: Pubkey,
    relayer: Pubkey,
    payer: Pubkey,
    note: &Note,
    merkle_path: &MerklePath,
    root: [u8; 32],
    fee: u64,
    round_id: u64,
    artifacts: WithdrawArtifacts,
) -> Result<CommitIntentBuild, ProverError> {
    let ext_data_hash = compute_ext_data_hash(&recipient.to_bytes(), &relayer.to_bytes(), fee);
    let inputs = WithdrawInputs {
        root,
        nullifier_hash: note.nullifier_hash(),
        ext_data_hash,
        nullifier: note.nullifier(),
        secret: note.secret(),
        path_elements: merkle_path.elements,
        path_indices: merkle_path.indices,
    };
    let (proof, public_inputs) = prover::prove_withdraw(
        artifacts.wasm_path,
        artifacts.r1cs_path,
        artifacts.zkey_path,
        &inputs,
    )?;
    let withdraw_proof = pool_program::verifier::WithdrawProof {
        a: prover::proof_a_to_solana_be(&proof.a)?,
        b: prover::g2_to_solana_be(&proof.b)?,
        c: prover::g1_to_solana_be(&proof.c)?,
    };

    let (intent_pda, _) = Pubkey::find_program_address(
        &[
            b"intent",
            pool.as_ref(),
            public_inputs.nullifier_hash.as_ref(),
        ],
        &pool_program::ID,
    );
    let (nullifier_pda, _) = Pubkey::find_program_address(
        &[
            b"nullifier",
            pool.as_ref(),
            public_inputs.nullifier_hash.as_ref(),
        ],
        &pool_program::ID,
    );

    let mut data = discriminator("commit_intent").to_vec();
    data.extend_from_slice(&withdraw_proof.a);
    data.extend_from_slice(&withdraw_proof.b);
    data.extend_from_slice(&withdraw_proof.c);
    data.extend_from_slice(&public_inputs.root);
    data.extend_from_slice(&public_inputs.nullifier_hash);
    data.extend_from_slice(&fee.to_le_bytes());
    data.extend_from_slice(&round_id.to_le_bytes());

    let instruction = Instruction {
        program_id: pool_program::ID,
        accounts: vec![
            AccountMeta::new_readonly(pool, false),
            AccountMeta::new(round, false),
            AccountMeta::new(intent_pda, false),
            AccountMeta::new(nullifier_pda, false),
            AccountMeta::new_readonly(recipient, false),
            AccountMeta::new_readonly(relayer, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    Ok(CommitIntentBuild {
        instruction,
        public_inputs,
    })
}

/// Builds `execute_round`. `intents` is `(intent_pda, recipient, relayer)` per
/// committed intent, in any order; they become the `remaining_accounts`.
pub fn build_execute_round_ix(
    pool: Pubkey,
    vault: Pubkey,
    cranker: Pubkey,
    round_id: u64,
    intents: &[(Pubkey, Pubkey, Pubkey)],
) -> Instruction {
    let (round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &round_id.to_le_bytes()],
        &pool_program::ID,
    );
    let (next_round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &(round_id + 1).to_le_bytes()],
        &pool_program::ID,
    );
    let mut accounts = vec![
        AccountMeta::new(pool, false),
        AccountMeta::new(round, false),
        AccountMeta::new(next_round, false),
        AccountMeta::new(vault, false),
        AccountMeta::new(cranker, true),
        AccountMeta::new_readonly(system_program::ID, false),
    ];
    for (intent, recipient, relayer) in intents {
        accounts.push(AccountMeta::new(*intent, false));
        accounts.push(AccountMeta::new(*recipient, false));
        accounts.push(AccountMeta::new(*relayer, false));
    }
    let mut data = discriminator("execute_round").to_vec();
    data.extend_from_slice(&round_id.to_le_bytes());
    Instruction {
        program_id: pool_program::ID,
        accounts,
        data,
    }
}

/// The PDA for an intent's stake account (`["stake", pool, intent_pda]`),
/// seeded off the INTENT PDA key itself (not the raw `nullifier_hash`) —
/// matches the on-chain stake dispatch arm in `execute_round`
/// (`programs/pool-program/src/lib.rs`).
pub fn stake_account_pda(pool: Pubkey, intent_pda: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"stake", pool.as_ref(), intent_pda.as_ref()],
        &pool_program::ID,
    )
    .0
}

/// Builds `execute_round` for a STAKE pool (`pool.action_kind == 1`). `intents`
/// is `(intent_pda, stake_account_pda, relayer)` per committed intent, in any
/// order; the shared tail `[validator, stake_program, stake_config, clock,
/// stake_history, rent]` is appended automatically. A separate builder from
/// `build_execute_round_ix` (rather than a shared/branching one) because the
/// two pool kinds need structurally different `remaining_accounts` shapes and
/// this is still the only caller of either.
///
/// The caller MUST prepend an adequate
/// `ComputeBudgetInstruction::set_compute_unit_limit(...)` for the round: the
/// stake path runs 4 CPIs + a `find_program_address` per intent, measured
/// ~55,300 CU at k=2 (`execute_round_stakes_the_batch_uniformly`); the spec's
/// target k≈17 needs proportionally more headroom than the 400k default.
#[allow(deprecated)] // `stake::config::ID` — the Stake program still requires this account in DelegateStake's CPI even though the type is deprecated.
pub fn build_execute_stake_round_ix(
    pool: Pubkey,
    vault: Pubkey,
    cranker: Pubkey,
    round_id: u64,
    validator: Pubkey,
    intents: &[(Pubkey, Pubkey, Pubkey)],
) -> Instruction {
    let (round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &round_id.to_le_bytes()],
        &pool_program::ID,
    );
    let (next_round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &(round_id + 1).to_le_bytes()],
        &pool_program::ID,
    );
    let mut accounts = vec![
        AccountMeta::new(pool, false),
        AccountMeta::new(round, false),
        AccountMeta::new(next_round, false),
        AccountMeta::new(vault, false),
        AccountMeta::new(cranker, true),
        AccountMeta::new_readonly(system_program::ID, false),
    ];
    for (intent, stake_account, relayer) in intents {
        accounts.push(AccountMeta::new(*intent, false));
        accounts.push(AccountMeta::new(*stake_account, false));
        accounts.push(AccountMeta::new(*relayer, false));
    }
    accounts.push(AccountMeta::new_readonly(validator, false));
    accounts.push(AccountMeta::new_readonly(stake::program::ID, false));
    accounts.push(AccountMeta::new_readonly(stake::config::ID, false));
    accounts.push(AccountMeta::new_readonly(sysvar::clock::ID, false));
    accounts.push(AccountMeta::new_readonly(sysvar::stake_history::ID, false));
    accounts.push(AccountMeta::new_readonly(sysvar::rent::ID, false));
    let mut data = discriminator("execute_round").to_vec();
    data.extend_from_slice(&round_id.to_le_bytes());
    Instruction {
        program_id: pool_program::ID,
        accounts,
        data,
    }
}

/// Builds `cancel_intent` (recipient must sign).
pub fn build_cancel_intent_ix(
    pool: Pubkey,
    vault: Pubkey,
    recipient: Pubkey,
    round_id: u64,
    nullifier_hash: [u8; 32],
) -> Instruction {
    let (round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &round_id.to_le_bytes()],
        &pool_program::ID,
    );
    let (intent, _) = Pubkey::find_program_address(
        &[b"intent", pool.as_ref(), nullifier_hash.as_ref()],
        &pool_program::ID,
    );
    let mut data = discriminator("cancel_intent").to_vec();
    data.extend_from_slice(&round_id.to_le_bytes());
    data.extend_from_slice(&nullifier_hash);
    Instruction {
        program_id: pool_program::ID,
        accounts: vec![
            AccountMeta::new_readonly(pool, false),
            AccountMeta::new(round, false),
            AccountMeta::new(intent, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(recipient, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    }
}
