use crate::PoolError;
use anchor_lang::prelude::*;

/// The k-floor: a round may only execute when it holds at least `k_floor`
/// intents. This is THE behavioral-anonymity invariant, enforced on-chain.
pub fn meets_k_floor(intent_count: u32, k_floor: u16) -> bool {
    intent_count >= k_floor as u32
}

/// Split a denomination into `(payout_to_recipient, fee_to_relayer)`, failing
/// closed if the fee exceeds the denomination (never underflows).
pub fn split_payout(denomination: u64, fee: u64) -> Result<(u64, u64)> {
    require!(fee <= denomination, PoolError::FeeExceedsDenomination);
    let payout = denomination
        .checked_sub(fee)
        .ok_or(error!(PoolError::FeeExceedsDenomination))?;
    Ok((payout, fee))
}

/// Stake account layout size (`StakeStateV2`) — used for the rent-exempt minimum
/// and the `create_account`/`allocate` size. Kept as a plain const so this pure
/// module stays syscall-free and host-testable; Task 2 adds a compile-time
/// `assert!(STAKE_ACCOUNT_SIZE == StakeStateV2::size_of())` in `action.rs` (where
/// the stake crate is imported) so the two can never drift.
pub const STAKE_ACCOUNT_SIZE: usize = 200;

/// The Stake program's minimum delegation (1 SOL on mainnet, verified via
/// `getStakeMinimumDelegation` — the `stake_raise_minimum_delegation_to_1_sol`
/// feature is active). The on-chain `DelegateStake` is the ultimate enforcer;
/// this const gates `initialize_pool` so a stake pool can't be created that would
/// fail every round.
pub const MIN_STAKE_DELEGATION: u64 = 1_000_000_000;

/// Split a stake pool's `denomination` into `(delegated, fee)`. The stake account
/// is funded with `denomination - stake_fee`; DelegateStake stakes its balance
/// above the rent reserve, so `delegated = denomination - stake_fee - stake_rent`.
/// Fails closed if the fee+rent exceed the denomination or the delegated amount is
/// below the network minimum.
pub fn stake_split(denomination: u64, stake_fee: u64, stake_rent: u64) -> Result<(u64, u64)> {
    let after_fee = denomination
        .checked_sub(stake_fee)
        .ok_or(error!(PoolError::FeeExceedsDenomination))?;
    let delegated = after_fee
        .checked_sub(stake_rent)
        .ok_or(error!(PoolError::StakeDenominationTooLow))?;
    require!(
        delegated >= MIN_STAKE_DELEGATION,
        PoolError::StakeDenominationTooLow
    );
    Ok((delegated, stake_fee))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k_floor_boundary() {
        assert!(!meets_k_floor(1, 2), "below floor rejected");
        assert!(meets_k_floor(2, 2), "exactly floor accepted");
        assert!(meets_k_floor(3, 2), "above floor accepted");
        assert!(!meets_k_floor(0, 2), "empty round rejected");
    }

    #[test]
    fn split_payout_conserves_value() {
        assert_eq!(split_payout(1_000, 10).unwrap(), (990, 10));
        assert_eq!(split_payout(1_000, 0).unwrap(), (1_000, 0));
        assert_eq!(split_payout(1_000, 1_000).unwrap(), (0, 1_000));
    }

    #[test]
    fn split_payout_rejects_fee_over_denomination() {
        assert!(
            split_payout(1_000, 1_001).is_err(),
            "fee > denomination fails closed"
        );
    }
}

#[cfg(test)]
mod stake_tests {
    use super::*;

    const RENT: u64 = 2_282_880; // ~rent-exempt for 200 bytes; exact value pinned at runtime

    #[test]
    fn stake_split_conserves_and_floors() {
        let denom = MIN_STAKE_DELEGATION + RENT + 5_000;
        assert_eq!(
            stake_split(denom, 5_000, RENT).unwrap(),
            (MIN_STAKE_DELEGATION, 5_000)
        );
    }

    #[test]
    fn stake_split_rejects_below_min_delegation() {
        // delegated = MIN - 1 < MIN → fail closed
        let denom = MIN_STAKE_DELEGATION - 1 + RENT + 5_000;
        assert!(stake_split(denom, 5_000, RENT).is_err());
    }

    #[test]
    fn stake_split_rejects_fee_plus_rent_over_denomination() {
        assert!(stake_split(1_000, 900, 200).is_err());
    }
}
