#[test]
fn program_id_is_set_to_generated_keypair() {
    // After `anchor keys sync`, declare_id! holds the generated (non-zero) pubkey.
    assert_ne!(
        pool_program::ID.to_bytes(),
        [0u8; 32],
        "run `anchor keys sync` so declare_id! is the generated program keypair"
    );
}
