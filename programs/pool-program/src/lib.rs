use anchor_lang::prelude::*;

pub mod action;
pub mod errors;
pub mod instructions;
pub mod invariants;
pub mod merkle;
pub mod nullifier;
pub mod poseidon;
pub mod roots;
pub mod round;
pub mod state;
pub mod verifier;
pub mod vk;

pub use errors::PoolError;
pub use instructions::*;

declare_id!("7oHnDkpPbhPacDfqzF38caM3eo1Xo7cBmFugNXJurnn3");

#[program]
pub mod pool_program {
    use super::*;

    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        denomination: u64,
        k_floor: u16,
        action_kind: u8,
        validator: Pubkey,
        fee: u64,
    ) -> Result<()> {
        instructions::initialize_pool::handler(
            ctx,
            denomination,
            k_floor,
            action_kind,
            validator,
            fee,
        )
    }

    pub fn deposit(ctx: Context<Deposit>, commitment: [u8; 32], amount: u64) -> Result<()> {
        instructions::deposit::handler(ctx, commitment, amount)
    }

    pub fn commit_intent(
        ctx: Context<CommitIntent>,
        proof: crate::verifier::WithdrawProof,
        root: [u8; 32],
        nullifier_hash: [u8; 32],
        fee: u64,
        round_id: u64,
    ) -> Result<()> {
        instructions::commit_intent::handler(ctx, proof, root, nullifier_hash, fee, round_id)
    }

    /// Coordinator-independent safety valve: a committed intent's own recipient
    /// (which must sign — `has_one`) reclaims its deposit while the round is
    /// still Open. This is a SINGLE-NOTE, non-batch exit — it does NOT provide
    /// k-anonymity (that is `execute_round`'s batch property); it exists so a
    /// committed participant's funds can never be locked by a censoring/offline
    /// coordinator or a round that never reaches `k`. The nullifier stays burned,
    /// so the reclaimed note can never be re-spent.
    pub fn cancel_intent(
        ctx: Context<CancelIntent>,
        _round_id: u64,
        _nullifier_hash: [u8; 32],
    ) -> Result<()> {
        instructions::cancel_intent::handler(ctx, _round_id, _nullifier_hash)
    }

    pub fn execute_round<'info>(
        ctx: Context<'_, '_, 'info, 'info, ExecuteRound<'info>>,
        round_id: u64,
    ) -> Result<()> {
        instructions::execute_round::handler(ctx, round_id)
    }
}
