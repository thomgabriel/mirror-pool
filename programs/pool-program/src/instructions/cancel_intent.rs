use anchor_lang::prelude::*;
use anchor_lang::system_program;

use crate::state::Pool;
use crate::PoolError;

pub fn handler(
    ctx: Context<CancelIntent>,
    _round_id: u64,
    _nullifier_hash: [u8; 32],
) -> Result<()> {
    require!(
        ctx.accounts.round.state == crate::round::RoundState::Open,
        PoolError::RoundClosed
    );

    // Timeout gate: a committed intent is uncancelable until TIMEOUT_SLOTS
    // slots after its own commit. Removes the instant commit->cancel exit and
    // makes "committed for N slots" enforced; cancel remains a linkable sub-k
    // exit by construction once the window opens (see the spec's claim list).
    let unlock = crate::invariants::cancel_unlock_slot(ctx.accounts.intent.committed_slot)?;
    require!(Clock::get()?.slot >= unlock, PoolError::CancelTooEarly);

    let (denomination, vault_bump) = {
        let pool = ctx.accounts.pool.load()?;
        (pool.denomination, pool.vault_bump)
    };

    let pool_key = ctx.accounts.pool.key();
    let vault_bump_arr = [vault_bump];
    let seeds: &[&[u8]] = &[b"vault", pool_key.as_ref(), &vault_bump_arr];
    let signer_seeds: &[&[&[u8]]] = &[seeds];

    // Return the note's deposited value to its bound recipient. The
    // nullifier PDA is intentionally NOT closed — the note stays spent, so
    // there is no double-spend; the intent PDA is closed by the `close`
    // constraint (rent back to the recipient).
    system_program::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.vault.to_account_info(),
                to: ctx.accounts.recipient.to_account_info(),
            },
            signer_seeds,
        ),
        denomination,
    )?;

    let round = &mut ctx.accounts.round;
    round.intent_count = round
        .intent_count
        .checked_sub(1)
        .ok_or(error!(PoolError::RoundOverflow))?;
    Ok(())
}

#[derive(Accounts)]
#[instruction(round_id: u64, nullifier_hash: [u8; 32])]
pub struct CancelIntent<'info> {
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

    // `close = recipient` returns the intent's rent and, with `has_one`,
    // proves the caller controls the bound recipient key (non-griefable).
    #[account(
        mut,
        close = recipient,
        seeds = [b"intent", pool.key().as_ref(), nullifier_hash.as_ref()],
        bump,
        constraint = intent.pool == pool.key() @ PoolError::IntentInvalid,
        constraint = intent.round_id == round_id @ PoolError::IntentInvalid,
        has_one = recipient @ PoolError::IntentAccountMismatch
    )]
    pub intent: Account<'info, crate::round::Intent>,

    /// CHECK: SOL vault PDA (system-owned); refunds the denomination via invoke_signed.
    #[account(
        mut,
        seeds = [b"vault", pool.key().as_ref()],
        bump = pool.load()?.vault_bump
    )]
    pub vault: UncheckedAccount<'info>,

    #[account(mut)]
    pub recipient: Signer<'info>,

    pub system_program: Program<'info, System>,
}
