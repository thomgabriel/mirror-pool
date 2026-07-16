//! Minimal client SDK for the mirror-pool shielded pool.
//!
//! Every hash here is a thin wrapper over the SAME implementation the
//! on-chain program (and the circuit's witness generator) use — never a
//! re-derivation — so nothing in this crate can silently drift from what
//! `pool-program` actually checks:
//!
//! - `Note::commitment` calls `pool_program::poseidon::hash2` directly.
//! - `Note::nullifier_hash` calls `pool_program::poseidon::hash1` directly —
//!   the same single-input Poseidon `crates/parity-fixtures` (the circuit's
//!   own witness source) calls.
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
use solana_sdk::{
    hash::hash as sha256,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

pub use ext_data::ext_data_hash as compute_ext_data_hash;
pub use prover::{FieldBytes, ProverError, PublicInputs, WithdrawInputs, TREE_DEPTH};

/// Errors from fallible SDK constructors — never a panic path on
/// attacker/untrusted-influenced input (e.g. a `Note` deserialized from
/// disk or the network).
#[derive(Debug, PartialEq, Eq)]
pub enum SdkError {
    /// A note field is not a canonical, in-field BN254 scalar
    /// (`pool_program::poseidon::is_in_field`).
    NotInField,
    /// The Poseidon hash used to derive an empty-subtree constant failed.
    MerkleHash,
}

/// A shielded note: `commitment = Poseidon2(nullifier, secret)` (the
/// deposited leaf), spent via `nullifier_hash = Poseidon1(nullifier)` (the
/// public signal `withdraw` marks as spent). Fields are private; every
/// `Note` is guaranteed in-field by construction (`new`/`from_parts`), so
/// `commitment`/`nullifier_hash` never need to fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    nullifier: [u8; 32],
    secret: [u8; 32],
}

impl Note {
    /// Generates a fresh random note. Both fields are rejection-sampled to
    /// be canonical, in-field BN254 scalar values — a raw 32 random bytes is
    /// out of field (`>= r`) a majority of the time, since `r` is a ~254-bit
    /// prime — matching the in-field requirement
    /// `pool_program::poseidon::hash2`/`merkle::insert` enforce on the
    /// commitment.
    pub fn new() -> Self {
        let nullifier = random_field_element();
        let secret = random_field_element();
        Note::from_parts(nullifier, secret)
            .expect("random_field_element always returns in-field values")
    }

    /// Builds a `Note` from possibly-untrusted `nullifier`/`secret` bytes
    /// (e.g. deserialized from JSON), failing closed instead of panicking
    /// if either is not a canonical in-field BN254 scalar.
    pub fn from_parts(nullifier: [u8; 32], secret: [u8; 32]) -> Result<Note, SdkError> {
        if !poseidon::is_in_field(&nullifier) || !poseidon::is_in_field(&secret) {
            return Err(SdkError::NotInField);
        }
        Ok(Note { nullifier, secret })
    }

    pub fn nullifier(&self) -> [u8; 32] {
        self.nullifier
    }

    pub fn secret(&self) -> [u8; 32] {
        self.secret
    }

    /// `Poseidon2(nullifier, secret)` — matches
    /// `programs/pool-program/src/merkle.rs`'s deposited leaf and the
    /// circuit's `cm.inputs = [nullifier, secret]`
    /// (`circuits/circom/withdraw.circom`).
    pub fn commitment(&self) -> [u8; 32] {
        poseidon::hash2(&self.nullifier, &self.secret)
            .expect("Note fields are validated in-field by from_parts")
    }

    /// `Poseidon1(nullifier)` — matches the circuit's
    /// `nh.inputs[0] <== nullifier; nh.out === nullifierHash` and
    /// `crates/parity-fixtures`'s identical `pool_program::poseidon::hash1`
    /// call. This is the public `nullifier_hash` the on-chain `withdraw`
    /// handler checks against a fresh nullifier PDA for single-spend.
    pub fn nullifier_hash(&self) -> [u8; 32] {
        poseidon::hash1(&self.nullifier).expect("Note fields are validated in-field by from_parts")
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
    round: Pubkey,
    mint: Pubkey,
    payer: Pubkey,
    denomination: u64,
    k_floor: u16,
) -> Instruction {
    let mut data = discriminator("initialize_pool").to_vec();
    data.extend_from_slice(&denomination.to_le_bytes());
    data.extend_from_slice(&k_floor.to_le_bytes());
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

/// The Merkle authentication path for a deposited note (private circuit
/// inputs `pathElements`/`pathIndices`), depth matching `prover::TREE_DEPTH`
/// (= `programs/pool-program/src/merkle.rs::TREE_HEIGHT`).
#[derive(Debug, Clone)]
pub struct MerklePath {
    pub elements: [FieldBytes; TREE_DEPTH],
    pub indices: [u8; TREE_DEPTH],
}

/// Client-side incremental Merkle tree mirroring `pool_program::merkle`
/// (`TREE_HEIGHT` levels, Poseidon2 nodes, empty positions filled with the
/// same zero-subtree constants). Lets a client compute its note's
/// authentication path from the leaves it has scanned — the private
/// `MerklePath` inputs `prover::prove_withdraw` needs.
pub struct MerkleTree {
    leaves: Vec<[u8; 32]>,
    zeros: [[u8; 32]; TREE_DEPTH],
}

impl MerkleTree {
    pub fn new() -> Result<Self, SdkError> {
        let zeros = pool_program::merkle::zeros().map_err(|_| SdkError::MerkleHash)?;
        Ok(Self {
            leaves: Vec::new(),
            zeros,
        })
    }

    /// Append a commitment; returns its leaf index. `leaf` must be a canonical
    /// in-field BN254 scalar (e.g. `Note::commitment()`); `root()` /
    /// `authentication_path()` panic in `next_level` otherwise.
    pub fn insert(&mut self, leaf: [u8; 32]) -> usize {
        self.leaves.push(leaf);
        self.leaves.len() - 1
    }

    pub fn root(&self) -> [u8; 32] {
        let mut level = self.leaves.clone();
        for l in 0..TREE_DEPTH {
            level = Self::next_level(&level, self.zeros[l]);
        }
        // After TREE_DEPTH pairings the single remaining node is the root; an
        // empty tree collapses to the empty-root constant.
        level
            .first()
            .copied()
            .unwrap_or_else(|| pool_program::merkle::empty_root(&self.zeros).expect("empty_root"))
    }

    /// The authentication path (sibling per level, and the left/right bit per
    /// level) for `index` against the current root.
    pub fn authentication_path(&self, index: usize) -> MerklePath {
        let mut elements = [[0u8; 32]; TREE_DEPTH];
        let mut indices = [0u8; TREE_DEPTH];
        let mut level = self.leaves.clone();
        let mut pos = index;
        for l in 0..TREE_DEPTH {
            let bit = (pos % 2) as u8;
            indices[l] = bit;
            let sibling = pos ^ 1;
            elements[l] = level.get(sibling).copied().unwrap_or(self.zeros[l]);
            level = Self::next_level(&level, self.zeros[l]);
            pos /= 2;
        }
        MerklePath { elements, indices }
    }

    fn next_level(nodes: &[[u8; 32]], zero: [u8; 32]) -> Vec<[u8; 32]> {
        let mut out = Vec::with_capacity(nodes.len().div_ceil(2));
        let mut i = 0;
        while i < nodes.len() {
            let left = nodes[i];
            let right = nodes.get(i + 1).copied().unwrap_or(zero);
            out.push(poseidon::hash2(&left, &right).expect("node hash in-field"));
            i += 2;
        }
        out
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
        assert!(poseidon::is_in_field(&note.nullifier()));
        assert!(poseidon::is_in_field(&note.secret()));
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
            poseidon::hash2(&note.nullifier(), &note.secret()).unwrap()
        );
    }

    /// Real cross-crate agreement check: the SDK's `Note::nullifier_hash()`
    /// must equal the on-chain program's own `poseidon::hash1` for the same
    /// nullifier — not a tautology re-deriving the same call.
    #[test]
    fn nullifier_hash_agrees_with_pool_program_hash1() {
        let note = Note::new();
        assert_eq!(
            note.nullifier_hash(),
            pool_program::poseidon::hash1(&note.nullifier()).unwrap()
        );
    }

    #[test]
    fn from_parts_rejects_out_of_field_nullifier() {
        let too_big = [0xffu8; 32]; // > BN254 modulus
        assert_eq!(
            Note::from_parts(too_big, [1u8; 32]),
            Err(SdkError::NotInField)
        );
    }

    #[test]
    fn from_parts_rejects_out_of_field_secret() {
        let too_big = [0xffu8; 32]; // > BN254 modulus
        assert_eq!(
            Note::from_parts([1u8; 32], too_big),
            Err(SdkError::NotInField)
        );
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
        let round = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let payer = Pubkey::new_unique();
        let denomination = 1_000_000u64;
        let k_floor = 2u16;

        let ix = build_initialize_pool_ix(pool, vault, round, mint, payer, denomination, k_floor);

        assert_eq!(&ix.data[..8], &discriminator("initialize_pool"));
        assert_eq!(&ix.data[8..16], &denomination.to_le_bytes());
        assert_eq!(&ix.data[16..18], &k_floor.to_le_bytes());
        assert_eq!(ix.accounts.len(), 6);
        assert_eq!(ix.accounts[4].pubkey, payer);
        assert!(ix.accounts[4].is_signer);
    }

    fn tf(n: u8) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[31] = n;
        b
    }

    fn tdecode_be_hex(s: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
        }
        out
    }

    #[test]
    fn merkle_root_matches_pool_program_incremental_insert() {
        // Same two leaves the committed bundle uses, same order.
        let decoy = poseidon::hash2(&tf(111), &tf(222)).unwrap();
        let note = poseidon::hash2(&tf(7), &tf(9)).unwrap();

        let mut tree = MerkleTree::new().unwrap();
        tree.insert(decoy);
        tree.insert(note);

        // Independent reference: the on-chain program's own incremental insert.
        let z = pool_program::merkle::zeros().unwrap();
        let mut next_index = 0u32;
        let mut root = pool_program::merkle::empty_root(&z).unwrap();
        let mut filled = z;
        pool_program::merkle::insert(&mut next_index, &mut root, &mut filled, decoy).unwrap();
        pool_program::merkle::insert(&mut next_index, &mut root, &mut filled, note).unwrap();

        assert_eq!(
            tree.root(),
            root,
            "SDK tree root must match on-chain incremental insert"
        );
    }

    #[test]
    fn merkle_path_matches_committed_bundle() {
        // The committed circuit-validated bundle: reconstruct its 2-leaf tree and
        // assert the SDK computes the SAME root/path the circuit signed off on.
        let raw = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(std::path::Path::parent)
                .unwrap()
                .join("circuits/test/withdraw_vectors.json"),
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();

        let decoy = poseidon::hash2(&tf(111), &tf(222)).unwrap();
        let note = poseidon::hash2(&tf(7), &tf(9)).unwrap();
        let mut tree = MerkleTree::new().unwrap();
        tree.insert(decoy);
        let note_index = tree.insert(note); // 1

        assert_eq!(
            tree.root(),
            tdecode_be_hex(v["root"].as_str().unwrap()),
            "root"
        );
        let path = tree.authentication_path(note_index);
        let want_elems: Vec<[u8; 32]> = v["pathElements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| tdecode_be_hex(e.as_str().unwrap()))
            .collect();
        let want_idx: Vec<u8> = v["pathIndices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.as_u64().unwrap() as u8)
            .collect();
        assert_eq!(
            path.elements.to_vec(),
            want_elems,
            "pathElements must match circuit bundle"
        );
        assert_eq!(
            path.indices.to_vec(),
            want_idx,
            "pathIndices must match circuit bundle"
        );
    }
}
