use anchor_lang::prelude::*;

use crate::state::Pool;
use crate::PoolError;

pub fn handler(
    ctx: Context<CommitIntent>,
    proof: crate::verifier::WithdrawProof,
    root: [u8; 32],
    nullifier_hash: [u8; 32],
    fee: u64,
    round_id: u64,
) -> Result<()> {
    let action_kind = {
        let pool = ctx.accounts.pool.load()?;
        require!(round_id == pool.current_round_id, PoolError::WrongRound);
        require!(
            crate::roots::is_known(&pool.roots, &root),
            PoolError::UnknownRoot
        );
        // Uniform pool-wide fee for BOTH action kinds: a variable fee is a payout-amount
        // fingerprint (settlement side), and fee=0 on withdraw is free self-fill.
        require!(fee == pool.fee, PoolError::FeeNotUniform);
        pool.action_kind
    };
    let kind = match action_kind {
        0 => crate::round::ActionKind::Withdraw,
        1 => crate::round::ActionKind::Stake,
        _ => return err!(PoolError::WrongActionConfig),
    };
    require!(
        ctx.accounts.round.state == crate::round::RoundState::Open,
        PoolError::RoundClosed
    );

    // extDataHash is computed from the recorded payout KEYS (the accounts
    // whose pubkeys `execute_round` pays), so the proof binds exactly the
    // keys stored in the Intent — no redirection possible.
    let ext = ext_data::ext_data_hash(
        &ctx.accounts.recipient.key().to_bytes(),
        &ctx.accounts.relayer.key().to_bytes(),
        fee,
    );
    crate::verifier::verify_withdraw(&proof, &[root, nullifier_hash, ext])?;

    // The nullifier PDA's `init` already enforced single-commit atomically.
    ctx.accounts.nullifier.spent = true;

    let intent = &mut ctx.accounts.intent;
    intent.pool = ctx.accounts.pool.key();
    intent.round_id = round_id;
    intent.recipient = ctx.accounts.recipient.key();
    intent.relayer = ctx.accounts.relayer.key();
    intent.fee = fee;
    intent.action = kind;
    intent.committed_slot = Clock::get()?.slot;

    let round = &mut ctx.accounts.round;
    round.intent_count = round
        .intent_count
        .checked_add(1)
        .ok_or(error!(PoolError::RoundOverflow))?;
    // MAX_K is the maximum EXECUTABLE count: past it, no single vault-signed
    // transaction can settle the round and funds could exit only via the
    // linkable cancel path — so the cap fails closed here, at commit.
    require!(
        round.intent_count <= crate::invariants::max_k(kind) as u32,
        PoolError::RoundFull
    );
    Ok(())
}

#[derive(Accounts)]
#[instruction(proof: crate::verifier::WithdrawProof, root: [u8; 32], nullifier_hash: [u8; 32], fee: u64, round_id: u64)]
pub struct CommitIntent<'info> {
    #[account(
        seeds = [b"pool", pool.load()?.mint.as_ref()],
        bump = pool.load()?.bump
    )]
    pub pool: AccountLoader<'info, Pool>,

    #[account(
        mut,
        seeds = [b"round", pool.key().as_ref(), &round_id.to_le_bytes()],
        bump
    )]
    pub round: Account<'info, crate::round::Round>,

    #[account(
        init,
        payer = payer,
        space = crate::round::Intent::SPACE,
        seeds = [b"intent", pool.key().as_ref(), nullifier_hash.as_ref()],
        bump
    )]
    pub intent: Account<'info, crate::round::Intent>,

    #[account(
        init,
        payer = payer,
        space = 8 + 1,
        seeds = [b"nullifier", pool.key().as_ref(), nullifier_hash.as_ref()],
        bump
    )]
    pub nullifier: Account<'info, crate::nullifier::NullifierRecord>,

    /// CHECK: payout recipient; bound into the proof via extDataHash, recorded in the Intent.
    pub recipient: SystemAccount<'info>,
    /// CHECK: relayer; bound into the proof via extDataHash, recorded in the Intent.
    pub relayer: SystemAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}
