use anchor_lang::prelude::*;
use anchor_lang::system_program;

use crate::merkle::{empty_root, zeros};
use crate::state::Pool;
use crate::PoolError;

pub fn handler(
    ctx: Context<InitializePool>,
    denomination: u64,
    k_floor: u16,
    action_kind: u8,
    validator: Pubkey,
    fee: u64,
) -> Result<()> {
    require!(
        k_floor >= crate::round::MIN_K_FLOOR,
        PoolError::KFloorTooLow
    );
    // Withdraw pools carry no validator and a fee ≤ denomination; stake pools name a validator and clear the delegation floor.
    match action_kind {
        0 => {
            require!(
                k_floor <= crate::invariants::max_k(crate::round::ActionKind::Withdraw),
                PoolError::KFloorTooHigh
            );
            require!(validator == Pubkey::default(), PoolError::WrongActionConfig);
            require!(fee <= denomination, PoolError::FeeExceedsDenomination);
        }
        1 => {
            require!(
                k_floor <= crate::invariants::max_k(crate::round::ActionKind::Stake),
                PoolError::KFloorTooHigh
            );
            require!(validator != Pubkey::default(), PoolError::WrongActionConfig);
            let stake_rent = Rent::get()?.minimum_balance(crate::invariants::STAKE_ACCOUNT_SIZE);
            // Fails closed if denomination can't cover fee + rent + min delegation.
            crate::invariants::stake_split(denomination, fee, stake_rent)?;
        }
        _ => return err!(PoolError::WrongActionConfig),
    }

    let z = zeros().map_err(|_| error!(PoolError::MerkleInit))?;
    let root = empty_root(&z).map_err(|_| error!(PoolError::MerkleInit))?;

    // Fund the vault to the rent-exempt minimum so custody funds are never at rent risk.
    let rent_min = Rent::get()?.minimum_balance(0);
    system_program::transfer(
        CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.payer.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
            },
        ),
        rent_min,
    )?;

    // `load_init` hands back a `RefMut` directly over the account's own backing
    // bytes (no Borsh copy of the ~3.9 KB struct onto the stack); the `init`
    // constraint zero-fills the account, so only the non-zero fields are set.
    {
        let mut pool = ctx.accounts.pool.load_init()?;
        pool.mint = ctx.accounts.mint.key();
        pool.denomination = denomination;
        pool.k_floor = k_floor;
        pool.current_round_id = 0;
        pool.action_kind = action_kind;
        pool.validator = validator;
        pool.fee = fee;
        pool.bump = ctx.bumps.pool;
        pool.vault_bump = ctx.bumps.vault;
        pool.filled_subtrees = z; // empty tree: filled subtrees == zeros
        pool.current_root = root;
        pool.roots[0] = root;
    }

    let round = &mut ctx.accounts.round;
    round.state = crate::round::RoundState::Open;
    round.intent_count = 0;
    Ok(())
}

#[derive(Accounts)]
pub struct InitializePool<'info> {
    #[account(
        init,
        payer = payer,
        space = Pool::SPACE,
        seeds = [b"pool", mint.key().as_ref()],
        bump
    )]
    pub pool: AccountLoader<'info, Pool>,

    /// CHECK: SOL vault PDA (system-owned); only holds lamports.
    #[account(
        mut,
        seeds = [b"vault", pool.key().as_ref()],
        bump
    )]
    pub vault: UncheckedAccount<'info>,

    #[account(
        init,
        payer = payer,
        space = crate::round::Round::SPACE,
        seeds = [b"round", pool.key().as_ref(), &0u64.to_le_bytes()],
        bump
    )]
    pub round: Account<'info, crate::round::Round>,

    /// CHECK: mint is used only as a PDA seed / label in this plan (no SPL logic yet).
    pub mint: UncheckedAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}
