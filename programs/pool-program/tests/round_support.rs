#![allow(dead_code, deprecated)]
// `#[path]` (not a bare `mod common;`): this file is compiled both as its own
// standalone integration-test crate (root = tests/) AND nested as
// `commit_intent.rs`'s `mod round_support;` submodule (root = tests/round_support/
// for child-module lookup) — `#[path]` pins the lookup to `tests/common.rs` in
// both cases instead of only the former.
#[path = "common.rs"]
mod common;
pub use common::{disc, program_id, so_path};

use litesvm::LiteSVM;
use pool_program::verifier::WithdrawProof;
use sdk::{MerkleTree, Note};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::Transaction,
};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const DENOMINATION: u64 = 2_000_000;
pub const FEE: u64 = 1_000;

pub struct IntentMaterial {
    pub note: Note,
    pub proof: WithdrawProof,
    pub root: [u8; 32],
    pub nullifier_hash: [u8; 32],
    pub recipient: Pubkey,
    pub relayer: Pubkey,
    pub fee: u64,
    pub intent_pda: Pubkey,
    pub nullifier_pda: Pubkey,
}

pub struct RoundFixture {
    pub svm: LiteSVM,
    pub payer: Keypair,
    pub pool: Pubkey,
    pub vault: Pubkey,
    pub k_floor: u16,
    pub intents: Vec<IntentMaterial>,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("two levels below workspace root")
        .to_path_buf()
}

/// Mirrors the other tests' build guard: the `circuits/build/*` artifacts are
/// gitignored outputs of `circuits/scripts/setup.sh`; generate them if absent
/// rather than skip the real prove/verify.
fn ensure_build_artifacts() -> PathBuf {
    static BUILD_DIR: OnceLock<PathBuf> = OnceLock::new();
    BUILD_DIR
        .get_or_init(|| {
            let circuits_dir = workspace_root().join("circuits");
            let build_dir = circuits_dir.join("build");
            let required = [
                build_dir.join("withdraw_js").join("withdraw.wasm"),
                build_dir.join("withdraw.r1cs"),
                build_dir.join("withdraw.zkey"),
                build_dir.join("verification_key.json"),
            ];
            if !required.iter().all(|p| p.exists()) {
                let status = std::process::Command::new("bash")
                    .arg(circuits_dir.join("scripts").join("setup.sh"))
                    .status();
                let ok = matches!(status, Ok(s) if s.success());
                if !ok || !required.iter().all(|p| p.exists()) {
                    panic!(
                        "circuits/build artifacts missing and setup.sh did not produce them \
                         (needs circom + npx/snarkjs on PATH)."
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

/// Initialize a pool with `k_floor`, deposit `n` fresh notes, and build a real
/// proof for each against the common final root.
pub fn build_round_fixture(k_floor: u16, n: usize) -> RoundFixture {
    let build_dir = ensure_build_artifacts();

    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    // initialize_pool(DENOMINATION, k_floor)
    let mut data = disc("initialize_pool").to_vec();
    data.extend_from_slice(&DENOMINATION.to_le_bytes());
    data.extend_from_slice(&k_floor.to_le_bytes());
    send(
        &mut svm,
        &payer,
        &[&payer],
        Instruction {
            program_id: program_id(),
            accounts: vec![
                AccountMeta::new(pool, false),
                AccountMeta::new(vault, false),
                AccountMeta::new(round0, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            data,
        },
    );

    // Build the client tree + deposit each commitment on-chain (roots agree by
    // the MerkleTree<->pool_program parity proven in Task 1).
    let notes: Vec<Note> = (0..n).map(|_| Note::new()).collect();
    let mut tree = MerkleTree::new().unwrap();
    for note in &notes {
        let commitment = note.commitment();
        tree.insert(commitment);
        let mut d = disc("deposit").to_vec();
        d.extend_from_slice(&commitment);
        d.extend_from_slice(&DENOMINATION.to_le_bytes());
        send(
            &mut svm,
            &payer,
            &[&payer],
            Instruction {
                program_id: program_id(),
                accounts: vec![
                    AccountMeta::new(pool, false),
                    AccountMeta::new(vault, false),
                    AccountMeta::new(payer.pubkey(), true),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: d,
            },
        );
    }

    let root = tree.root();
    let mut intents = Vec::with_capacity(n);
    for (i, note) in notes.iter().enumerate() {
        let recipient = Pubkey::new_unique();
        let relayer = Pubkey::new_unique();
        let path = tree.authentication_path(i);
        let ext = sdk::compute_ext_data_hash(&recipient.to_bytes(), &relayer.to_bytes(), FEE);
        let inputs = sdk::WithdrawInputs {
            root,
            nullifier_hash: note.nullifier_hash(),
            ext_data_hash: ext,
            nullifier: note.nullifier(),
            secret: note.secret(),
            path_elements: path.elements,
            path_indices: path.indices,
        };
        let (proof, public_inputs) = prover::prove_withdraw(
            build_dir.join("withdraw_js").join("withdraw.wasm"),
            build_dir.join("withdraw.r1cs"),
            build_dir.join("withdraw.zkey"),
            &inputs,
        )
        .expect("proving a fresh note must succeed");
        let withdraw_proof = WithdrawProof {
            a: prover::proof_a_to_solana_be(&proof.a).unwrap(),
            b: prover::g2_to_solana_be(&proof.b).unwrap(),
            c: prover::g1_to_solana_be(&proof.c).unwrap(),
        };
        let (intent_pda, _) = Pubkey::find_program_address(
            &[
                b"intent",
                pool.as_ref(),
                public_inputs.nullifier_hash.as_ref(),
            ],
            &program_id(),
        );
        let (nullifier_pda, _) = Pubkey::find_program_address(
            &[
                b"nullifier",
                pool.as_ref(),
                public_inputs.nullifier_hash.as_ref(),
            ],
            &program_id(),
        );
        intents.push(IntentMaterial {
            note: *note,
            proof: withdraw_proof,
            root: public_inputs.root,
            nullifier_hash: public_inputs.nullifier_hash,
            recipient,
            relayer,
            fee: FEE,
            intent_pda,
            nullifier_pda,
        });
    }

    RoundFixture {
        svm,
        payer,
        pool,
        vault,
        k_floor,
        intents,
    }
}

/// Like `build_round_fixture` but each intent's recipient is a keypair we
/// control (needed by `cancel_intent`, where the recipient must sign). Returns
/// the fixture plus the recipient keypairs (index-aligned with `intents`).
pub fn build_round_fixture_signer_recipients(
    k_floor: u16,
    n: usize,
) -> (RoundFixture, Vec<Keypair>) {
    let build_dir = ensure_build_artifacts();

    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    // initialize_pool(DENOMINATION, k_floor)
    let mut data = disc("initialize_pool").to_vec();
    data.extend_from_slice(&DENOMINATION.to_le_bytes());
    data.extend_from_slice(&k_floor.to_le_bytes());
    send(
        &mut svm,
        &payer,
        &[&payer],
        Instruction {
            program_id: program_id(),
            accounts: vec![
                AccountMeta::new(pool, false),
                AccountMeta::new(vault, false),
                AccountMeta::new(round0, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            data,
        },
    );

    // Build the client tree + deposit each commitment on-chain (roots agree by
    // the MerkleTree<->pool_program parity proven in Task 1).
    let notes: Vec<Note> = (0..n).map(|_| Note::new()).collect();
    let mut tree = MerkleTree::new().unwrap();
    for note in &notes {
        let commitment = note.commitment();
        tree.insert(commitment);
        let mut d = disc("deposit").to_vec();
        d.extend_from_slice(&commitment);
        d.extend_from_slice(&DENOMINATION.to_le_bytes());
        send(
            &mut svm,
            &payer,
            &[&payer],
            Instruction {
                program_id: program_id(),
                accounts: vec![
                    AccountMeta::new(pool, false),
                    AccountMeta::new(vault, false),
                    AccountMeta::new(payer.pubkey(), true),
                    AccountMeta::new_readonly(system_program::ID, false),
                ],
                data: d,
            },
        );
    }

    let root = tree.root();
    let mut intents = Vec::with_capacity(n);
    let mut recipient_keypairs = Vec::with_capacity(n);
    for (i, note) in notes.iter().enumerate() {
        let recipient_kp = Keypair::new();
        let recipient = recipient_kp.pubkey();
        svm.airdrop(&recipient, 1_000_000).unwrap();
        let relayer = Pubkey::new_unique();
        let path = tree.authentication_path(i);
        let ext = sdk::compute_ext_data_hash(&recipient.to_bytes(), &relayer.to_bytes(), FEE);
        let inputs = sdk::WithdrawInputs {
            root,
            nullifier_hash: note.nullifier_hash(),
            ext_data_hash: ext,
            nullifier: note.nullifier(),
            secret: note.secret(),
            path_elements: path.elements,
            path_indices: path.indices,
        };
        let (proof, public_inputs) = prover::prove_withdraw(
            build_dir.join("withdraw_js").join("withdraw.wasm"),
            build_dir.join("withdraw.r1cs"),
            build_dir.join("withdraw.zkey"),
            &inputs,
        )
        .expect("proving a fresh note must succeed");
        let withdraw_proof = WithdrawProof {
            a: prover::proof_a_to_solana_be(&proof.a).unwrap(),
            b: prover::g2_to_solana_be(&proof.b).unwrap(),
            c: prover::g1_to_solana_be(&proof.c).unwrap(),
        };
        let (intent_pda, _) = Pubkey::find_program_address(
            &[
                b"intent",
                pool.as_ref(),
                public_inputs.nullifier_hash.as_ref(),
            ],
            &program_id(),
        );
        let (nullifier_pda, _) = Pubkey::find_program_address(
            &[
                b"nullifier",
                pool.as_ref(),
                public_inputs.nullifier_hash.as_ref(),
            ],
            &program_id(),
        );
        intents.push(IntentMaterial {
            note: *note,
            proof: withdraw_proof,
            root: public_inputs.root,
            nullifier_hash: public_inputs.nullifier_hash,
            recipient,
            relayer,
            fee: FEE,
            intent_pda,
            nullifier_pda,
        });
        recipient_keypairs.push(recipient_kp);
    }

    (
        RoundFixture {
            svm,
            payer,
            pool,
            vault,
            k_floor,
            intents,
        },
        recipient_keypairs,
    )
}

/// Build a `commit_intent` transaction for intent `i` against round `round_id`.
pub fn commit_intent_tx(fx: &RoundFixture, i: usize, round_id: u64) -> Transaction {
    let m = &fx.intents[i];
    let (round, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &round_id.to_le_bytes()],
        &program_id(),
    );
    let mut data = disc("commit_intent").to_vec();
    data.extend_from_slice(&m.proof.a);
    data.extend_from_slice(&m.proof.b);
    data.extend_from_slice(&m.proof.c);
    data.extend_from_slice(&m.root);
    data.extend_from_slice(&m.nullifier_hash);
    data.extend_from_slice(&m.fee.to_le_bytes());
    data.extend_from_slice(&round_id.to_le_bytes());
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new_readonly(fx.pool, false),
            AccountMeta::new(round, false),
            AccountMeta::new(m.intent_pda, false),
            AccountMeta::new(m.nullifier_pda, false),
            AccountMeta::new_readonly(m.recipient, false),
            AccountMeta::new_readonly(m.relayer, false),
            AccountMeta::new(fx.payer.pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            ix,
        ],
        Some(&fx.payer.pubkey()),
    );
    Transaction::new(&[&fx.payer], msg, fx.svm.latest_blockhash())
}
