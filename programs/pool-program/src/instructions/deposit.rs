use anchor_lang::prelude::*;
use anchor_lang::system_program;

use crate::state::Pool;
use crate::PoolError;

pub fn handler(ctx: Context<Deposit>, commitment: [u8; 32], amount: u64) -> Result<()> {
    require!(amount > 0, PoolError::ZeroDeposit);
    let denomination = ctx.accounts.pool.load()?.denomination;
    require!(amount == denomination, PoolError::WrongDenomination);
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
