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
//!   the one shared implementation `pool-program`'s `commit_intent` handler
//!   also calls.
//! - `build_commit_intent_ix` generates its proof via `prover::prove_withdraw`
//!   and formats it with `prover::proof_a_to_solana_be`/`g1_to_solana_be`/
//!   `g2_to_solana_be` — the same encoding `pool-program`'s
//!   `groth16-solana` verifier expects.

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

mod ix;
mod note;
mod tree;

pub use ix::*;
pub use note::*;
pub use tree::*;

#[cfg(test)]
mod tests {
    use pool_program::poseidon;
    use solana_sdk::{pubkey::Pubkey, system_program};

    use super::*;
    use crate::ix::discriminator;

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

        let ix = build_initialize_pool_ix(
            pool,
            vault,
            round,
            mint,
            payer,
            denomination,
            k_floor,
            0,
            Pubkey::default(),
            0,
        );

        assert_eq!(&ix.data[..8], &discriminator("initialize_pool"));
        assert_eq!(&ix.data[8..16], &denomination.to_le_bytes());
        assert_eq!(&ix.data[16..18], &k_floor.to_le_bytes());
        assert_eq!(ix.data[18], 0, "action_kind");
        assert_eq!(&ix.data[19..51], &Pubkey::default().to_bytes(), "validator");
        assert_eq!(&ix.data[51..59], &0u64.to_le_bytes(), "fee");
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
