use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke_signed, system_instruction};
use anchor_lang::system_program;
use solana_stake_interface::{
    instruction as stake_instruction,
    state::{Authorized, Lockup, StakeAuthorize, StakeStateV2},
};

// Pin the hand-written host-side const to the real on-chain layout size, so the
// rent-exempt minimum and the allocation size can never silently disagree.
const _: () = assert!(crate::invariants::STAKE_ACCOUNT_SIZE == StakeStateV2::size_of());

/// The one sanctioned extension seam (CLAUDE.md): every pooled action, when a
/// round executes, produces its effect through `execute`. Adding a protocol =
/// one new impl + one new `ActionKind` variant + one dispatch arm in
/// `execute_round`. Action-specific validation happens at commit time (the ZK
/// proof), so `execute` only performs the effect.
pub trait PooledAction {
    fn execute(&self) -> Result<()>;
}

/// Pay a single withdraw intent from the vault: `denomination - fee` to the
/// recipient, `fee` to the relayer, both signed by the vault PDA.
pub struct WithdrawAction<'a, 'info> {
    pub vault: AccountInfo<'info>,
    pub recipient: AccountInfo<'info>,
    pub relayer: AccountInfo<'info>,
    pub system_program: AccountInfo<'info>,
    pub signer_seeds: &'a [&'a [&'a [u8]]],
    pub denomination: u64,
    pub fee: u64,
}

impl PooledAction for WithdrawAction<'_, '_> {
    fn execute(&self) -> Result<()> {
        let (payout, fee) = crate::invariants::split_payout(self.denomination, self.fee)?;
        if payout > 0 {
            system_program::transfer(
                CpiContext::new_with_signer(
                    self.system_program.clone(),
                    system_program::Transfer {
                        from: self.vault.clone(),
                        to: self.recipient.clone(),
                    },
                    self.signer_seeds,
                ),
                payout,
            )?;
        }
        if fee > 0 {
            system_program::transfer(
                CpiContext::new_with_signer(
                    self.system_program.clone(),
                    system_program::Transfer {
                        from: self.vault.clone(),
                        to: self.relayer.clone(),
                    },
                    self.signer_seeds,
                ),
                fee,
            )?;
        }
        Ok(())
    }
}

/// Delegate a single intent's note to the pool's validator. Ordered so the VAULT
/// acts unilaterally (the participant's key is never present at execute):
///   1. Create the stake PDA (DoS-robust), funded with `denomination - fee`
///   2. Initialize       staker = VAULT, withdrawer = recipient (a Pubkey — CPI data, not an account)
///   3. DelegateStake     vault signs as staker → validator
///   4. Authorize(Staker) vault → recipient (participant now holds both authorities)
///   5. fee → relayer
///
/// `delegated = denomination - fee - stake_rent` is what actually stakes (balance
/// above the rent reserve); `DelegateStake` enforces the network minimum.
///
/// NOTE: `recipient` is a `Pubkey`, NOT an AccountInfo — the stake authority is
/// instruction DATA to Initialize/Authorize, never a passed account, which is what
/// keeps the per-intent account count at 3 (k≈17).
pub struct StakeAction<'a, 'info> {
    pub vault: AccountInfo<'info>,
    pub stake_account: AccountInfo<'info>,
    pub recipient: Pubkey, // the stake authority (CPI data, not an account)
    pub relayer: AccountInfo<'info>,
    pub validator: AccountInfo<'info>,
    pub stake_program: AccountInfo<'info>,
    pub stake_config: AccountInfo<'info>,
    pub clock: AccountInfo<'info>,
    pub stake_history: AccountInfo<'info>,
    pub rent: AccountInfo<'info>,
    pub system_program: AccountInfo<'info>,
    pub vault_seeds: &'a [&'a [&'a [u8]]],
    pub stake_seeds: &'a [&'a [&'a [u8]]],
    pub denomination: u64,
    pub fee: u64,
    pub stake_rent: u64,
}

impl PooledAction for StakeAction<'_, '_> {
    fn execute(&self) -> Result<()> {
        // Value split (fail-closed) — total to the stake account = denomination - fee.
        let (_delegated, fee) =
            crate::invariants::stake_split(self.denomination, self.fee, self.stake_rent)?;
        let to_stake = self
            .denomination
            .checked_sub(fee)
            .ok_or(error!(crate::PoolError::FeeExceedsDenomination))?;

        // 1. Create the stake PDA — DoS-ROBUST *and amount-exact*. The PDA seed chain
        //    is PUBLIC (nullifier_hash → intent_pda → stake_pda), so an attacker can
        //    pre-fund the address. A raw `create_account` then fails "already in use"
        //    and bricks every round (liveness). Mirror Anchor's own `init` fallback —
        //    but the account must end with EXACTLY `to_stake`, not just "exist": if a
        //    pre-fund is left in place the intent delegates a different amount than the
        //    round and becomes distinguishable on-chain (privacy — see the uniformity
        //    note above the Global Constraints). So normalize the balance to `to_stake`
        //    in BOTH directions before allocate. An attacker can only ADD lamports to a
        //    system account (never allocate/assign our PDA), and while the account is
        //    still system-owned + data-empty our program can sign for it to move them.
        let existing = self.stake_account.lamports();
        let size = crate::invariants::STAKE_ACCOUNT_SIZE as u64;
        if existing == 0 {
            invoke_signed(
                &system_instruction::create_account(
                    self.vault.key,
                    self.stake_account.key,
                    to_stake,
                    size,
                    self.stake_program.key,
                ),
                &[
                    self.vault.clone(),
                    self.stake_account.clone(),
                    self.system_program.clone(),
                ],
                &[self.vault_seeds[0], self.stake_seeds[0]],
            )?;
        } else {
            // Normalize to EXACTLY to_stake before allocate (system transfer refuses a
            // data-carrying account, so this must precede allocate).
            if to_stake > existing {
                // dusted below target → vault tops up
                invoke_signed(
                    &system_instruction::transfer(
                        self.vault.key,
                        self.stake_account.key,
                        to_stake - existing,
                    ),
                    &[
                        self.vault.clone(),
                        self.stake_account.clone(),
                        self.system_program.clone(),
                    ],
                    self.vault_seeds,
                )?;
            } else if existing > to_stake {
                // pre-funded ABOVE target → sweep the excess back to the vault (the grief
                // becomes a donation), else this intent delegates more than the round.
                invoke_signed(
                    &system_instruction::transfer(
                        self.stake_account.key,
                        self.vault.key,
                        existing - to_stake,
                    ),
                    &[
                        self.stake_account.clone(),
                        self.vault.clone(),
                        self.system_program.clone(),
                    ],
                    &[self.stake_seeds[0]],
                )?;
            }
            invoke_signed(
                &system_instruction::allocate(self.stake_account.key, size),
                &[self.stake_account.clone(), self.system_program.clone()],
                &[self.stake_seeds[0]],
            )?;
            invoke_signed(
                &system_instruction::assign(self.stake_account.key, self.stake_program.key),
                &[self.stake_account.clone(), self.system_program.clone()],
                &[self.stake_seeds[0]],
            )?;
        }

        // 2. Initialize: staker = VAULT, withdrawer = participant (both are Pubkeys/data).
        let authorized = Authorized {
            staker: *self.vault.key,
            withdrawer: self.recipient,
        };
        invoke_signed(
            &stake_instruction::initialize(self.stake_account.key, &authorized, &Lockup::default()),
            &[self.stake_account.clone(), self.rent.clone()],
            &[self.stake_seeds[0]],
        )?;

        // 3. Delegate — the VAULT signs as the staker authority.
        invoke_signed(
            &stake_instruction::delegate_stake(
                self.stake_account.key,
                self.vault.key,
                self.validator.key,
            ),
            &[
                self.stake_account.clone(),
                self.validator.clone(),
                self.clock.clone(),
                self.stake_history.clone(),
                self.stake_config.clone(),
                self.vault.clone(),
            ],
            &[self.vault_seeds[0]],
        )?;

        // 4. Hand the staker authority to the participant.
        invoke_signed(
            &stake_instruction::authorize(
                self.stake_account.key,
                self.vault.key,
                &self.recipient,
                StakeAuthorize::Staker,
                None,
            ),
            &[
                self.stake_account.clone(),
                self.clock.clone(),
                self.vault.clone(),
            ],
            &[self.vault_seeds[0]],
        )?;

        // 5. Fee → relayer (from the vault).
        if fee > 0 {
            invoke_signed(
                &system_instruction::transfer(self.vault.key, self.relayer.key, fee),
                &[
                    self.vault.clone(),
                    self.relayer.clone(),
                    self.system_program.clone(),
                ],
                self.vault_seeds,
            )?;
        }
        Ok(())
    }
}
