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

    pub fn deposit(ctx: Context<Deposit>, commitment: [u8; 32], amount: u64) -> Result<()> {
        require!(amount > 0, PoolError::ZeroDeposit);
        require!(
            crate::poseidon::is_in_field(&commitment),
            PoolError::CommitmentNotInField
        );

        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.payer.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                },
            ),
            amount,
        )?;

        // Mutate through the loader's RefMut in place — never copy the ~3.9 KB
        // `Pool` by value (see state.rs doc comment on why zero_copy exists).
        let mut pool = ctx.accounts.pool.load_mut()?;
        let leaf_index = pool.insert_commitment(commitment).map_err(|e| match e {
            crate::merkle::MerkleError::TreeFull => error!(PoolError::TreeFull),
            crate::merkle::MerkleError::NotInField => error!(PoolError::CommitmentNotInField),
            crate::merkle::MerkleError::Hash => error!(PoolError::MerkleInit),
        })?;
        let new_root = pool.current_root;
        pool.push_root(new_root);
        drop(pool);

        emit!(DepositEvent {
            commitment,
            leaf_index,
            new_root,
        });
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

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(
        mut,
        seeds = [b"pool", pool.load()?.mint.as_ref()],
        bump = pool.load()?.bump
    )]
    pub pool: AccountLoader<'info, Pool>,

    /// CHECK: SOL vault PDA (system-owned); receives lamports.
    #[account(
        mut,
        seeds = [b"vault", pool.key().as_ref()],
        bump = pool.load()?.vault_bump
    )]
    pub vault: UncheckedAccount<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[event]
pub struct DepositEvent {
    pub commitment: [u8; 32],
    pub leaf_index: u32,
    pub new_root: [u8; 32],
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
