//! End-to-end LiteSVM integration test driven THROUGH the SDK's public API
//! (`build_initialize_pool_ix` / `build_deposit_ix` / `build_withdraw_ix` /
//! `Note`) — NOT hand-built instructions. `programs/pool-program/tests/withdraw.rs`
//! (Task 3) already proves the on-chain program's guards against hand-built
//! instructions; this test is the load-bearing proof that a real user,
//! calling only the SDK, gets a working deposit -> prove -> withdraw round
//! trip: circuit <-> prover <-> on-chain verifier <-> SDK all agree.
//!
//! Flow:
//!   1. `initialize_pool(denomination D)` via `build_initialize_pool_ix`.
//!   2. Deposit the bundle's exact two-leaf tree — decoy `hash2(111,222)` at
//!      leaf 0, then the real note (leaf 1) — via `build_deposit_ix`, so the
//!      pool's `current_root` matches `circuits/test/withdraw_vectors.json`'s
//!      committed root (the only root the committed proof can verify
//!      against).
//!   3. Build the withdraw instruction via `build_withdraw_ix` (a real Groth16
//!      proof bound to test-controlled recipient/relayer/fee accounts) and
//!      submit it.
//!   4. Assert recipient/relayer/vault balances, then assert the guards via
//!      the SDK path: double-spend, unknown root, and recipient-account
//!      substitution (extDataHash mismatch) all fail.

use litesvm::LiteSVM;
use sdk::{build_deposit_ix, build_initialize_pool_ix, build_withdraw_ix, MerklePath, Note};
use serde_json::Value;
use solana_sdk::{
    account::ReadableAccount,
    compute_budget::ComputeBudgetInstruction,
    instruction::{Instruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::{Transaction, TransactionError},
};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Anchor custom program errors start at 6000, assigned in `PoolError`
/// declaration order (see `programs/pool-program/src/lib.rs`): MerkleInit=6000,
/// ZeroDeposit=6001, CommitmentNotInField=6002, TreeFull=6003,
/// ProofMalformed=6004, ProofInvalid=6005, WrongDenomination=6006,
/// UnknownRoot=6007, FeeExceedsDenomination=6008.
const PROOF_INVALID_CODE: u32 = 6005;
const UNKNOWN_ROOT_CODE: u32 = 6007;

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
    workspace_root()
        .join("target/deploy/pool_program.so")
        .to_string_lossy()
        .into_owned()
}

/// Mirrors `programs/pool-program/tests/withdraw.rs`'s / `crates/sdk/tests/sdk.rs`'s
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

fn decode_be_hex(s: &str) -> [u8; 32] {
    assert_eq!(s.len(), 64, "expected 64 hex chars (32 bytes): {s}");
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("valid hex digit");
    }
    out
}

fn feb(n: u8) -> [u8; 32] {
    let mut b = [0u8; 32];
    b[31] = n;
    b
}

/// Loads the committed note bundle: the real note (`nullifier`/`secret`), its
/// Merkle path, and its root — same underlying deposited note as
/// `programs/pool-program/tests/withdraw.rs`'s fixture.
fn load_bundle() -> (Note, [u8; 32], MerklePath) {
    let path = workspace_root().join("circuits/test/withdraw_vectors.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let v: Value = serde_json::from_str(&raw).expect("valid JSON");
    let str_field = |name: &str| decode_be_hex(v[name].as_str().unwrap());

    let path_elements: Vec<[u8; 32]> = v["pathElements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| decode_be_hex(e.as_str().unwrap()))
        .collect();
    let path_indices: Vec<u8> = v["pathIndices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_u64().unwrap() as u8)
        .collect();

    assert_eq!(path_elements.len(), sdk::TREE_DEPTH);
    assert_eq!(path_indices.len(), sdk::TREE_DEPTH);

    let note = Note::from_parts(str_field("nullifier"), str_field("secret"))
        .expect("bundle note fields are in-field BN254 scalars");
    let root = str_field("root");
    let merkle_path = MerklePath {
        elements: path_elements.try_into().unwrap(),
        indices: path_indices.try_into().unwrap(),
    };
    (note, root, merkle_path)
}

fn cu_limit_ix() -> Instruction {
    ComputeBudgetInstruction::set_compute_unit_limit(200_000)
}

struct Fixture {
    svm: LiteSVM,
    payer: Keypair,
    pool: Pubkey,
    vault: Pubkey,
}

fn setup_fixture() -> Fixture {
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(pool_program::ID, so_path())
        .unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &pool_program::ID);
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &pool_program::ID);

    // --- Step 1: initialize_pool(D) via the SDK builder.
    let init_ix = build_initialize_pool_ix(pool, vault, mint, payer.pubkey(), DENOMINATION);
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            init_ix,
        ],
        Some(&payer.pubkey()),
    );
    svm.send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .unwrap();

    // --- Step 2: deposit the decoy hash2(111,222) THEN the real note (leaf 1)
    // via the SDK builder, so `current_root` matches the committed bundle's
    // root exactly (same tree-shape requirement as Task 3's test).
    let decoy_commitment = pool_program::poseidon::hash2(&feb(111), &feb(222)).expect("in-field");
    let decoy_ix = build_deposit_ix(pool, vault, payer.pubkey(), decoy_commitment, DENOMINATION);
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            decoy_ix,
        ],
        Some(&payer.pubkey()),
    );
    svm.send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .unwrap();

    let (note, _root, _path) = load_bundle();
    let real_ix = build_deposit_ix(pool, vault, payer.pubkey(), note.commitment(), DENOMINATION);
    svm.expire_blockhash();
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            real_ix,
        ],
        Some(&payer.pubkey()),
    );
    svm.send_transaction(Transaction::new(&[&payer], msg, svm.latest_blockhash()))
        .unwrap();

    Fixture {
        svm,
        payer,
        pool,
        vault,
    }
}

#[test]
fn sdk_driven_deposit_prove_withdraw_round_trip() {
    let Fixture {
        mut svm,
        payer,
        pool,
        vault,
    } = setup_fixture();

    let build_dir = ensure_build_artifacts();
    let (note, root, merkle_path) = load_bundle();

    // Sanity: the deposited tree's root must equal the bundle's committed
    // root — the load-bearing precondition for the generated proof to verify
    // at all (same check `pool-program/tests/withdraw.rs` makes).
    let offset = 8 + core::mem::offset_of!(pool_program::state::Pool, current_root);
    let current_root: [u8; 32] = svm.get_account(&pool).unwrap().data()[offset..offset + 32]
        .try_into()
        .unwrap();
    assert_eq!(
        current_root, root,
        "deposited tree's root (via SDK deposit ixs) must match the committed bundle's root"
    );

    let recipient = Pubkey::new_unique();
    let relayer = Keypair::new();
    svm.airdrop(&relayer.pubkey(), 10_000_000_000).unwrap();

    let artifacts = sdk::WithdrawArtifacts {
        wasm_path: &build_dir.join("withdraw_js").join("withdraw.wasm"),
        r1cs_path: &build_dir.join("withdraw.r1cs"),
        zkey_path: &build_dir.join("withdraw.zkey"),
    };

    // --- Step 3: build the withdraw instruction via the SDK (real Groth16
    // proof bound to recipient/relayer/fee).
    let build = build_withdraw_ix(
        pool,
        vault,
        recipient,
        relayer.pubkey(),
        &note,
        &merkle_path,
        root,
        FEE,
        artifacts,
    )
    .expect("proving the committed note bundle via the SDK must succeed");

    // --- Guard: a withdraw whose recipient ACCOUNT differs from the one
    // bound in the proof must fail (extDataHash mismatch -> ProofInvalid).
    // We can't ask `build_withdraw_ix` to build this directly (it always
    // binds the accounts it's given), so splice the wrong recipient into the
    // SDK-built instruction's accounts — this is exactly the attack the
    // extDataHash binding defends against: reuse a valid proof's bytes
    // against a different payout account.
    {
        let mut tampered = build.instruction.clone();
        let wrong_recipient = Pubkey::new_unique();
        tampered.accounts[3].pubkey = wrong_recipient;
        let msg = Message::new(&[cu_limit_ix(), tampered], Some(&payer.pubkey()));
        let tx = Transaction::new(&[&payer, &relayer], msg, svm.latest_blockhash());
        let outcome = svm
            .send_transaction(tx)
            .expect_err("withdraw to an unbound recipient must fail");
        assert!(
            matches!(
                outcome.err,
                TransactionError::InstructionError(_, InstructionError::Custom(code))
                    if code == PROOF_INVALID_CODE
            ),
            "expected ProofInvalid({PROOF_INVALID_CODE}), got {:?} (logs: {:?})",
            outcome.err,
            outcome.meta.logs
        );
    }

    // --- Guard: an unknown root must fail before proof verification runs.
    // Tamper the encoded root bytes in the SDK-built instruction data
    // in-place (root is at a fixed offset: disc(8) || a(64) || b(128) ||
    // c(64) || root(32) || nullifier_hash(32) || fee_le(8)).
    {
        let mut tampered = build.instruction.clone();
        let root_off = 8 + 64 + 128 + 64;
        tampered.data[root_off] ^= 0x01;
        svm.expire_blockhash();
        let msg = Message::new(&[cu_limit_ix(), tampered], Some(&payer.pubkey()));
        let tx = Transaction::new(&[&payer, &relayer], msg, svm.latest_blockhash());
        let outcome = svm
            .send_transaction(tx)
            .expect_err("withdraw against an unknown root must fail");
        assert!(
            matches!(
                outcome.err,
                TransactionError::InstructionError(_, InstructionError::Custom(code))
                    if code == UNKNOWN_ROOT_CODE
            ),
            "expected UnknownRoot({UNKNOWN_ROOT_CODE}), got {:?} (logs: {:?})",
            outcome.err,
            outcome.meta.logs
        );
    }

    // --- Happy path: submit the SDK-built instruction unmodified.
    let vault_before = svm.get_account(&vault).unwrap().lamports();
    let recipient_before = svm
        .get_account(&recipient)
        .map(|a| a.lamports())
        .unwrap_or(0);
    let relayer_before = svm.get_account(&relayer.pubkey()).unwrap().lamports();

    svm.expire_blockhash();
    let msg = Message::new(
        &[cu_limit_ix(), build.instruction.clone()],
        Some(&payer.pubkey()),
    );
    let tx = Transaction::new(&[&payer, &relayer], msg, svm.latest_blockhash());
    let meta = svm
        .send_transaction(tx)
        .expect("a real SDK-built proof, submitted unmodified, must succeed");
    println!("e2e withdraw CU consumed: {}", meta.compute_units_consumed);

    let (nullifier_pda, _) = Pubkey::find_program_address(
        &[
            b"nullifier",
            pool.as_ref(),
            build.public_inputs.nullifier_hash.as_ref(),
        ],
        &pool_program::ID,
    );
    let nullifier_rent = svm.get_account(&nullifier_pda).unwrap().lamports();

    let vault_after = svm.get_account(&vault).unwrap().lamports();
    let recipient_after = svm.get_account(&recipient).unwrap().lamports();
    let relayer_after = svm.get_account(&relayer.pubkey()).unwrap().lamports();

    assert_eq!(
        vault_before - vault_after,
        DENOMINATION,
        "vault paid out the full denomination"
    );
    assert_eq!(
        recipient_after - recipient_before,
        DENOMINATION - FEE,
        "recipient received denomination minus fee"
    );
    assert_eq!(
        relayer_after,
        relayer_before - nullifier_rent + FEE,
        "relayer paid the nullifier's rent and received the fee"
    );

    // --- Guard: a second, otherwise-identical withdraw with the same
    // nullifier_hash must fail (double-spend; the nullifier PDA already
    // exists).
    svm.expire_blockhash();
    let msg = Message::new(
        &[cu_limit_ix(), build.instruction.clone()],
        Some(&payer.pubkey()),
    );
    let tx = Transaction::new(&[&payer, &relayer], msg, svm.latest_blockhash());
    let outcome = svm
        .send_transaction(tx)
        .expect_err("re-spending the same nullifier must fail (PDA already exists)");
    assert_ne!(
        outcome.err,
        TransactionError::AlreadyProcessed,
        "must be rejected by the init double-spend guard during execution, not tx dedup"
    );
    assert!(
        matches!(
            outcome.err,
            TransactionError::InstructionError(_, InstructionError::Custom(_))
        ),
        "expected an InstructionError from the init double-spend guard, got {:?} (logs: {:?})",
        outcome.err,
        outcome.meta.logs
    );
    assert!(
        outcome
            .meta
            .logs
            .iter()
            .any(|log| log.contains("already in use")),
        "expected the System Program's Allocate 'already in use' guard to fire; logs: {:?}",
        outcome.meta.logs
    );
}
