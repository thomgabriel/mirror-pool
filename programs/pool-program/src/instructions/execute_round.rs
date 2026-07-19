use anchor_lang::prelude::*;

use crate::state::Pool;
use crate::PoolError;

// Named `'info` (not the bare `Context<ExecuteRound>` elision): the handler
// reads `ctx.remaining_accounts` (tied to the Context's 3rd lifetime) and
// clones its `AccountInfo`s into the same `WithdrawAction` as
// `ctx.accounts.vault`'s `AccountInfo`, so both must unify with `'info` for
// the compiler to accept it (`AccountInfo<'info>` is invariant over
// `'info`) — a syntax requirement, not a logic change.
pub fn handler<'info>(
    ctx: Context<'_, '_, 'info, 'info, ExecuteRound<'info>>,
    round_id: u64,
) -> Result<()> {
    let (denomination, vault_bump, k_floor, current_round_id, action_kind, validator, fee) = {
        let pool = ctx.accounts.pool.load()?;
        (
            pool.denomination,
            pool.vault_bump,
            pool.k_floor,
            pool.current_round_id,
            pool.action_kind,
            pool.validator,
            pool.fee,
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
                let intent: Account<crate::round::Intent> =
                    Account::try_from(intent_ai).map_err(|_| error!(PoolError::IntentInvalid))?;
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

                // Defense-in-depth: fee was fixed at commit (== pool.fee), so payouts are
                // uniform across the round. Re-assert so a stale/forged intent can't slip a
                // non-uniform amount into the batch — uniformity is enforced IDENTICALLY on
                // every PooledAction adapter, withdraw included.
                require!(intent.fee == fee, PoolError::FeeNotUniform);

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
            let stake_rent = Rent::get()?.minimum_balance(crate::invariants::STAKE_ACCOUNT_SIZE);

            let mut seen: Vec<Pubkey> = Vec::with_capacity(count as usize);
            for i in 0..(count as usize) {
                let intent_ai = &rem[i * 3];
                let stake_ai = &rem[i * 3 + 1];
                let relayer_ai = &rem[i * 3 + 2];

                let intent: Account<crate::round::Intent> =
                    Account::try_from(intent_ai).map_err(|_| error!(PoolError::IntentInvalid))?;
                require_keys_eq!(intent.pool, pool_key, PoolError::IntentInvalid);
                require!(intent.round_id == round_id, PoolError::IntentInvalid);
                require!(!seen.contains(intent_ai.key), PoolError::DuplicateIntent);
                seen.push(*intent_ai.key);
                // Defense-in-depth: fee was fixed at commit (== pool.fee), so
                // the delegated amounts are uniform. Re-assert so a stale/forged
                // intent can't slip a non-uniform amount into the batch.
                require!(intent.fee == fee, PoolError::FeeNotUniform);
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
