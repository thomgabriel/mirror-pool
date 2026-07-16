//! Minimal client SDK for the mirror-pool shielded pool.
//!
//! Every hash here is a thin wrapper over the SAME implementation the
//! on-chain program (and the circuit's witness generator) use — never a
//! re-derivation — so nothing in this crate can silently drift from what
//! `pool-program` actually checks:
//!
//! - `Note::commitment` calls `pool_program::poseidon::hash2` directly.
//! - `Note::nullifier_hash` calls `solana_poseidon::hashv` with the exact
//!   arguments `crates/parity-fixtures` (the circuit's own witness source)
//!   uses for its single-input Poseidon.
//! - `compute_ext_data_hash` is a re-export of `ext_data::ext_data_hash`,
//!   the one shared implementation `pool-program`'s `withdraw` handler also
//!   calls.
//! - `build_withdraw_ix` generates its proof via `prover::prove_withdraw`
//!   and formats it with `prover::proof_a_to_solana_be`/`g1_to_solana_be`/
//!   `g2_to_solana_be` — the same encoding `pool-program`'s
//!   `groth16-solana` verifier expects.

use std::path::Path;

use pool_program::poseidon;
use rand::RngCore;
use solana_poseidon::{hashv, Endianness, Parameters};
use solana_sdk::{
    hash::hash as sha256,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

pub use ext_data::ext_data_hash as compute_ext_data_hash;
pub use prover::{FieldBytes, ProverError, PublicInputs, WithdrawInputs, TREE_DEPTH};

/// A shielded note: `commitment = Poseidon2(nullifier, secret)` (the
/// deposited leaf), spent via `nullifier_hash = Poseidon1(nullifier)` (the
/// public signal `withdraw` marks as spent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pub nullifier: [u8; 32],
    pub secret: [u8; 32],
}

impl Note {
    /// Generates a fresh random note. Both fields are rejection-sampled to
    /// be canonical, in-field BN254 scalar values — a raw 32 random bytes is
    /// out of field (`>= r`) a majority of the time, since `r` is a ~254-bit
    /// prime — matching the in-field requirement
    /// `pool_program::poseidon::hash2`/`merkle::insert` enforce on the
    /// commitment.
    pub fn new() -> Self {
        Note {
            nullifier: random_field_element(),
            secret: random_field_element(),
        }
    }

    /// `Poseidon2(nullifier, secret)` — matches
    /// `programs/pool-program/src/merkle.rs`'s deposited leaf and the
    /// circuit's `cm.inputs = [nullifier, secret]`
    /// (`circuits/circom/withdraw.circom`).
    pub fn commitment(&self) -> [u8; 32] {
        poseidon::hash2(&self.nullifier, &self.secret)
            .expect("Note fields are constructed to be in-field")
    }

    /// `Poseidon1(nullifier)` — matches the circuit's
    /// `nh.inputs[0] <== nullifier; nh.out === nullifierHash` and
    /// `crates/parity-fixtures`'s identical `hashv` call. This is the
    /// public `nullifier_hash` the on-chain `withdraw` handler checks
    /// against a fresh nullifier PDA for single-spend.
    pub fn nullifier_hash(&self) -> [u8; 32] {
        hashv(
            Parameters::Bn254X5,
            Endianness::BigEndian,
            &[self.nullifier.as_slice()],
        )
        .expect("Note::nullifier is constructed to be in-field")
        .to_bytes()
    }
}

impl Default for Note {
    fn default() -> Self {
        Self::new()
    }
}

fn random_field_element() -> [u8; 32] {
    let mut rng = rand::thread_rng();
    loop {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        if poseidon::is_in_field(&bytes) {
            return bytes;
        }
    }
}

/// Anchor instruction discriminator = `sha256("global:<name>")[..8]`
/// (matches `programs/pool-program/tests/common.rs::disc`).
fn discriminator(name: &str) -> [u8; 8] {
    let h = sha256(format!("global:{name}").as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h.to_bytes()[..8]);
    d
}

/// Builds the `initialize_pool` instruction. Account order/writability
/// matches `programs/pool-program/src/lib.rs`'s `InitializePool` context.
pub fn build_initialize_pool_ix(
    pool: Pubkey,
    vault: Pubkey,
    mint: Pubkey,
    payer: Pubkey,
    denomination: u64,
) -> Instruction {
    let mut data = discriminator("initialize_pool").to_vec();
    data.extend_from_slice(&denomination.to_le_bytes());
    Instruction {
        program_id: pool_program::ID,
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    }
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

/// The Merkle authentication path for a deposited note (private circuit
/// inputs `pathElements`/`pathIndices`), depth matching `prover::TREE_DEPTH`
/// (= `programs/pool-program/src/merkle.rs::TREE_HEIGHT`).
#[derive(Debug, Clone)]
pub struct MerklePath {
    pub elements: [FieldBytes; TREE_DEPTH],
    pub indices: [u8; TREE_DEPTH],
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

/// The result of building a `withdraw` instruction: the instruction itself,
/// plus the public inputs the witness actually computed (bound into the
/// proof — see `prover::PublicInputs`'s doc comment on why `ext_data_hash`
/// passes through unconstrained while `root`/`nullifier_hash` are
/// circuit-checked).
#[derive(Debug, Clone)]
pub struct WithdrawBuild {
    pub instruction: Instruction,
    pub public_inputs: PublicInputs,
}

/// Builds the `withdraw` instruction for `note`, generating a real Groth16
/// proof via `prover::prove_withdraw` and binding it to `(recipient, relayer,
/// fee)` through `ext_data_hash` — the SAME hash
/// `programs/pool-program/src/lib.rs`'s `withdraw` handler recomputes from
/// the payout accounts' keys, so a proof built here for one set of payout
/// accounts is rejected outright for any other (front-run safety).
///
/// Account order/writability matches `programs/pool-program/src/lib.rs`'s
/// `Withdraw` context; instruction data field order matches Anchor's
/// declaration-order Borsh encoding of `withdraw`'s args
/// (`proof`, `root`, `nullifier_hash`, `fee`).
#[allow(clippy::too_many_arguments)]
pub fn build_withdraw_ix(
    pool: Pubkey,
    vault: Pubkey,
    recipient: Pubkey,
    relayer: Pubkey,
    note: &Note,
    merkle_path: &MerklePath,
    root: [u8; 32],
    fee: u64,
    artifacts: WithdrawArtifacts,
) -> Result<WithdrawBuild, ProverError> {
    // The payout accounts ARE the hashed keys (matching the on-chain
    // handler's `ctx.accounts.recipient.key()`/`ctx.accounts.relayer.key()`),
    // so there is no separate "which accounts did I mean" argument to
    // desync from the accounts actually listed below.
    let ext_data_hash = compute_ext_data_hash(&recipient.to_bytes(), &relayer.to_bytes(), fee);

    let inputs = WithdrawInputs {
        root,
        nullifier_hash: note.nullifier_hash(),
        ext_data_hash,
        nullifier: note.nullifier,
        secret: note.secret,
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

    let (nullifier_pda, _) = Pubkey::find_program_address(
        &[
            b"nullifier",
            pool.as_ref(),
            public_inputs.nullifier_hash.as_ref(),
        ],
        &pool_program::ID,
    );

    // Anchor Borsh-serializes instruction args field-by-field in declaration
    // order: `proof: WithdrawProof { a, b, c }`, then `root`, `nullifier_hash`,
    // `fee` (matches `programs/pool-program/tests/withdraw.rs::withdraw_tx`).
    let mut data = discriminator("withdraw").to_vec();
    data.extend_from_slice(&withdraw_proof.a);
    data.extend_from_slice(&withdraw_proof.b);
    data.extend_from_slice(&withdraw_proof.c);
    data.extend_from_slice(&public_inputs.root);
    data.extend_from_slice(&public_inputs.nullifier_hash);
    data.extend_from_slice(&fee.to_le_bytes());

    let instruction = Instruction {
        program_id: pool_program::ID,
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(nullifier_pda, false),
            AccountMeta::new(recipient, false),
            AccountMeta::new(relayer, true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };

    Ok(WithdrawBuild {
        instruction,
        public_inputs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_new_produces_in_field_values() {
        let note = Note::new();
        assert!(poseidon::is_in_field(&note.nullifier));
        assert!(poseidon::is_in_field(&note.secret));
    }

    #[test]
    fn note_new_is_random() {
        assert_ne!(Note::new(), Note::new());
    }

    #[test]
    fn commitment_matches_hash2_directly() {
        let note = Note::new();
        assert_eq!(
            note.commitment(),
            poseidon::hash2(&note.nullifier, &note.secret).unwrap()
        );
    }

    #[test]
    fn nullifier_hash_matches_single_input_poseidon_directly() {
        let note = Note::new();
        let expected = hashv(
            Parameters::Bn254X5,
            Endianness::BigEndian,
            &[note.nullifier.as_slice()],
        )
        .unwrap()
        .to_bytes();
        assert_eq!(note.nullifier_hash(), expected);
    }

    #[test]
    fn deposit_ix_encodes_commitment_and_amount() {
        let pool = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let commitment = Note::new().commitment();
        let amount = 2_000_000u64;

        let ix = build_deposit_ix(pool, vault, payer, commitment, amount);

        assert_eq!(ix.program_id, pool_program::ID);
        assert_eq!(&ix.data[..8], &discriminator("deposit"));
        assert_eq!(&ix.data[8..40], &commitment);
        assert_eq!(&ix.data[40..48], &amount.to_le_bytes());
        assert_eq!(ix.accounts.len(), 4);
        assert_eq!(ix.accounts[0].pubkey, pool);
        assert_eq!(ix.accounts[1].pubkey, vault);
        assert_eq!(ix.accounts[2].pubkey, payer);
        assert!(ix.accounts[2].is_signer);
        assert_eq!(ix.accounts[3].pubkey, system_program::ID);
    }

    #[test]
    fn initialize_pool_ix_encodes_denomination() {
        let pool = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let denomination = 1_000_000u64;

        let ix = build_initialize_pool_ix(pool, vault, mint, payer, denomination);

        assert_eq!(&ix.data[..8], &discriminator("initialize_pool"));
        assert_eq!(&ix.data[8..16], &denomination.to_le_bytes());
        assert_eq!(ix.accounts.len(), 5);
        assert_eq!(ix.accounts[3].pubkey, payer);
        assert!(ix.accounts[3].is_signer);
    }
}
