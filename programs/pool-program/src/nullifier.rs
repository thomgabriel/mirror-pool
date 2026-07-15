use anchor_lang::prelude::*;

/// Existence of this PDA at seeds ["nullifier", pool, nullifier_hash] means the
/// nullifier has been spent. `spent` is a readability aid — the security property
/// is the PDA's existence.
#[account]
pub struct NullifierRecord {
    pub spent: bool,
}
