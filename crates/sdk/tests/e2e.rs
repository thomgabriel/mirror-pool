//! End-to-end LiteSVM round trip driven THROUGH the SDK: initialize_pool ->
//! deposit(k) -> commit_intent(k) -> execute_round. Proves the SDK's
//! MerkleTree/proof/instruction builders agree with the on-chain program.
//!
//! `programs/pool-program/tests/execute_round.rs` (Tasks 3-4) already proves
//! the on-chain program's guards against hand-built instructions; this test
//! is the load-bearing proof that a real client, calling only the SDK's
//! public builders, gets a working deposit -> commit -> execute round trip
//! for a full k=2 round: circuit <-> prover <-> on-chain verifier <-> SDK all
//! agree.

use litesvm::LiteSVM;
use sdk::{
    build_commit_intent_ix, build_deposit_ix, build_execute_round_ix, build_initialize_pool_ix,
    round_pda, MerkleTree, Note, WithdrawArtifacts,
};
use solana_sdk::{
    account::ReadableAccount,
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const DENOMINATION: u64 = 2_000_000;
const FEE: u64 = 1_000;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/sdk is two levels below the workspace root")
        .to_path_buf()
}

fn so_path() -> String {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/deploy/pool_program.so"
    )
    .to_string()
}

/// Mirrors `programs/pool-program/tests/round_support.rs`'s / `crates/sdk/tests/sdk.rs`'s
/// build guard — the `circuits/build/*` artifacts are gitignored outputs of
/// `circuits/scripts/setup.sh`; generate them rather than skip the real
/// prove/verify this test exists to exercise.
fn ensure_build_artifacts() -> PathBuf {
    static BUILD_DIR: OnceLock<PathBuf> = OnceLock::new();
    BUILD_DIR
        .get_or_init(|| {
            let circuits_dir = workspace_root().join("circuits");
            let build_dir = circuits_dir.join("build");
            let wasm = build_dir.join("withdraw_js").join("withdraw.wasm");
            let r1cs = build_dir.join("withdraw.r1cs");
            let zkey = build_dir.join("withdraw.zkey");
            let vk = build_dir.join("verification_key.json");
            let required = [&wasm, &r1cs, &zkey, &vk];

            if !required.iter().all(|p| p.exists()) {
                eprintln!(
                    "circuits/build artifacts missing — running circuits/scripts/setup.sh ..."
                );
                let status = std::process::Command::new("bash")
                    .arg(circuits_dir.join("scripts").join("setup.sh"))
                    .status();
                let setup_ran = matches!(status, Ok(s) if s.success());
                if !setup_ran || !required.iter().all(|p| p.exists()) {
                    panic!(
                        "circuits/build artifacts are missing and `circuits/scripts/setup.sh` did not \
                         produce them (it needs `circom` and `npx`/`snarkjs` on PATH). \
                         Run circuits/scripts/setup.sh first, then re-run `cargo test -p sdk --test e2e`."
                    );
                }
            }
            build_dir
        })
        .clone()
}

fn send(svm: &mut LiteSVM, payer: &Keypair, signers: &[&Keypair], ix: Instruction) {
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&payer.pubkey()),
    );
    svm.send_transaction(Transaction::new(signers, msg, svm.latest_blockhash()))
        .unwrap();
}

#[test]
fn sdk_driven_round_trip_two_intents() {
    let build_dir = ensure_build_artifacts();
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.add_program_from_file(pool_program::ID, so_path())
        .unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &pool_program::ID);
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &pool_program::ID);

    // init(k_floor=2)
    let init = build_initialize_pool_ix(
        pool,
        vault,
        round_pda(pool, 0),
        mint,
        payer.pubkey(),
        DENOMINATION,
        2,
    );
    send(&mut svm, &payer, &[&payer], init);

    // two notes -> deposit both
    let notes = [Note::new(), Note::new()];
    let mut tree = MerkleTree::new().unwrap();
    for note in &notes {
        tree.insert(note.commitment());
        send(
            &mut svm,
            &payer,
            &[&payer],
            build_deposit_ix(pool, vault, payer.pubkey(), note.commitment(), DENOMINATION),
        );
    }
    let root = tree.root();

    // Sanity: the deposited tree's root (via SDK deposit ixs) must match the
    // on-chain pool's current_root — the load-bearing precondition for the
    // proofs generated below (against this same `root`) to verify at all.
    let offset = 8 + core::mem::offset_of!(pool_program::state::Pool, current_root);
    let current_root: [u8; 32] = svm.get_account(&pool).unwrap().data()[offset..offset + 32]
        .try_into()
        .unwrap();
    assert_eq!(
        current_root, root,
        "SDK-computed tree root must match the on-chain pool's current_root"
    );

    // commit both
    let mut triples = Vec::new();
    for (i, note) in notes.iter().enumerate() {
        let recipient = Pubkey::new_unique();
        let relayer = Pubkey::new_unique();
        let path = tree.authentication_path(i);
        let build = build_commit_intent_ix(
            pool,
            round_pda(pool, 0),
            recipient,
            relayer,
            payer.pubkey(),
            note,
            &path,
            root,
            FEE,
            0,
            WithdrawArtifacts {
                wasm_path: &build_dir.join("withdraw_js").join("withdraw.wasm"),
                r1cs_path: &build_dir.join("withdraw.r1cs"),
                zkey_path: &build_dir.join("withdraw.zkey"),
            },
        )
        .expect("proving a fresh SDK-generated note must succeed");
        let (intent, _) = Pubkey::find_program_address(
            &[
                b"intent",
                pool.as_ref(),
                build.public_inputs.nullifier_hash.as_ref(),
            ],
            &pool_program::ID,
        );
        svm.expire_blockhash();
        send(&mut svm, &payer, &[&payer], build.instruction);
        triples.push((intent, recipient, relayer));
    }

    // execute
    let cranker = Keypair::new();
    svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    svm.expire_blockhash();
    let exec = build_execute_round_ix(pool, vault, cranker.pubkey(), 0, &triples);
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            exec,
        ],
        Some(&cranker.pubkey()),
    );
    svm.send_transaction(Transaction::new(&[&cranker], msg, svm.latest_blockhash()))
        .expect("a full k-round, built entirely through the SDK, must execute");

    for (_, recipient, relayer) in &triples {
        assert_eq!(
            svm.get_account(recipient).unwrap().lamports(),
            DENOMINATION - FEE,
            "recipient paid denomination - fee"
        );
        assert_eq!(
            svm.get_account(relayer).unwrap().lamports(),
            FEE,
            "relayer paid fee"
        );
    }
}
