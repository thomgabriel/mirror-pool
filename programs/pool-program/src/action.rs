use anchor_lang::prelude::*;
use anchor_lang::system_program;

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
