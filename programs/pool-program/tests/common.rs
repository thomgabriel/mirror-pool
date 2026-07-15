#![allow(dead_code)]
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, instruction::Instruction, pubkey::Pubkey,
};

/// The generated program ID (declare_id!), read from the crate under test.
pub fn program_id() -> Pubkey {
    pool_program::ID
}

/// Absolute path to the SBF artifact. `anchor build` writes to the WORKSPACE-root
/// target/, but `cargo test -p pool-program` runs with CWD = the package dir, so a
/// relative path fails. CARGO_MANIFEST_DIR = programs/pool-program → ../../target.
pub fn so_path() -> String {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/deploy/pool_program.so"
    )
    .to_string()
}

/// Anchor instruction discriminator = sha256("global:<name>")[..8].
pub fn disc(name: &str) -> [u8; 8] {
    use solana_sdk::hash::hash;
    let h = hash(format!("global:{name}").as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h.to_bytes()[..8]);
    d
}

/// Headroom for zero-copy account access + ~20 Poseidon syscalls.
pub fn cu_limit_ix() -> Instruction {
    ComputeBudgetInstruction::set_compute_unit_limit(400_000)
}
