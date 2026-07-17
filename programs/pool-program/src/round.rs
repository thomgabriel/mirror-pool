use anchor_lang::prelude::*;

/// A pool with `k_floor < 2` provides no anonymity; reject it at init.
pub const MIN_K_FLOOR: u16 = 2;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoundState {
    Open,
    Executed,
}

/// One accumulation window. `intent_count` is the authoritative number of
/// live intents committed to this round (incremented at commit, decremented
/// at cancel); `execute_round` checks it against the k-floor.
#[account]
pub struct Round {
    pub state: RoundState,
    pub intent_count: u32,
}

impl Round {
    pub const SPACE: usize = 8 + 1 + 4;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActionKind {
    Withdraw,
    Stake,
}

/// A committed intent: the note is already spent (its nullifier PDA exists);
/// `recipient`/`relayer` were bound into the proof via `extDataHash`, so
/// `execute_round` pays exactly these keys. `pool`/`round_id` bind the intent
/// to its pool and round, closing cross-pool / cross-round reuse.
#[account]
pub struct Intent {
    pub pool: Pubkey,
    pub round_id: u64,
    pub recipient: Pubkey,
    pub relayer: Pubkey,
    pub fee: u64,
    pub action: ActionKind,
    pub committed_slot: u64,
}

impl Intent {
    pub const SPACE: usize = 8 + 32 + 8 + 32 + 32 + 8 + 1 + 8;
}
