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
/// module stays syscall-free and host-testable; a compile-time
/// `assert!(STAKE_ACCOUNT_SIZE == StakeStateV2::size_of())` in `action.rs` (where
/// the stake crate is imported) keeps the two from drifting.
pub const STAKE_ACCOUNT_SIZE: usize = 200;

/// The Stake program's minimum delegation (1 SOL on mainnet, verified via
/// `getStakeMinimumDelegation` — the `stake_raise_minimum_delegation_to_1_sol`
/// feature is active). The on-chain `DelegateStake` is the ultimate enforcer;
/// this const gates `initialize_pool` so a stake pool can't be created that would
/// fail every round.
pub const MIN_STAKE_DELEGATION: u64 = 1_000_000_000;

/// Split a stake pool's `denomination` into `(delegated, fee)`. The stake account
/// is funded with `denomination - fee`; DelegateStake stakes its balance
/// above the rent reserve, so `delegated = denomination - fee - stake_rent`.
/// Fails closed if the fee+rent exceed the denomination or the delegated amount is
/// below the network minimum.
pub fn stake_split(denomination: u64, fee: u64, stake_rent: u64) -> Result<(u64, u64)> {
    let after_fee = denomination
        .checked_sub(fee)
        .ok_or(error!(PoolError::FeeExceedsDenomination))?;
    let delegated = after_fee
        .checked_sub(stake_rent)
        .ok_or(error!(PoolError::StakeDenominationTooLow))?;
    require!(
        delegated >= MIN_STAKE_DELEGATION,
        PoolError::StakeDenominationTooLow
    );
    Ok((delegated, fee))
}

/// Slots a committed intent stays uncancelable, counted from its own commit.
/// ~1h at 400 ms/slot. A workload-contingent judgment call, not a derived number:
/// it means anything only if it is >= a credible fill horizon so that "the round
/// failed" is plausible by the time cancel opens. Promote to a bounded per-pool
/// config when fill horizons diverge (already true for stake vs withdraw); kept a
/// const here to avoid unused config surface.
pub const TIMEOUT_SLOTS: u64 = 9_000;

/// Earliest slot at which an intent committed at `committed_slot` may be cancelled.
/// Fails closed on overflow (cannot cancel) rather than wrapping.
pub fn cancel_unlock_slot(committed_slot: u64) -> Result<u64> {
    committed_slot
        .checked_add(TIMEOUT_SLOTS)
        .ok_or(error!(PoolError::CancelTooEarly))
}

/// Per-round intent caps: the executable-transaction envelope, enforced at
/// commit_intent so a round can never grow past what ONE vault-signed
/// execute_round transaction settles (past it, funds could exit only via the
/// linkable cancel path). Measured 2026-07-18 (full sweep + logs in
/// tests/max_k.rs's `sweep_execute_round_ceiling` and task-1-report.md):
///
/// - Withdraw: every k up to 21 executed with no failure of any kind (no
///   64-account-lock enforcement observed, no compute-ceiling hit). The
///   binding ceiling is therefore the plan's lock-arithmetic bound,
///   ⌊(64-9)/3⌋ = 18 (conservative, counting the ALT table key). Shipped one
///   below for cranker headroom: MAX_K_WITHDRAW = 17.
/// - Stake: k=8..11 executed; k=12+ ALL failed with
///   `InstructionError(_, ProgramFailedToComplete)` ("Program log: Error:
///   memory allocation failed, out of memory") at roughly a quarter of the CU
///   budget — the 32 KiB SBF bump-allocator heap, NOT compute or the account
///   lock. Re-measured with `request_heap_frame(256 * 1024)`: identical
///   failure at the same k — the default allocator is hard-capped regardless
///   of the requested frame size, so this ceiling is NOT liftable by the
///   cranker (only a custom global allocator could raise it — out of this
///   plan's scope; noted as future work). The binding ceiling is the
///   measured heap wall, 11, NOT the lock-arithmetic bound (16) or any
///   compute limit. Shipped one below measured: MAX_K_STAKE = 10.
///
/// Re-measure if Solana's increase_tx_account_lock_limit feature (64->128)
/// activates on mainnet (would only move withdraw's ceiling; stake's is
/// heap-bound and independent of the lock limit).
pub const MAX_K_WITHDRAW: u16 = 17;
pub const MAX_K_STAKE: u16 = 10;

pub fn max_k(kind: crate::round::ActionKind) -> u16 {
    match kind {
        crate::round::ActionKind::Withdraw => MAX_K_WITHDRAW,
        crate::round::ActionKind::Stake => MAX_K_STAKE,
    }
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

#[cfg(test)]
mod max_k_tests {
    use super::*;
    use crate::round::{ActionKind, MIN_K_FLOOR};

    #[test]
    fn max_k_selects_per_kind() {
        assert_eq!(max_k(ActionKind::Withdraw), MAX_K_WITHDRAW);
        assert_eq!(max_k(ActionKind::Stake), MAX_K_STAKE);
    }

    #[test]
    fn max_k_bounds_are_sane() {
        // A cap below the minimum floor would make every pool of that kind
        // uninitializable; stake's 6-account tail means its cap can never
        // exceed withdraw's. All-const inputs, so this is a compile-time
        // check (clippy's `assertions_on_constants` otherwise fires on a
        // runtime `assert!` here).
        const { assert!(MAX_K_WITHDRAW >= MIN_K_FLOOR) };
        const { assert!(MAX_K_STAKE >= MIN_K_FLOOR) };
        const { assert!(MAX_K_STAKE <= MAX_K_WITHDRAW) };
    }
}

#[cfg(test)]
mod cancel_tests {
    use super::*;

    #[test]
    fn cancel_unlock_slot_adds_timeout() {
        assert_eq!(cancel_unlock_slot(0).unwrap(), TIMEOUT_SLOTS);
        assert_eq!(cancel_unlock_slot(1_000).unwrap(), 1_000 + TIMEOUT_SLOTS);
    }

    #[test]
    fn cancel_unlock_slot_overflow_fails_closed() {
        // committed_slot so large that +TIMEOUT_SLOTS overflows u64 → cannot cancel.
        assert!(cancel_unlock_slot(u64::MAX).is_err());
        assert!(cancel_unlock_slot(u64::MAX - TIMEOUT_SLOTS + 1).is_err());
        // exactly representable boundary still succeeds:
        assert_eq!(
            cancel_unlock_slot(u64::MAX - TIMEOUT_SLOTS).unwrap(),
            u64::MAX
        );
    }
}
