use anchor_lang::prelude::*;
use anchor_lang::system_program;

pub mod action;
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
        action_kind: u8,
        validator: Pubkey,
        stake_fee: u64,
    ) -> Result<()> {
        require!(
            k_floor >= crate::round::MIN_K_FLOOR,
            PoolError::KFloorTooLow
        );
        // Validate the action config. Withdraw pools carry no stake params; stake
        // pools must name a validator and clear the delegation floor.
        match action_kind {
            0 => require!(
                validator == Pubkey::default() && stake_fee == 0,
                PoolError::WrongActionConfig
            ),
            1 => {
                require!(validator != Pubkey::default(), PoolError::WrongActionConfig);
                let stake_rent =
                    Rent::get()?.minimum_balance(crate::invariants::STAKE_ACCOUNT_SIZE);
                // Fails closed if denomination can't cover fee + rent + min delegation.
                crate::invariants::stake_split(denomination, stake_fee, stake_rent)?;
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
            pool.stake_fee = stake_fee;
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
            // Stake pools require a uniform, pool-fixed fee (privacy + liveness — see note).
            if pool.action_kind == 1 {
                require!(fee == pool.stake_fee, PoolError::WrongActionConfig);
            }
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
        require!(
            ctx.accounts.round.state == crate::round::RoundState::Open,
            PoolError::RoundClosed
        );

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

    // Named `'info` (not the bare `Context<ExecuteRound>` elision): the handler
    // reads `ctx.remaining_accounts` (tied to the Context's 3rd lifetime) and
    // clones its `AccountInfo`s into the same `WithdrawAction` as
    // `ctx.accounts.vault`'s `AccountInfo`, so both must unify with `'info` for
    // the compiler to accept it (`AccountInfo<'info>` is invariant over
    // `'info`) — a syntax requirement, not a logic change.
    pub fn execute_round<'info>(
        ctx: Context<'_, '_, 'info, 'info, ExecuteRound<'info>>,
        round_id: u64,
    ) -> Result<()> {
        let (
            denomination,
            vault_bump,
            k_floor,
            current_round_id,
            action_kind,
            validator,
            stake_fee,
        ) = {
            let pool = ctx.accounts.pool.load()?;
            (
                pool.denomination,
                pool.vault_bump,
                pool.k_floor,
                pool.current_round_id,
                pool.action_kind,
                pool.validator,
                pool.stake_fee,
            )
        };
        // Re-execution and stale/future round_id are impossible by construction —
        // an explicit `round.state`/`round_id` check here would be UNREACHABLE dead
        // code (CLAUDE.md forbids it), because the account constraints ARE the guard:
        //   * `next_round` is `init` at seeds ["round", pool, round_id+1]; once this
        //     round has executed, round_id+1 already exists → its init fails "already
        //     in use" atomically, so a round executes at most once.
        //   * a future/non-existent `round_id` fails Anchor's `round` account load
        //     before the handler body runs.
        // Rounds are created strictly sequentially (Round(0) at init, Round(N+1) at
        // execute(N)), so the ONLY reachable path here has `round_id == current_round_id`;
        // the `current_round_id + 1` bump below is therefore correct.
        let count = ctx.accounts.round.intent_count;
        require!(
            crate::invariants::meets_k_floor(count, k_floor),
            PoolError::KFloorNotMet
        );

        let pool_key = ctx.accounts.pool.key();
        let vault_bump_arr = [vault_bump];
        let seeds: &[&[u8]] = &[b"vault", pool_key.as_ref(), &vault_bump_arr];
        let signer_seeds: &[&[&[u8]]] = &[seeds];

        // Dispatch is by `pool.action_kind`, NOT `intent.action` — a pool is a
        // single action kind, so the pool config alone selects the effect.
        let rem = ctx.remaining_accounts;
        match action_kind {
            0 => {
                // WITHDRAW: [intent, recipient, relayer] x count.
                require!(
                    rem.len() == (count as usize) * 3,
                    PoolError::IntentAccountsMismatch
                );

                let mut seen: Vec<Pubkey> = Vec::with_capacity(count as usize);
                for i in 0..(count as usize) {
                    let intent_ai = &rem[i * 3];
                    let recipient_ai = &rem[i * 3 + 1];
                    let relayer_ai = &rem[i * 3 + 2];

                    // Owner + discriminator checked by `try_from`; `pool`/`round_id`
                    // bind it to THIS pool and round (closes cross-pool / cross-round
                    // reuse); uniqueness closes duplicate-padding.
                    let intent: Account<crate::round::Intent> = Account::try_from(intent_ai)
                        .map_err(|_| error!(PoolError::IntentInvalid))?;
                    require_keys_eq!(intent.pool, pool_key, PoolError::IntentInvalid);
                    require!(intent.round_id == round_id, PoolError::IntentInvalid);
                    require!(!seen.contains(intent_ai.key), PoolError::DuplicateIntent);
                    seen.push(*intent_ai.key);
                    require_keys_eq!(
                        *recipient_ai.key,
                        intent.recipient,
                        PoolError::IntentAccountMismatch
                    );
                    require_keys_eq!(
                        *relayer_ai.key,
                        intent.relayer,
                        PoolError::IntentAccountMismatch
                    );

                    let action = crate::action::WithdrawAction {
                        vault: ctx.accounts.vault.to_account_info(),
                        recipient: recipient_ai.clone(),
                        relayer: relayer_ai.clone(),
                        system_program: ctx.accounts.system_program.to_account_info(),
                        signer_seeds,
                        denomination,
                        fee: intent.fee,
                    };
                    crate::action::PooledAction::execute(&action)?;
                }
            }
            1 => {
                // STAKE: [intent, stake_account, relayer] x count, then the shared
                // tail [validator, stake_program, stake_config, clock, stake_history, rent].
                const TAIL: usize = 6;
                require!(
                    rem.len() == (count as usize) * 3 + TAIL,
                    PoolError::IntentAccountsMismatch
                );
                let tail = &rem[(count as usize) * 3..];
                let (validator_ai, stake_prog, stake_config, clock, stake_history, rent_ai) =
                    (&tail[0], &tail[1], &tail[2], &tail[3], &tail[4], &tail[5]);
                require_keys_eq!(*validator_ai.key, validator, PoolError::StakeAccountInvalid);
                let stake_rent =
                    Rent::get()?.minimum_balance(crate::invariants::STAKE_ACCOUNT_SIZE);

                let mut seen: Vec<Pubkey> = Vec::with_capacity(count as usize);
                for i in 0..(count as usize) {
                    let intent_ai = &rem[i * 3];
                    let stake_ai = &rem[i * 3 + 1];
                    let relayer_ai = &rem[i * 3 + 2];

                    let intent: Account<crate::round::Intent> = Account::try_from(intent_ai)
                        .map_err(|_| error!(PoolError::IntentInvalid))?;
                    require_keys_eq!(intent.pool, pool_key, PoolError::IntentInvalid);
                    require!(intent.round_id == round_id, PoolError::IntentInvalid);
                    require!(!seen.contains(intent_ai.key), PoolError::DuplicateIntent);
                    seen.push(*intent_ai.key);
                    // Defense-in-depth: fee was fixed at commit (== pool.stake_fee), so
                    // the delegated amounts are uniform. Re-assert so a stale/forged
                    // intent can't slip a non-uniform amount into the batch.
                    require!(intent.fee == stake_fee, PoolError::WrongActionConfig);
                    require_keys_eq!(
                        *relayer_ai.key,
                        intent.relayer,
                        PoolError::IntentAccountMismatch
                    );

                    // The stake account is the intent's canonical PDA, seeded off the
                    // INTENT PDA key (itself ["intent", pool, nullifier_hash]) — no
                    // nullifier_hash field on Intent, no struct change, no rent on
                    // withdraw intents.
                    let (expected_stake, stake_bump) = Pubkey::find_program_address(
                        &[b"stake", pool_key.as_ref(), intent_ai.key.as_ref()],
                        &crate::ID,
                    );
                    require_keys_eq!(
                        *stake_ai.key,
                        expected_stake,
                        PoolError::StakeAccountInvalid
                    );

                    let stake_bump_arr = [stake_bump];
                    let stake_seed_refs: &[&[u8]] = &[
                        b"stake",
                        pool_key.as_ref(),
                        intent_ai.key.as_ref(),
                        &stake_bump_arr,
                    ];
                    let stake_seeds: &[&[&[u8]]] = &[stake_seed_refs];

                    let action = crate::action::StakeAction {
                        vault: ctx.accounts.vault.to_account_info(),
                        stake_account: stake_ai.clone(),
                        recipient: intent.recipient, // a Pubkey — CPI data, not an account
                        relayer: relayer_ai.clone(),
                        validator: validator_ai.clone(),
                        stake_program: stake_prog.clone(),
                        stake_config: stake_config.clone(),
                        clock: clock.clone(),
                        stake_history: stake_history.clone(),
                        rent: rent_ai.clone(),
                        system_program: ctx.accounts.system_program.to_account_info(),
                        vault_seeds: signer_seeds,
                        stake_seeds,
                        denomination,
                        fee: intent.fee,
                        stake_rent,
                    };
                    crate::action::PooledAction::execute(&action)?;
                }
            }
            _ => return err!(PoolError::WrongActionConfig),
        }

        ctx.accounts.round.state = crate::round::RoundState::Executed;
        {
            let mut pool = ctx.accounts.pool.load_mut()?;
            pool.current_round_id = current_round_id
                .checked_add(1)
                .ok_or(error!(PoolError::RoundOverflow))?;
        }
        let next = &mut ctx.accounts.next_round;
        next.state = crate::round::RoundState::Open;
        next.intent_count = 0;
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

#[derive(Accounts)]
#[instruction(round_id: u64)]
pub struct ExecuteRound<'info> {
    #[account(
        mut,
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
        payer = cranker,
        space = crate::round::Round::SPACE,
        seeds = [b"round", pool.key().as_ref(), &(round_id + 1).to_le_bytes()],
        bump
    )]
    pub next_round: Account<'info, crate::round::Round>,

    /// CHECK: SOL vault PDA (system-owned); pays out the batch via invoke_signed.
    #[account(
        mut,
        seeds = [b"vault", pool.key().as_ref()],
        bump = pool.load()?.vault_bump
    )]
    pub vault: UncheckedAccount<'info>,

    #[account(mut)]
    pub cranker: Signer<'info>,
    pub system_program: Program<'info, System>,
    // remaining_accounts, by `pool.action_kind`:
    //   0 (Withdraw): [intent, recipient, relayer] × intent_count
    //   1 (Stake):    [intent, stake_account, relayer] × intent_count, then the
    //                 shared tail [validator, stake_program, stake_config, clock,
    //                 stake_history, rent]
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
    #[msg("round has fewer intents than the k-floor")]
    KFloorNotMet,
    #[msg("wrong number of intent accounts for this round")]
    IntentAccountsMismatch,
    #[msg("intent account does not belong to this pool/round")]
    IntentInvalid,
    #[msg("payout account does not match the recorded intent")]
    IntentAccountMismatch,
    #[msg("duplicate intent account in the batch")]
    DuplicateIntent,
    #[msg("action_kind/validator/stake_fee configuration is invalid for this pool")]
    WrongActionConfig,
    #[msg("stake pool denomination is too low to cover fee + rent + minimum delegation")]
    StakeDenominationTooLow,
    #[msg("account does not match the pool's configured stake action")]
    StakeAccountInvalid,
}
