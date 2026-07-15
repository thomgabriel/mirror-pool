use anchor_lang::prelude::*;
use anchor_lang::system_program;

pub mod merkle;
pub mod poseidon;
pub mod roots;
pub mod state;

use crate::merkle::{empty_root, zeros};
use crate::state::Pool;

declare_id!("7oHnDkpPbhPacDfqzF38caM3eo1Xo7cBmFugNXJurnn3");

#[program]
pub mod pool_program {
    use super::*;

    pub fn initialize_pool(ctx: Context<InitializePool>) -> Result<()> {
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
        let mut pool = ctx.accounts.pool.load_init()?;
        pool.mint = ctx.accounts.mint.key();
        pool.bump = ctx.bumps.pool;
        pool.vault_bump = ctx.bumps.vault;
        pool.filled_subtrees = z; // empty tree: filled subtrees == zeros
        pool.current_root = root;
        pool.roots[0] = root;
        Ok(())
    }
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

    /// CHECK: mint is used only as a PDA seed / label in this plan (no SPL logic yet).
    pub mint: UncheckedAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

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
}
