use solana_poseidon::{hashv, Endianness, Parameters};

/// BN254 scalar field modulus, big-endian.
pub const BN254_MODULUS_BE: [u8; 32] = [
    0x30, 0x64, 0x4e, 0x72, 0xe1, 0x31, 0xa0, 0x29, 0xb8, 0x50, 0x45, 0xb6, 0x81, 0x81, 0x58, 0x5d,
    0x28, 0x33, 0xe8, 0x48, 0x79, 0xb9, 0x70, 0x91, 0x43, 0xe1, 0xf5, 0x93, 0xf0, 0x00, 0x00, 0x01,
];

#[derive(Debug, PartialEq, Eq)]
pub enum PoseidonError {
    NotInField,
    HashFailed,
}

/// True iff `bytes` (big-endian) is a canonical BN254 field element (< modulus).
pub fn is_in_field(bytes: &[u8; 32]) -> bool {
    for i in 0..32 {
        if bytes[i] < BN254_MODULUS_BE[i] {
            return true;
        }
        if bytes[i] > BN254_MODULUS_BE[i] {
            return false;
        }
    }
    false // equal to modulus is NOT in field
}

/// Circom-compatible BN254 Poseidon over two field elements, big-endian I/O.
/// Uses the native Solana `poseidon` syscall on-chain; the same call has a host
/// implementation, so this also runs under `cargo test`.
pub fn hash2(left: &[u8; 32], right: &[u8; 32]) -> core::result::Result<[u8; 32], PoseidonError> {
    if !is_in_field(left) || !is_in_field(right) {
        return Err(PoseidonError::NotInField);
    }
    let h = hashv(
        Parameters::Bn254X5,
        Endianness::BigEndian,
        &[left.as_slice(), right.as_slice()],
    )
    .map_err(|_| PoseidonError::HashFailed)?;
    Ok(h.to_bytes())
}

/// Circom-compatible BN254 Poseidon over a single field element, big-endian
/// I/O. Uses the same native Solana `poseidon` syscall as `hash2`.
pub fn hash1(x: &[u8; 32]) -> core::result::Result<[u8; 32], PoseidonError> {
    if !is_in_field(x) {
        return Err(PoseidonError::NotInField);
    }
    let h = hashv(Parameters::Bn254X5, Endianness::BigEndian, &[x.as_slice()])
        .map_err(|_| PoseidonError::HashFailed)?;
    Ok(h.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash2_is_deterministic_and_nonzero() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let h1 = hash2(&a, &b).unwrap();
        let h2 = hash2(&a, &b).unwrap();
        assert_eq!(h1, h2, "Poseidon must be deterministic");
        assert_ne!(h1, [0u8; 32], "hash of nonzero inputs must be nonzero");
    }

    #[test]
    fn hash2_is_order_sensitive() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        assert_ne!(hash2(&a, &b).unwrap(), hash2(&b, &a).unwrap());
    }

    #[test]
    fn rejects_out_of_field_input() {
        let too_big = [0xffu8; 32]; // > BN254 modulus
        assert!(matches!(
            hash2(&too_big, &[0u8; 32]),
            Err(PoseidonError::NotInField)
        ));
    }

    #[test]
    fn rejects_field_modulus_exactly() {
        // The modulus itself is NOT a valid field element (canonical range is < modulus).
        assert!(!is_in_field(&BN254_MODULUS_BE));
        assert!(matches!(
            hash2(&BN254_MODULUS_BE, &[0u8; 32]),
            Err(PoseidonError::NotInField)
        ));
    }

    #[test]
    fn hash1_is_deterministic_and_nonzero() {
        let a = [1u8; 32];
        let h1 = hash1(&a).unwrap();
        let h2 = hash1(&a).unwrap();
        assert_eq!(h1, h2, "Poseidon must be deterministic");
        assert_ne!(h1, [0u8; 32], "hash of nonzero input must be nonzero");
    }

    #[test]
    fn hash1_rejects_out_of_field_input() {
        let too_big = [0xffu8; 32]; // > BN254 modulus
        assert!(matches!(hash1(&too_big), Err(PoseidonError::NotInField)));
    }
}
