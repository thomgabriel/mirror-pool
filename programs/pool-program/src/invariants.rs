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
