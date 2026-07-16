use anchor_lang::prelude::*;
use anchor_lang::system_program;

pub mod invariants;
pub mod merkle;
pub mod nullifier;
pub mod poseidon;
pub mod roots;
pub mod round;
pub mod state;
pub mod verifier;
pub mod vk;

use crate::merkle::{empty_root, zeros};
use crate::state::Pool;

declare_id!("7oHnDkpPbhPacDfqzF38caM3eo1Xo7cBmFugNXJurnn3");

#[program]
pub mod pool_program {
    use super::*;

    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        denomination: u64,
        k_floor: u16,
    ) -> Result<()> {
        require!(
            k_floor >= crate::round::MIN_K_FLOOR,
            PoolError::KFloorTooLow
        );

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

    pub fn deposit(ctx: Context<Deposit>, commitment: [u8; 32], amount: u64) -> Result<()> {
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

    pub fn withdraw(
        ctx: Context<Withdraw>,
        proof: crate::verifier::WithdrawProof,
        root: [u8; 32],
        nullifier_hash: [u8; 32],
        fee: u64,
    ) -> Result<()> {
        let (denomination, vault_bump) = {
            let pool = ctx.accounts.pool.load()?;
            require!(
                crate::roots::is_known(&pool.roots, &root),
                PoolError::UnknownRoot
            );
            require!(fee <= pool.denomination, PoolError::FeeExceedsDenomination);
            (pool.denomination, pool.vault_bump)
        };

        // extDataHash is computed from the PAYOUT ACCOUNT KEYS (the accounts that
        // actually receive lamports below) — NOT from separate instruction args —
        // so the bound pubkeys are the payout accounts by construction; no
        // redirection is possible without invalidating the proof.
        let ext = ext_data::ext_data_hash(
            &ctx.accounts.recipient.key().to_bytes(),
            &ctx.accounts.relayer.key().to_bytes(),
            fee,
        );
        crate::verifier::verify_withdraw(&proof, &[root, nullifier_hash, ext])?;

        // The nullifier PDA's `init` constraint already enforced single-spend
        // atomically (this instruction would have failed above it if the PDA
        // already existed); `spent` is a readability aid only.
        ctx.accounts.nullifier.spent = true;

        let pool_key = ctx.accounts.pool.key();
        let vault_bump_arr = [vault_bump];
        let seeds: &[&[u8]] = &[b"vault", pool_key.as_ref(), &vault_bump_arr];
        let signer_seeds: &[&[&[u8]]] = &[seeds];

        let payout = denomination - fee;
        if payout > 0 {
            system_program::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.system_program.to_account_info(),
                    system_program::Transfer {
                        from: ctx.accounts.vault.to_account_info(),
                        to: ctx.accounts.recipient.to_account_info(),
                    },
                    signer_seeds,
                ),
                payout,
            )?;
        }
        if fee > 0 {
            system_program::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.system_program.to_account_info(),
                    system_program::Transfer {
                        from: ctx.accounts.vault.to_account_info(),
                        to: ctx.accounts.relayer.to_account_info(),
                    },
                    signer_seeds,
                ),
                fee,
            )?;
        }
        Ok(())
    }

    pub fn commit_intent(
        ctx: Context<CommitIntent>,
        proof: crate::verifier::WithdrawProof,
        root: [u8; 32],
        nullifier_hash: [u8; 32],
        fee: u64,
        round_id: u64,
    ) -> Result<()> {
        {
            let pool = ctx.accounts.pool.load()?;
            require!(round_id == pool.current_round_id, PoolError::WrongRound);
            require!(
                crate::roots::is_known(&pool.roots, &root),
                PoolError::UnknownRoot
            );
            require!(fee <= pool.denomination, PoolError::FeeExceedsDenomination);
        }
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
        intent.action = crate::round::ActionKind::Withdraw;

        let round = &mut ctx.accounts.round;
        round.intent_count = round
            .intent_count
            .checked_add(1)
            .ok_or(error!(PoolError::RoundOverflow))?;
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

#[derive(Accounts)]
#[instruction(proof: crate::verifier::WithdrawProof, root: [u8; 32], nullifier_hash: [u8; 32])]
pub struct Withdraw<'info> {
    #[account(
        mut,
        seeds = [b"pool", pool.load()?.mint.as_ref()],
        bump = pool.load()?.bump
    )]
    pub pool: AccountLoader<'info, Pool>,

    /// CHECK: SOL vault PDA (system-owned); pays out lamports via `invoke_signed`.
    #[account(
        mut,
        seeds = [b"vault", pool.key().as_ref()],
        bump = pool.load()?.vault_bump
    )]
    pub vault: UncheckedAccount<'info>,

    // `init` here is the atomic single-spend guard: this transaction fails
    // outright if the PDA already exists for this `nullifier_hash`.
    #[account(
        init,
        payer = relayer,
        space = 8 + 1,
        seeds = [b"nullifier", pool.key().as_ref(), nullifier_hash.as_ref()],
        bump
    )]
    pub nullifier: Account<'info, crate::nullifier::NullifierRecord>,

    // These are the ONLY carriers of the payout destination — `ext_data_hash` is
    // computed from their keys, not from separate instruction args, so there is
    // no way to redirect funds without invalidating the proof (see `withdraw`).
    /// CHECK: payout recipient; bound into the proof via `extDataHash`, not trusted otherwise.
    #[account(mut)]
    pub recipient: SystemAccount<'info>,

    #[account(mut)]
    pub relayer: Signer<'info>,

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
    #[msg("proof bytes are malformed")]
    ProofMalformed,
    #[msg("proof failed verification")]
    ProofInvalid,
    #[msg("deposit amount must equal the pool's denomination")]
    WrongDenomination,
    #[msg("proof root is not a known recent root")]
    UnknownRoot,
    #[msg("fee must not exceed the pool's denomination")]
    FeeExceedsDenomination,
    #[msg("k_floor must be at least MIN_K_FLOOR")]
    KFloorTooLow,
    #[msg("round_id does not match the pool's current round")]
    WrongRound,
    #[msg("round is not open")]
    RoundClosed,
    #[msg("round intent count overflow")]
    RoundOverflow,
}
