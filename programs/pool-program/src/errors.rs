use anchor_lang::prelude::*;

#[error_code]
pub enum PoolError {
    #[msg("failed to initialize the merkle tree")]
    MerkleInit,
    #[msg("deposit amount must be greater than zero")]
    ZeroDeposit,
    #[msg("commitment is not a valid field element")]
    CommitmentNotInField,
    #[msg("merkle tree is full")]
    TreeFull,
    #[msg("proof bytes are malformed")]
    ProofMalformed,
    #[msg("proof failed verification")]
    ProofInvalid,
    #[msg("deposit amount must equal the pool's denomination")]
    WrongDenomination,
    #[msg("proof root is not a known recent root")]
    UnknownRoot,
    #[msg("fee must not exceed the pool's denomination")]
    FeeExceedsDenomination,
    #[msg("k_floor must be at least MIN_K_FLOOR")]
    KFloorTooLow,
    #[msg("round_id does not match the pool's current round")]
    WrongRound,
    #[msg("round is not open")]
    RoundClosed,
    #[msg("round intent count overflow")]
    RoundOverflow,
    #[msg("round has fewer intents than the k-floor")]
    KFloorNotMet,
    #[msg("wrong number of intent accounts for this round")]
    IntentAccountsMismatch,
    #[msg("intent account does not belong to this pool/round")]
    IntentInvalid,
    #[msg("payout account does not match the recorded intent")]
    IntentAccountMismatch,
    #[msg("duplicate intent account in the batch")]
    DuplicateIntent,
    #[msg("action_kind/validator/fee configuration is invalid for this pool")]
    WrongActionConfig,
    #[msg("stake pool denomination is too low to cover fee + rent + minimum delegation")]
    StakeDenominationTooLow,
    #[msg("account does not match the pool's configured stake action")]
    StakeAccountInvalid,
    #[msg("intent cannot be cancelled until its commit timeout has elapsed")]
    CancelTooEarly,
    #[msg("intent fee does not equal the pool's uniform fee")]
    FeeNotUniform,
    #[msg("round already holds the maximum number of executable intents")]
    RoundFull,
    #[msg("k_floor exceeds the maximum executable round size")]
    KFloorTooHigh,
}

#[cfg(test)]
mod abi_tests {
    use super::*;

    /// The variant order IS the error-code ABI (Anchor code = 6000 + discriminant).
    /// Name-based log assertions travel with the variant and cannot catch a reorder;
    /// this pin can. Append-only: new variants extend this list, never reorder it.
    /// (`Variant as u32` is always valid on a fieldless enum — no derive assumptions.)
    #[test]
    fn error_code_abi_is_pinned() {
        assert_eq!(PoolError::MerkleInit as u32, 0);
        assert_eq!(PoolError::ZeroDeposit as u32, 1);
        assert_eq!(PoolError::CommitmentNotInField as u32, 2);
        assert_eq!(PoolError::TreeFull as u32, 3);
        assert_eq!(PoolError::ProofMalformed as u32, 4);
        assert_eq!(PoolError::ProofInvalid as u32, 5);
        assert_eq!(PoolError::WrongDenomination as u32, 6);
        assert_eq!(PoolError::UnknownRoot as u32, 7);
        assert_eq!(PoolError::FeeExceedsDenomination as u32, 8);
        assert_eq!(PoolError::KFloorTooLow as u32, 9);
        assert_eq!(PoolError::WrongRound as u32, 10);
        assert_eq!(PoolError::RoundClosed as u32, 11);
        assert_eq!(PoolError::RoundOverflow as u32, 12);
        assert_eq!(PoolError::KFloorNotMet as u32, 13);
        assert_eq!(PoolError::IntentAccountsMismatch as u32, 14);
        assert_eq!(PoolError::IntentInvalid as u32, 15);
        assert_eq!(PoolError::IntentAccountMismatch as u32, 16);
        assert_eq!(PoolError::DuplicateIntent as u32, 17);
        assert_eq!(PoolError::WrongActionConfig as u32, 18);
        assert_eq!(PoolError::StakeDenominationTooLow as u32, 19);
        assert_eq!(PoolError::StakeAccountInvalid as u32, 20);
        assert_eq!(PoolError::CancelTooEarly as u32, 21);
        assert_eq!(PoolError::FeeNotUniform as u32, 22);
        assert_eq!(PoolError::RoundFull as u32, 23);
        assert_eq!(PoolError::KFloorTooHigh as u32, 24);
    }
}
