use pool_program::poseidon;
use rand::RngCore;

use crate::SdkError;

/// A shielded note: `commitment = Poseidon2(nullifier, secret)` (the
/// deposited leaf), spent via `nullifier_hash = Poseidon1(nullifier)` (the
/// public signal `commit_intent` marks as spent). Fields are private; every
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
    /// (`circuits/circom/membership.circom`).
    pub fn commitment(&self) -> [u8; 32] {
        poseidon::hash2(&self.nullifier, &self.secret)
            .expect("Note fields are validated in-field by from_parts")
    }

    /// `Poseidon1(nullifier)` — matches the circuit's
    /// `nh.inputs[0] <== nullifier; nh.out === nullifierHash` and
    /// `crates/parity-fixtures`'s identical `pool_program::poseidon::hash1`
    /// call. This is the public `nullifier_hash` the on-chain `commit_intent`
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
