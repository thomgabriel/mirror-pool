//! On-chain Groth16 verification of the membership circuit's proof, against
//! the embedded `vk::MEMBERSHIP_VK`.
//!
//! Byte layout for `a`/`b`/`c` must come from `prover::proof_a_to_solana_be`
//! / `g2_to_solana_be` / `g1_to_solana_be` (or the SDK's equivalent) — `a` is
//! expected PRE-negated by the caller; this module does not re-negate it.

use anchor_lang::prelude::*;
use groth16_solana::groth16::Groth16Verifier;

use crate::vk::MEMBERSHIP_VK;
use crate::PoolError;

/// The membership circuit's Groth16 proof, in `groth16-solana`'s BE byte
/// layout (`a` PRE-negated).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct MembershipProof {
    pub a: [u8; 64],
    pub b: [u8; 128],
    pub c: [u8; 64],
}

/// Verifies a membership proof over the public inputs
/// `[root, nullifierHash, extDataHash]` (each a 32-byte big-endian BN254
/// scalar field element, matching the circuit's declared public-input
/// order).
pub fn verify_membership(proof: &MembershipProof, public_inputs: &[[u8; 32]; 3]) -> Result<()> {
    let mut verifier =
        Groth16Verifier::new(&proof.a, &proof.b, &proof.c, public_inputs, &MEMBERSHIP_VK)
            .map_err(|_| error!(PoolError::ProofMalformed))?;
    verifier
        .verify()
        .map_err(|_| error!(PoolError::ProofInvalid))?;
    Ok(())
}
