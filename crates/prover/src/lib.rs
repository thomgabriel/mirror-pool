//! Rust proving harness for the withdraw circuit (`ark-circom` + `ark-groth16`).
//!
//! The public API is big-endian 32-byte throughout (matching the note bundle
//! and the on-chain Poseidon/Merkle hashes); conversion to/from
//! `ark_bn254::Fr` happens only at this module's boundary, via
//! `Fr::from_be_bytes_mod_order` / `into_bigint().to_bytes_be()`. Do not
//! substitute ark's canonical (little-endian) (de)serialization here — it
//! silently decodes a different field element from the same big-endian bytes.

use ark_bn254::{Bn254, Fr, G1Affine, G2Affine};
use ark_circom::{read_zkey, CircomBuilder, CircomConfig, CircomReduction};
use ark_ff::{BigInteger, PrimeField};
use ark_groth16::{Groth16, Proof};
use ark_serialize::{CanonicalSerialize, Compress};
use num_bigint::{BigInt, BigUint};
use std::fmt;
use std::fs::File;
use std::ops::Neg;
use std::path::Path;

/// Merkle tree depth of the withdraw circuit (`circuits/circom/withdraw.circom`).
pub const TREE_DEPTH: usize = 20;

/// A big-endian 32-byte field element, as used throughout the note bundle
/// and the on-chain hashes.
pub type FieldBytes = [u8; 32];

/// The withdraw circuit's named signals, decoded from the note bundle
/// (`circuits/test/withdraw_vectors.json`).
#[derive(Debug, Clone)]
pub struct WithdrawInputs {
    pub root: FieldBytes,
    pub nullifier_hash: FieldBytes,
    pub nullifier: FieldBytes,
    pub secret: FieldBytes,
    pub path_elements: [FieldBytes; TREE_DEPTH],
    pub path_indices: [u8; TREE_DEPTH],
}

/// The circuit's public signals, in the order declared by
/// `component main {public [root, nullifierHash]}` — this is also the order
/// `ark_groth16::Groth16::verify_proof` expects them in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicInputs {
    pub root: FieldBytes,
    pub nullifier_hash: FieldBytes,
}

impl PublicInputs {
    /// Public signals as field elements, in circuit declaration order.
    pub fn as_fr(&self) -> [Fr; 2] {
        [be_to_fr(&self.root), be_to_fr(&self.nullifier_hash)]
    }
}

#[derive(Debug)]
pub enum ProverError {
    Io(std::io::Error),
    /// `CircomConfig`/`CircomBuilder` setup or witness generation failed.
    Circuit(String),
    /// The `.zkey` proving key failed to parse.
    Zkey(String),
    /// `ark-groth16` proof generation failed.
    Synthesis(String),
    /// The witness didn't expose the two expected public signals.
    UnexpectedPublicInputCount(usize),
}

impl fmt::Display for ProverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProverError::Io(e) => write!(f, "io error: {e}"),
            ProverError::Circuit(e) => write!(f, "circom circuit error: {e}"),
            ProverError::Zkey(e) => write!(f, "zkey read error: {e}"),
            ProverError::Synthesis(e) => write!(f, "groth16 proving error: {e}"),
            ProverError::UnexpectedPublicInputCount(n) => {
                write!(f, "expected 2 public inputs (root, nullifierHash), got {n}")
            }
        }
    }
}

impl std::error::Error for ProverError {}

impl From<std::io::Error> for ProverError {
    fn from(e: std::io::Error) -> Self {
        ProverError::Io(e)
    }
}

/// Decodes a big-endian 32-byte field element, reducing modulo the BN254
/// scalar field order if necessary (this is `ark_ff`'s BE-aware entry point —
/// NOT the same as `CanonicalDeserialize`, which is little-endian).
pub fn be_to_fr(bytes: &FieldBytes) -> Fr {
    Fr::from_be_bytes_mod_order(bytes)
}

/// Encodes a field element back to big-endian 32 bytes. Round-trips with
/// [`be_to_fr`] for any `bytes` that already represent an in-field value.
pub fn fr_to_be(fr: Fr) -> FieldBytes {
    let be = fr.into_bigint().to_bytes_be();
    let mut out = [0u8; 32];
    out.copy_from_slice(&be);
    out
}

/// `circom`'s witness calculator takes signals as (signed) `num_bigint::BigInt`s.
/// Routing every value through [`be_to_fr`] first (rather than
/// `BigInt::from_bytes_be` directly) guarantees the same big-endian decoding
/// — and the same modular reduction — as everywhere else in this module.
fn fr_to_circom_bigint(bytes: &FieldBytes) -> BigInt {
    let biguint: BigUint = be_to_fr(bytes).into_bigint().into();
    BigInt::from(biguint)
}

/// Generates a real Groth16 proof for the withdraw circuit.
///
/// `wasm_path`/`r1cs_path` are `circuits/build/withdraw_js/withdraw.wasm` and
/// `circuits/build/withdraw.r1cs`; `zkey_path` is `circuits/build/withdraw.zkey`.
/// Returns the proof plus the public signals the witness actually computed
/// (bound to `inputs.root`/`inputs.nullifier_hash` by the circuit's `===`
/// constraints, so they match unless the witness itself was rejected).
pub fn prove_withdraw(
    wasm_path: impl AsRef<Path>,
    r1cs_path: impl AsRef<Path>,
    zkey_path: impl AsRef<Path>,
    inputs: &WithdrawInputs,
) -> Result<(Proof<Bn254>, PublicInputs), ProverError> {
    // ark-circom's WASI witness environment (wasmer-wasix) reaches for a
    // running Tokio reactor even though building/witnessing here is
    // synchronous; entering a bare runtime satisfies that without requiring
    // callers of this (sync) function to run inside one themselves.
    let rt = tokio::runtime::Builder::new_current_thread().build()?;
    let _rt_guard = rt.enter();

    let cfg = CircomConfig::<Fr>::new(wasm_path, r1cs_path)
        .map_err(|e| ProverError::Circuit(e.to_string()))?;
    let mut builder = CircomBuilder::new(cfg);

    builder.push_input("root", fr_to_circom_bigint(&inputs.root));
    builder.push_input("nullifierHash", fr_to_circom_bigint(&inputs.nullifier_hash));
    builder.push_input("nullifier", fr_to_circom_bigint(&inputs.nullifier));
    builder.push_input("secret", fr_to_circom_bigint(&inputs.secret));
    for elem in &inputs.path_elements {
        builder.push_input("pathElements", fr_to_circom_bigint(elem));
    }
    for &bit in &inputs.path_indices {
        builder.push_input("pathIndices", BigInt::from(bit));
    }

    let circuit = builder
        .build()
        .map_err(|e| ProverError::Circuit(e.to_string()))?;

    let public_signals = circuit
        .get_public_inputs()
        .ok_or(ProverError::UnexpectedPublicInputCount(0))?;
    if public_signals.len() != 2 {
        return Err(ProverError::UnexpectedPublicInputCount(
            public_signals.len(),
        ));
    }
    let public_inputs = PublicInputs {
        root: fr_to_be(public_signals[0]),
        nullifier_hash: fr_to_be(public_signals[1]),
    };

    let mut zkey_file = File::open(zkey_path)?;
    let (pk, _matrices) =
        read_zkey(&mut zkey_file).map_err(|e| ProverError::Zkey(e.to_string()))?;

    let mut rng = ark_std::rand::thread_rng();
    // CircomReduction, not the default LibsnarkReduction: circom's R1CS-to-QAP
    // mapping differs from ark-groth16's default, and proving against the
    // wrong reduction yields a proof that fails to verify against snarkjs's VK.
    let proof = Groth16::<Bn254, CircomReduction>::create_random_proof_with_reduction(
        circuit, &pk, &mut rng,
    )
    .map_err(|e| ProverError::Synthesis(e.to_string()))?;

    Ok((proof, public_inputs))
}

fn reverse32(chunk: &[u8]) -> FieldBytes {
    let mut out = [0u8; 32];
    out.copy_from_slice(chunk);
    out.reverse();
    out
}

/// Encodes a G1 point as the 64-byte big-endian `[be(x), be(y)]` layout
/// `groth16-solana`/the `alt_bn128` syscalls expect (EIP-197). `ark-bn254`'s
/// own `CanonicalSerialize` is little-endian per-coordinate, so each 32-byte
/// half is reversed independently — no coordinate reordering, unlike G2.
pub fn g1_to_solana_be(p: &G1Affine) -> Result<[u8; 64], ProverError> {
    let mut le = [0u8; 64];
    p.x.serialize_with_mode(&mut le[..32], Compress::No)
        .map_err(|e| ProverError::Synthesis(e.to_string()))?;
    p.y.serialize_with_mode(&mut le[32..], Compress::No)
        .map_err(|e| ProverError::Synthesis(e.to_string()))?;
    let mut out = [0u8; 64];
    out[..32].copy_from_slice(&reverse32(&le[..32]));
    out[32..].copy_from_slice(&reverse32(&le[32..]));
    Ok(out)
}

/// Encodes a G2 point as the 128-byte big-endian `[be(x1), be(x0), be(y1),
/// be(y0)]` layout `groth16-solana`/`alt_bn128` expect (EIP-197). Unlike G1,
/// this also swaps the `Fq2` coefficient order — `ark-bn254` serializes
/// `[le(x0), le(x1), le(y0), le(y1)]` (c0 before c1); EIP-197 wants the
/// "imaginary" coefficient first.
pub fn g2_to_solana_be(p: &G2Affine) -> Result<[u8; 128], ProverError> {
    let mut le = [0u8; 128];
    p.x.serialize_with_mode(&mut le[..64], Compress::No)
        .map_err(|e| ProverError::Synthesis(e.to_string()))?;
    p.y.serialize_with_mode(&mut le[64..], Compress::No)
        .map_err(|e| ProverError::Synthesis(e.to_string()))?;
    let x0 = reverse32(&le[0..32]);
    let x1 = reverse32(&le[32..64]);
    let y0 = reverse32(&le[64..96]);
    let y1 = reverse32(&le[96..128]);
    let mut out = [0u8; 128];
    out[0..32].copy_from_slice(&x1);
    out[32..64].copy_from_slice(&x0);
    out[64..96].copy_from_slice(&y1);
    out[96..128].copy_from_slice(&y0);
    Ok(out)
}

/// Encodes `proof.a`, negated, in the `groth16-solana` G1 byte layout.
///
/// `groth16-solana`'s pairing check is arranged with `proof.a` on the
/// opposite side from `ark-groth16`'s own `verify_proof` — every circom/EVM
/// Groth16 verifier negates `proof.a` for exactly this reason. `proof.b` and
/// `proof.c` need no such adjustment, only the endianness conversion in
/// [`g1_to_solana_be`]/[`g2_to_solana_be`].
pub fn proof_a_to_solana_be(a: &G1Affine) -> Result<[u8; 64], ProverError> {
    g1_to_solana_be(&a.neg())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_decode_32(s: &str) -> FieldBytes {
        assert_eq!(s.len(), 64, "expected 64 hex chars (32 bytes)");
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap();
        }
        out
    }

    #[test]
    fn be_fr_round_trip_holds_for_in_field_values() {
        let cases: &[FieldBytes] = &[
            [0u8; 32],
            {
                let mut b = [0u8; 32];
                b[31] = 1;
                b
            },
            {
                let mut b = [0u8; 32];
                b[0] = 0x0f;
                b[31] = 0x07;
                b
            },
            // BN254 Fr modulus minus 1 — the largest representable in-field value.
            hex_decode_32("30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000000"),
        ];
        for b in cases {
            let fr = be_to_fr(b);
            assert_eq!(fr_to_be(fr), *b, "round-trip mismatch for {b:?}");
        }
    }

    #[test]
    fn be_fr_round_trip_holds_for_note_bundle_values() {
        // Values taken from circuits/test/withdraw_vectors.json — real
        // in-field BN254 scalars, not synthetic edge cases.
        let hex_values = [
            "0000000000000000000000000000000000000000000000000000000000000007",
            "2f447495cd13dfa223b07ada1d51ac114901e15056a30f8bf28f6fbb4a27376a",
        ];
        for h in hex_values {
            let bytes = hex_decode_32(&h[h.len() - 64..]);
            let fr = be_to_fr(&bytes);
            assert_eq!(fr_to_be(fr), bytes);
        }
    }
}
