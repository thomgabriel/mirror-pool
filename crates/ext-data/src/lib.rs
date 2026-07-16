//! `extDataHash` — the single shared binding of a withdraw's recipient,
//! relayer, and fee into one BN254 scalar-field element.
//!
//! `pool-program`, the SDK, and `parity-fixtures` (the circuit's witness
//! source) all call [`ext_data_hash`] instead of each re-deriving the hash —
//! any divergence in byte layout or reduction here would silently break every
//! honest proof (an off-chain-computed hash that the on-chain recomputation
//! doesn't match). See the [`tests`] module for the committed KAT that pins
//! this contract.
//!
//! Logic is written against `core`/fixed-size arrays only (no heap
//! allocation), so this crate builds equally well host-side and under
//! `cargo-build-sbf`.

use solana_program::keccak;

/// BN254 scalar field order `r`, big-endian
/// (`21888242871839275222246405745257275088548364400416034343698204186575808495617`).
const BN254_MODULUS_BE: [u8; 32] = [
    0x30, 0x64, 0x4e, 0x72, 0xe1, 0x31, 0xa0, 0x29, 0xb8, 0x50, 0x45, 0xb6, 0x81, 0x81, 0x58, 0x5d,
    0x28, 0x33, 0xe8, 0x48, 0x79, 0xb9, 0x70, 0x91, 0x43, 0xe1, 0xf5, 0x93, 0xf0, 0x00, 0x00, 0x01,
];

/// Binds a withdraw's payout accounts and fee into one public input.
///
/// `keccak256(recipient(32) ‖ relayer(32) ‖ fee.to_le_bytes()(8))`, interpreted
/// as a big-endian 256-bit integer and reduced modulo the BN254 scalar field
/// order `r` (a 32-byte pubkey doesn't fit in one ~254-bit field element, so
/// the circuit binds this hash rather than the raw account keys). The
/// reduction is a full repeated-subtraction loop, not a single conditional
/// subtract: a keccak digest spans up to `~5.3r` (`r` is a ~254-bit prime, the
/// digest a full 256-bit value), so a single subtraction can leave a result
/// `>= r` — which would silently diverge from `ark_ff`'s
/// `Fr::from_be_bytes_mod_order` (a full reduction) used when this hash is fed
/// into the circuit witness by `crates/prover`. The loop runs at most 5
/// iterations for any input, so there's no unbounded-loop/DoS concern on-chain.
pub fn ext_data_hash(recipient: &[u8; 32], relayer: &[u8; 32], fee: u64) -> [u8; 32] {
    let digest = keccak::hashv(&[recipient, relayer, &fee.to_le_bytes()]).to_bytes();
    reduce_mod_bn254(digest)
}

fn reduce_mod_bn254(mut digest: [u8; 32]) -> [u8; 32] {
    while be_ge(&digest, &BN254_MODULUS_BE) {
        digest = be_sub(&digest, &BN254_MODULUS_BE);
    }
    digest
}

/// `a >= b` for two big-endian 32-byte integers.
fn be_ge(a: &[u8; 32], b: &[u8; 32]) -> bool {
    for i in 0..32 {
        if a[i] != b[i] {
            return a[i] > b[i];
        }
    }
    true
}

/// `a - b` for two big-endian 32-byte integers; the caller must ensure `a >= b`.
fn be_sub(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    let mut borrow: i16 = 0;
    for i in (0..32).rev() {
        let diff = a[i] as i16 - b[i] as i16 - borrow;
        if diff < 0 {
            out[i] = (diff + 256) as u8;
            borrow = 1;
        } else {
            out[i] = diff as u8;
            borrow = 0;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feb(n: u8) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[31] = n;
        b
    }

    fn hex(b: &[u8; 32]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// Known-answer test pinning the exact `ext_data_hash` contract
    /// (concatenation order, LE `fee`, BE-interpret + full `mod r` reduce).
    /// `pool-program`'s on-chain recomputation and the SDK must reproduce this
    /// exact value for the same inputs — any divergence here breaks every
    /// honest proof's binding.
    #[test]
    fn known_answer() {
        let recipient = feb(1);
        let relayer = feb(2);
        let fee: u64 = 1_000;

        let got = ext_data_hash(&recipient, &relayer, fee);
        assert_eq!(
            hex(&got),
            "01eb65392c5412b5fef881a9b7373cde09d8844ddce82088c20f49101a639f1b"
        );
    }

    #[test]
    fn output_is_always_a_canonical_bn254_field_element() {
        // Every all-0xff digest input (the worst case for the reduction loop)
        // must land strictly below the modulus.
        let recipient = [0xffu8; 32];
        let relayer = [0xffu8; 32];
        for fee in [0u64, 1, u64::MAX] {
            let out = ext_data_hash(&recipient, &relayer, fee);
            assert!(be_ge(&BN254_MODULUS_BE, &out) && out != BN254_MODULUS_BE);
        }
    }

    #[test]
    fn deterministic_and_sensitive_to_every_field() {
        let base = ext_data_hash(&feb(1), &feb(2), 3);
        assert_eq!(base, ext_data_hash(&feb(1), &feb(2), 3));
        assert_ne!(base, ext_data_hash(&feb(9), &feb(2), 3));
        assert_ne!(base, ext_data_hash(&feb(1), &feb(9), 3));
        assert_ne!(base, ext_data_hash(&feb(1), &feb(2), 4));
    }
}
