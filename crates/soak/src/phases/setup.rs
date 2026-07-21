//! Setup: a real, delegable validator vote account, then two pools — a
//! withdraw pool and a stake pool — both `initialize_pool`d through the SDK's
//! public builder so the round phases have somewhere to deposit into.

use sdk::{build_initialize_pool_ix, round_pda};
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction, system_program,
    vote::{
        instruction::{create_account_with_config, CreateVoteAccountConfig},
        state::{VoteInit, VoteStateVersions},
    },
};

use crate::rpc::{send_ixs, Ctx, SoakError, SoakResult};

/// Shared with `phases::withdraw_round`, which needs the same values to build
/// `deposit`/`commit_intent` instructions against this pool.
///
/// Every fee here must clear the real network's rent-exempt minimum for a
/// brand-new 0-byte System account (~890_880 lamports on this validator,
/// confirmed via `solana rent 0`) — LIVE-BANK-DISCOVERED, not visible under
/// LiteSVM: `execute_round` pays `fee` lamports directly into a fresh
/// keypair, and a live bank enforces every touched account end at 0 or above
/// rent-exemption, unlike LiteSVM, which lets `crates/sdk/tests/e2e.rs`'s much
/// smaller `FEE = 1_000` slide. `WITHDRAW_FEE = 1_000_000` and
/// `WITHDRAW_DENOMINATION - WITHDRAW_FEE = 99_000_000` are chosen to clear
/// that floor on BOTH legs while staying two visibly distinct amount classes
/// in A3's evidence (99M vs 1M) — a 50/50 split would still clear the floor
/// but collapses the payout table into one indistinguishable class.
pub(crate) const WITHDRAW_DENOMINATION: u64 = 100_000_000;
pub(crate) const WITHDRAW_FEE: u64 = 1_000_000;
const WITHDRAW_K_FLOOR: u16 = 2;
const STAKE_K_FLOOR: u16 = 2;
/// Same live-bank rent-exemption floor as `WITHDRAW_FEE` above — the relayer
/// leg of a stake round's payout is a fresh System account too.
pub(crate) const SOAK_STAKE_FEE: u64 = 1_000_000;
/// Slack above delegated + rent + fee (mirrors
/// `programs/pool-program/tests/round_support.rs::stake_pool_denomination`).
const STAKE_HEADROOM: u64 = 1_000_000;

pub struct SetupOut {
    pub vote_account: Pubkey,
    pub withdraw_pool: Pubkey,
    pub withdraw_vault: Pubkey,
    pub stake_pool: Pubkey,
    pub stake_vault: Pubkey,
    pub mints: (Pubkey, Pubkey),
    pub stake_denomination: u64,
}

pub fn run(ctx: &Ctx) -> SoakResult<SetupOut> {
    let vote_account = create_validator_vote_account(ctx)?;

    // A real random key, not `Pubkey::new_unique()` — that helper is a
    // process-local counter restarting at 1 every run, which would re-derive
    // the same pool/vault PDAs (and collide with prior state) if this binary
    // is re-run against a validator that wasn't freshly `--reset`.
    let withdraw_mint = Keypair::new().pubkey();
    let (withdraw_pool, _) =
        Pubkey::find_program_address(&[b"pool", withdraw_mint.as_ref()], &pool_program::ID);
    let (withdraw_vault, _) =
        Pubkey::find_program_address(&[b"vault", withdraw_pool.as_ref()], &pool_program::ID);
    let init_withdraw = build_initialize_pool_ix(
        withdraw_pool,
        withdraw_vault,
        round_pda(withdraw_pool, 0),
        withdraw_mint,
        ctx.operator.pubkey(),
        WITHDRAW_DENOMINATION,
        WITHDRAW_K_FLOOR,
        0,
        Pubkey::default(),
        WITHDRAW_FEE,
    );
    send_ixs(
        ctx,
        "setup: initialize_pool(withdraw)",
        &[init_withdraw],
        &[&ctx.operator],
    )?;

    let stake_rent = ctx
        .client
        .get_minimum_balance_for_rent_exemption(pool_program::invariants::STAKE_ACCOUNT_SIZE)
        .map_err(|e| {
            SoakError::new(format!(
                "setup: get_minimum_balance_for_rent_exemption(STAKE_ACCOUNT_SIZE): {e}"
            ))
        })?;
    let stake_denomination = pool_program::invariants::MIN_STAKE_DELEGATION
        + stake_rent
        + SOAK_STAKE_FEE
        + STAKE_HEADROOM;

    let stake_mint = Keypair::new().pubkey();
    let (stake_pool, _) =
        Pubkey::find_program_address(&[b"pool", stake_mint.as_ref()], &pool_program::ID);
    let (stake_vault, _) =
        Pubkey::find_program_address(&[b"vault", stake_pool.as_ref()], &pool_program::ID);
    let init_stake = build_initialize_pool_ix(
        stake_pool,
        stake_vault,
        round_pda(stake_pool, 0),
        stake_mint,
        ctx.operator.pubkey(),
        stake_denomination,
        STAKE_K_FLOOR,
        1,
        vote_account,
        SOAK_STAKE_FEE,
    );
    send_ixs(
        ctx,
        "setup: initialize_pool(stake)",
        &[init_stake],
        &[&ctx.operator],
    )?;

    Ok(SetupOut {
        vote_account,
        withdraw_pool,
        withdraw_vault,
        stake_pool,
        stake_vault,
        mints: (withdraw_mint, stake_mint),
        stake_denomination,
    })
}

/// RPC port of `crates/sdk/tests/e2e.rs::create_validator_vote_account`: a
/// funded node identity plus a Vote-program-owned account initialized via the
/// real `CreateAccount` + `InitializeAccount` CPI pair, so `DelegateStake`
/// accepts it exactly as it would on a live cluster.
fn create_validator_vote_account(ctx: &Ctx) -> SoakResult<Pubkey> {
    let node = Keypair::new();
    let vote_account = Keypair::new();
    let vote_space = VoteStateVersions::vote_state_size_of(true) as u64;

    let node_rent = ctx
        .client
        .get_minimum_balance_for_rent_exemption(0)
        .map_err(|e| {
            SoakError::new(format!(
                "setup: get_minimum_balance_for_rent_exemption(node identity): {e}"
            ))
        })?;
    let vote_rent = ctx
        .client
        .get_minimum_balance_for_rent_exemption(vote_space as usize)
        .map_err(|e| {
            SoakError::new(format!(
                "setup: get_minimum_balance_for_rent_exemption(vote account): {e}"
            ))
        })?;

    let mut instructions = vec![system_instruction::create_account(
        &ctx.operator.pubkey(),
        &node.pubkey(),
        node_rent,
        0,
        &system_program::ID,
    )];
    instructions.extend(create_account_with_config(
        &ctx.operator.pubkey(),
        &vote_account.pubkey(),
        &VoteInit {
            node_pubkey: node.pubkey(),
            authorized_voter: node.pubkey(),
            authorized_withdrawer: node.pubkey(),
            commission: 0,
        },
        vote_rent,
        CreateVoteAccountConfig {
            space: vote_space,
            ..Default::default()
        },
    ));

    send_ixs(
        ctx,
        "setup: create_validator_vote_account",
        &instructions,
        &[&ctx.operator, &node, &vote_account],
    )?;
    Ok(vote_account.pubkey())
}
