use anchor_lang::prelude::*;

pub mod poseidon;

// Overwritten by `anchor keys sync` in Step 4 with the generated keypair's pubkey.
declare_id!("7oHnDkpPbhPacDfqzF38caM3eo1Xo7cBmFugNXJurnn3");

#[program]
pub mod pool_program {
    use super::*;

    pub fn ping(_ctx: Context<Ping>) -> Result<()> {
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Ping<'info> {
    pub signer: Signer<'info>,
}
