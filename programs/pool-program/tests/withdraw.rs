//! Full happy-path + guard-rail coverage for `withdraw`: real Groth16 proof
//! verification, known-root enforcement, atomic single-spend (re-homed from
//! the removed `tests/nullifier.rs`), denominated payout via the vault PDA,
//! and the front-run-safety guarantee (payout accounts ARE the hashed keys,
//! so a proof generated for one recipient/relayer/fee is rejected outright
//! for any other payout accounts).
#![allow(deprecated)]

mod common;
use common::{disc, program_id, so_path};
use litesvm::LiteSVM;
use pool_program::verifier::WithdrawProof;
use prover::{FieldBytes, WithdrawInputs, TREE_DEPTH};
use serde_json::Value;
use solana_sdk::{
    account::ReadableAccount,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::{Transaction, TransactionError},
};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Anchor custom program errors start at 6000, assigned in `PoolError` declaration
/// order (see `programs/pool-program/src/lib.rs`): MerkleInit=6000, ZeroDeposit=6001,
/// CommitmentNotInField=6002, TreeFull=6003, ProofMalformed=6004, ProofInvalid=6005,
/// WrongDenomination=6006, UnknownRoot=6007, FeeExceedsDenomination=6008.
const PROOF_INVALID_CODE: u32 = 6005;
const UNKNOWN_ROOT_CODE: u32 = 6007;

/// The pool's fixed denomination for this test; unrelated to the note bundle
/// itself, just large enough to make a nontrivial fee legible.
const DENOMINATION: u64 = 2_000_000;
const FEE: u64 = 1_000;

/// Measured empirically at ~109.5k CU (see the printed "withdraw CU consumed"
/// line) — the `alt_bn128` multi-pairing in `verify_withdraw` is the dominant
/// cost, and it's hardware-accelerated via syscalls rather than software
/// pairing, so it lands well under the plan's worst-case 400k-1.4M estimate.
/// 200k leaves comfortable headroom over the measured figure.
fn withdraw_cu_limit_ix() -> Instruction {
    ComputeBudgetInstruction::set_compute_unit_limit(200_000)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("programs/pool-program is two levels below the workspace root")
        .to_path_buf()
}

/// Mirrors `tests/verifier.rs::ensure_build_artifacts` / `crates/prover`'s own
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
                         Run circuits/scripts/setup.sh first, then re-run `cargo test -p pool-program`."
                    );
                }
            }
            build_dir
        })
        .clone()
}

fn decode_be_hex(s: &str) -> FieldBytes {
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

/// Loads the committed note bundle's Merkle proof (nullifier/secret/root/path)
/// from `circuits/test/withdraw_vectors.json`. The bundle's own fixed
/// recipient/relayer/fee are NOT reused here: `extDataHash` is an unconstrained
/// circuit signal (see `crates/prover`'s doc comment), so this test generates a
/// fresh proof bound to whichever real, test-controlled payout accounts it
/// chooses, while reusing the bundle's `root`/`nullifier`/`secret`/path (i.e.
/// the same underlying deposited note).
fn load_bundle_merkle_proof() -> WithdrawInputs {
    let path = workspace_root().join("circuits/test/withdraw_vectors.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let v: Value = serde_json::from_str(&raw).expect("valid JSON");

    let str_field = |name: &str| decode_be_hex(v[name].as_str().unwrap());
    let path_elements: Vec<FieldBytes> = v["pathElements"]
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

    assert_eq!(path_elements.len(), TREE_DEPTH);
    assert_eq!(path_indices.len(), TREE_DEPTH);

    WithdrawInputs {
        root: str_field("root"),
        nullifier_hash: str_field("nullifierHash"),
        // Placeholder — overwritten by the caller with a test-chosen ext_data_hash.
        ext_data_hash: [0u8; 32],
        nullifier: str_field("nullifier"),
        secret: str_field("secret"),
        path_elements: path_elements.try_into().unwrap(),
        path_indices: path_indices.try_into().unwrap(),
    }
}

fn setup_pool(svm: &mut LiteSVM, payer: &Keypair, denomination: u64) -> (Pubkey, Pubkey) {
    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    let mut data = disc("initialize_pool").to_vec();
    data.extend_from_slice(&denomination.to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(round, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&payer.pubkey()),
    );
    svm.send_transaction(Transaction::new(&[payer], msg, svm.latest_blockhash()))
        .unwrap();
    (pool, vault)
}

fn deposit(
    svm: &mut LiteSVM,
    payer: &Keypair,
    pool: Pubkey,
    vault: Pubkey,
    commitment: [u8; 32],
    amount: u64,
) {
    let mut data = disc("deposit").to_vec();
    data.extend_from_slice(&commitment);
    data.extend_from_slice(&amount.to_le_bytes());
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&payer.pubkey()),
    );
    svm.send_transaction(Transaction::new(&[payer], msg, svm.latest_blockhash()))
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn withdraw_tx(
    svm: &LiteSVM,
    fee_payer: &Keypair,
    relayer: &Keypair,
    pool: Pubkey,
    vault: Pubkey,
    recipient: Pubkey,
    proof: &WithdrawProof,
    root: [u8; 32],
    nullifier_hash: [u8; 32],
    fee: u64,
) -> Transaction {
    let (nullifier, _) = Pubkey::find_program_address(
        &[b"nullifier", pool.as_ref(), nullifier_hash.as_ref()],
        &program_id(),
    );

    let mut data = disc("withdraw").to_vec();
    // Anchor Borsh-serializes instruction args field-by-field in declaration order:
    // `proof: WithdrawProof { a, b, c }`, then `root`, `nullifier_hash`, `fee`.
    data.extend_from_slice(&proof.a);
    data.extend_from_slice(&proof.b);
    data.extend_from_slice(&proof.c);
    data.extend_from_slice(&root);
    data.extend_from_slice(&nullifier_hash);
    data.extend_from_slice(&fee.to_le_bytes());

    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(pool, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(nullifier, false),
            AccountMeta::new(recipient, false),
            AccountMeta::new(relayer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    let msg = Message::new(&[withdraw_cu_limit_ix(), ix], Some(&fee_payer.pubkey()));
    Transaction::new(&[fee_payer, relayer], msg, svm.latest_blockhash())
}

struct Fixture {
    svm: LiteSVM,
    payer: Keypair,
    pool: Pubkey,
    vault: Pubkey,
    proof: WithdrawProof,
    root: [u8; 32],
    nullifier_hash: [u8; 32],
    recipient: Pubkey,
    relayer: Keypair,
}

/// Deposits the bundle's exact two-leaf tree (decoy `hash2(111,222)` at leaf 0,
/// then the real note `hash2(7,9)` at leaf 1 — only this order reproduces the
/// bundle's committed root) into a freshly initialized pool, then generates a
/// real Groth16 proof bound to a chosen `(recipient, relayer, fee)`.
fn setup_fixture() -> Fixture {
    let build_dir = ensure_build_artifacts();
    let merkle = load_bundle_merkle_proof();

    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let (pool, vault) = setup_pool(&mut svm, &payer, DENOMINATION);

    let decoy = pool_program::poseidon::hash2(&feb(111), &feb(222)).expect("in-field");
    deposit(&mut svm, &payer, pool, vault, decoy, DENOMINATION);

    let commitment = pool_program::poseidon::hash2(&feb(7), &feb(9)).expect("in-field");
    deposit(&mut svm, &payer, pool, vault, commitment, DENOMINATION);

    // Sanity: the deposited tree's root must equal the bundle's committed root —
    // the load-bearing precondition for the generated proof to verify at all.
    let offset = 8 + core::mem::offset_of!(pool_program::state::Pool, current_root);
    let current_root: [u8; 32] = svm.get_account(&pool).unwrap().data()[offset..offset + 32]
        .try_into()
        .unwrap();
    assert_eq!(
        current_root, merkle.root,
        "deposited tree's root must match the committed bundle's root"
    );

    let recipient = Pubkey::new_unique();
    let relayer = Keypair::new();
    svm.airdrop(&relayer.pubkey(), 10_000_000_000).unwrap();

    let ext_data_hash =
        ext_data::ext_data_hash(&recipient.to_bytes(), &relayer.pubkey().to_bytes(), FEE);
    let inputs = WithdrawInputs {
        ext_data_hash,
        ..merkle
    };

    let (proof, public_inputs) = prover::prove_withdraw(
        build_dir.join("withdraw_js").join("withdraw.wasm"),
        build_dir.join("withdraw.r1cs"),
        build_dir.join("withdraw.zkey"),
        &inputs,
    )
    .expect("proving the committed note bundle must succeed");

    let withdraw_proof = WithdrawProof {
        a: prover::proof_a_to_solana_be(&proof.a).unwrap(),
        b: prover::g2_to_solana_be(&proof.b).unwrap(),
        c: prover::g1_to_solana_be(&proof.c).unwrap(),
    };

    Fixture {
        svm,
        payer,
        pool,
        vault,
        proof: withdraw_proof,
        root: public_inputs.root,
        nullifier_hash: public_inputs.nullifier_hash,
        recipient,
        relayer,
    }
}

#[test]
fn withdraw_pays_out_and_enforces_every_guard() {
    let Fixture {
        mut svm,
        payer,
        pool,
        vault,
        proof,
        root,
        nullifier_hash,
        recipient,
        relayer,
    } = setup_fixture();

    // --- Guard 1: a withdraw whose `recipient` account differs from the one the
    // proof was bound to must fail (extDataHash mismatch -> ProofInvalid). The
    // nullifier `init` runs (and is rolled back) as part of account validation,
    // so this doesn't burn the real nullifier.
    let wrong_recipient = Pubkey::new_unique();
    let tx = withdraw_tx(
        &svm,
        &payer,
        &relayer,
        pool,
        vault,
        wrong_recipient,
        &proof,
        root,
        nullifier_hash,
        FEE,
    );
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

    // --- Guard 2: an unknown root must fail before proof verification even runs.
    let mut bad_root = root;
    bad_root[0] ^= 0x01;
    svm.expire_blockhash();
    let tx = withdraw_tx(
        &svm,
        &payer,
        &relayer,
        pool,
        vault,
        recipient,
        &proof,
        bad_root,
        nullifier_hash,
        FEE,
    );
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

    // --- Happy path: the real withdraw, to the bound recipient/relayer.
    let vault_before = svm.get_account(&vault).unwrap().lamports();
    let recipient_before = svm
        .get_account(&recipient)
        .map(|a| a.lamports())
        .unwrap_or(0);
    let relayer_before = svm.get_account(&relayer.pubkey()).unwrap().lamports();

    svm.expire_blockhash();
    let tx = withdraw_tx(
        &svm,
        &payer,
        &relayer,
        pool,
        vault,
        recipient,
        &proof,
        root,
        nullifier_hash,
        FEE,
    );
    let meta = svm
        .send_transaction(tx)
        .expect("a real proof for the committed bundle, to its bound accounts, must succeed");
    println!("withdraw CU consumed: {}", meta.compute_units_consumed);

    let (nullifier_pda, _) = Pubkey::find_program_address(
        &[b"nullifier", pool.as_ref(), nullifier_hash.as_ref()],
        &program_id(),
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
    // `relayer` is the `payer` for the nullifier PDA's rent (see `Withdraw` accounts,
    // `payer = relayer`) and is not the transaction's fee payer here (that's `payer`),
    // so its only other lamport movement is the `fee` credit from the vault.
    assert_eq!(
        relayer_after,
        relayer_before - nullifier_rent + FEE,
        "relayer paid the nullifier's rent and received the fee"
    );

    // --- Guard 3 (re-homed from the removed tests/nullifier.rs): a second,
    // otherwise-identical withdraw with the same nullifier_hash must fail —
    // the nullifier PDA already exists (atomic single-spend guard).
    svm.expire_blockhash();
    let tx = withdraw_tx(
        &svm,
        &payer,
        &relayer,
        pool,
        vault,
        recipient,
        &proof,
        root,
        nullifier_hash,
        FEE,
    );
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
