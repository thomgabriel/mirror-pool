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
    system_instruction, system_program,
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
    // Real validator vote account for a Stake pool (`Pubkey::default()` for a
    // Withdraw pool, matching `pool.validator`'s own zero value there).
    pub validator: Pubkey,
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
    data.push(0u8);
    data.extend_from_slice(&Pubkey::default().to_bytes());
    data.extend_from_slice(&FEE.to_le_bytes());
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
    // the MerkleTree<->pool_program parity proven by the sdk parity tests).
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
        validator: Pubkey::default(),
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
    data.push(0u8);
    data.extend_from_slice(&Pubkey::default().to_bytes());
    data.extend_from_slice(&FEE.to_le_bytes());
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
    // the MerkleTree<->pool_program parity proven by the sdk parity tests).
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
            validator: Pubkey::default(),
        },
        recipient_keypairs,
    )
}

/// Same rent sysvar the on-chain `Rent::get()` reads inside `initialize_pool`'s
/// stake-config validation, computed independently so callers never hardcode a
/// value that could drift from LiteSVM's actual rent parameters.
fn stake_account_rent() -> u64 {
    solana_sdk::rent::Rent::default().minimum_balance(pool_program::invariants::STAKE_ACCOUNT_SIZE)
}

/// The denomination `build_stake_round_fixture` sizes its pool to: enough to
/// clear `stake_fee + rent + MIN_STAKE_DELEGATION` with slack. Exposed so
/// execute-round tests can independently recompute `to_stake = denomination -
/// stake_fee` (the amount the stake account is funded with) without
/// re-deriving the fixture's internal formula.
pub fn stake_pool_denomination(stake_fee: u64) -> u64 {
    pool_program::invariants::MIN_STAKE_DELEGATION + stake_account_rent() + stake_fee + 1_000_000
}

/// Create a real, delegable validator vote account: a funded node identity
/// plus a Vote-program-owned account initialized via the real
/// `CreateAccount`+`InitializeAccount` CPI pair (not a hand-serialized
/// `VoteState`) so `DelegateStake` accepts it exactly as it would on a live
/// cluster. Returns the vote account's pubkey (the pool's `validator`).
pub fn create_validator_vote_account(svm: &mut LiteSVM, payer: &Keypair) -> Pubkey {
    use solana_sdk::vote::{
        instruction::{create_account_with_config, CreateVoteAccountConfig},
        state::{VoteInit, VoteStateVersions},
    };

    let node = Keypair::new();
    let vote_account = Keypair::new();
    let rent = solana_sdk::rent::Rent::default();
    let vote_space = VoteStateVersions::vote_state_size_of(true) as u64;

    let mut instructions = vec![system_instruction::create_account(
        &payer.pubkey(),
        &node.pubkey(),
        rent.minimum_balance(0),
        0,
        &system_program::ID,
    )];
    instructions.extend(create_account_with_config(
        &payer.pubkey(),
        &vote_account.pubkey(),
        &VoteInit {
            node_pubkey: node.pubkey(),
            authorized_voter: node.pubkey(),
            authorized_withdrawer: node.pubkey(),
            commission: 0,
        },
        rent.minimum_balance(vote_space as usize),
        CreateVoteAccountConfig {
            space: vote_space,
            ..Default::default()
        },
    ));
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            instructions[0].clone(),
            instructions[1].clone(),
        ],
        Some(&payer.pubkey()),
    );
    svm.send_transaction(Transaction::new(
        &[payer, &node, &vote_account],
        msg,
        svm.latest_blockhash(),
    ))
    .expect("real vote-account creation must succeed");
    vote_account.pubkey()
}

/// Like `build_round_fixture`, but the pool is configured as a STAKE pool
/// (`action_kind = 1`) targeting a freshly created, real, delegable validator
/// vote account (`fx.validator`). Every intent's `fee` is set to `stake_fee`
/// (the only value `commit_intent`'s uniform-fee guard accepts for a stake
/// pool), and the denomination is sized to clear `fee + rent +
/// MIN_STAKE_DELEGATION` with slack.
pub fn build_stake_round_fixture(k_floor: u16, n: usize, stake_fee: u64) -> RoundFixture {
    let build_dir = ensure_build_artifacts();

    let denomination = stake_pool_denomination(stake_fee);

    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let validator = create_validator_vote_account(&mut svm, &payer);

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    // initialize_pool(denomination, k_floor, action_kind=1, validator, stake_fee)
    let mut data = disc("initialize_pool").to_vec();
    data.extend_from_slice(&denomination.to_le_bytes());
    data.extend_from_slice(&k_floor.to_le_bytes());
    data.push(1u8);
    data.extend_from_slice(&validator.to_bytes());
    data.extend_from_slice(&stake_fee.to_le_bytes());
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

    let notes: Vec<Note> = (0..n).map(|_| Note::new()).collect();
    let mut tree = MerkleTree::new().unwrap();
    for note in &notes {
        let commitment = note.commitment();
        tree.insert(commitment);
        let mut d = disc("deposit").to_vec();
        d.extend_from_slice(&commitment);
        d.extend_from_slice(&denomination.to_le_bytes());
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
        let ext = sdk::compute_ext_data_hash(&recipient.to_bytes(), &relayer.to_bytes(), stake_fee);
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
            fee: stake_fee,
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
        validator,
    }
}

/// Like `build_stake_round_fixture`, but each intent's recipient is a keypair
/// we control (needed by `cancel_intent`, where the recipient must sign — see
/// `build_round_fixture_signer_recipients`'s withdraw-pool counterpart).
/// Returns the fixture plus the recipient keypairs (index-aligned with
/// `intents`).
pub fn build_stake_round_fixture_signer_recipients(
    k_floor: u16,
    n: usize,
    stake_fee: u64,
) -> (RoundFixture, Vec<Keypair>) {
    let build_dir = ensure_build_artifacts();

    let denomination = stake_pool_denomination(stake_fee);

    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();

    let validator = create_validator_vote_account(&mut svm, &payer);

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    // initialize_pool(denomination, k_floor, action_kind=1, validator, stake_fee)
    let mut data = disc("initialize_pool").to_vec();
    data.extend_from_slice(&denomination.to_le_bytes());
    data.extend_from_slice(&k_floor.to_le_bytes());
    data.push(1u8);
    data.extend_from_slice(&validator.to_bytes());
    data.extend_from_slice(&stake_fee.to_le_bytes());
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

    let notes: Vec<Note> = (0..n).map(|_| Note::new()).collect();
    let mut tree = MerkleTree::new().unwrap();
    for note in &notes {
        let commitment = note.commitment();
        tree.insert(commitment);
        let mut d = disc("deposit").to_vec();
        d.extend_from_slice(&commitment);
        d.extend_from_slice(&denomination.to_le_bytes());
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
        let ext = sdk::compute_ext_data_hash(&recipient.to_bytes(), &relayer.to_bytes(), stake_fee);
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
            fee: stake_fee,
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
            validator,
        },
        recipient_keypairs,
    )
}

/// Uniform fee for the cached STAKE material pool (mirrors stake_round.rs's
/// per-test `stake_fee = 5_000`).
pub const STAKE_FEE: u64 = 5_000;

/// Size of each cached material pool: covers the largest guard test
/// (MAX_K_WITHDRAW + 1 commits) plus sweep headroom above the ~19 lock-arithmetic
/// ceiling.
pub const MAX_CACHED_INTENTS: usize = 21;

/// One pre-proven note: everything about an intent that does NOT depend on
/// which pool/SVM instance it is used against (proofs bind root + nullifier +
/// extDataHash only). PDAs are derived per-fixture.
pub struct CachedMaterial {
    pub note: Note,
    pub proof: WithdrawProof,
    pub root: [u8; 32],
    pub nullifier_hash: [u8; 32],
    pub recipient_keypair: [u8; 64],
    pub relayer: Pubkey,
    pub fee: u64,
}

fn generate_materials(fee: u64) -> Vec<CachedMaterial> {
    let build_dir = ensure_build_artifacts();
    let notes: Vec<Note> = (0..MAX_CACHED_INTENTS).map(|_| Note::new()).collect();
    let mut tree = MerkleTree::new().unwrap();
    for note in &notes {
        tree.insert(note.commitment());
    }
    let root = tree.root();
    notes
        .iter()
        .enumerate()
        .map(|(i, note)| {
            let recipient_kp = Keypair::new();
            let relayer = Pubkey::new_unique();
            let path = tree.authentication_path(i);
            let ext = sdk::compute_ext_data_hash(
                &recipient_kp.pubkey().to_bytes(),
                &relayer.to_bytes(),
                fee,
            );
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
            .expect("proving a cached note must succeed");
            CachedMaterial {
                note: *note,
                proof: WithdrawProof {
                    a: prover::proof_a_to_solana_be(&proof.a).unwrap(),
                    b: prover::g2_to_solana_be(&proof.b).unwrap(),
                    c: prover::g1_to_solana_be(&proof.c).unwrap(),
                },
                root: public_inputs.root,
                nullifier_hash: public_inputs.nullifier_hash,
                recipient_keypair: recipient_kp.to_bytes(),
                relayer,
                fee,
            }
        })
        .collect()
}

/// The ~21 Groth16 proofs are generated ONCE per test binary and shared by
/// every cached fixture (the expensive part is pool-independent).
pub fn cached_withdraw_materials() -> &'static [CachedMaterial] {
    static M: OnceLock<Vec<CachedMaterial>> = OnceLock::new();
    M.get_or_init(|| generate_materials(FEE))
}

pub fn cached_stake_materials() -> &'static [CachedMaterial] {
    static M: OnceLock<Vec<CachedMaterial>> = OnceLock::new();
    M.get_or_init(|| generate_materials(STAKE_FEE))
}

/// Build a fixture from a cached pool: fresh SVM + pool, deposit ALL
/// MAX_CACHED_INTENTS commitments (the cached proofs bind the full-tree root,
/// which lands in the pool's root ring), then materialize intents for the
/// first `n`. Recipients are signer keypairs (index-aligned), so cancel tests
/// work too.
fn fixture_from_cache(
    materials: &'static [CachedMaterial],
    k_floor: u16,
    n: usize,
    action_kind: u8,
) -> (RoundFixture, Vec<Keypair>) {
    assert!(n <= MAX_CACHED_INTENTS, "cache holds {MAX_CACHED_INTENTS}");
    let (denomination, fee, mut validator) = match action_kind {
        0 => (DENOMINATION, FEE, Pubkey::default()),
        1 => (
            stake_pool_denomination(STAKE_FEE),
            STAKE_FEE,
            Pubkey::default(),
        ),
        _ => unreachable!("test fixture supports action kinds 0/1"),
    };

    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.add_program_from_file(program_id(), so_path()).unwrap();
    if action_kind == 1 {
        validator = create_validator_vote_account(&mut svm, &payer);
    }

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &program_id());
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &program_id());
    let (round0, _) = Pubkey::find_program_address(
        &[b"round", pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );

    let mut data = disc("initialize_pool").to_vec();
    data.extend_from_slice(&denomination.to_le_bytes());
    data.extend_from_slice(&k_floor.to_le_bytes());
    data.push(action_kind);
    data.extend_from_slice(&validator.to_bytes());
    data.extend_from_slice(&fee.to_le_bytes());
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

    // Deposit every cached commitment so the pool's current root equals the
    // root all cached proofs were generated against.
    for m in materials.iter() {
        let mut d = disc("deposit").to_vec();
        d.extend_from_slice(&m.note.commitment());
        d.extend_from_slice(&denomination.to_le_bytes());
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

    let mut intents = Vec::with_capacity(n);
    let mut recipient_keypairs = Vec::with_capacity(n);
    for m in materials.iter().take(n) {
        let recipient_kp = Keypair::from_bytes(&m.recipient_keypair).unwrap();
        svm.airdrop(&recipient_kp.pubkey(), 1_000_000).unwrap();
        let (intent_pda, _) = Pubkey::find_program_address(
            &[b"intent", pool.as_ref(), m.nullifier_hash.as_ref()],
            &program_id(),
        );
        let (nullifier_pda, _) = Pubkey::find_program_address(
            &[b"nullifier", pool.as_ref(), m.nullifier_hash.as_ref()],
            &program_id(),
        );
        intents.push(IntentMaterial {
            note: m.note,
            proof: m.proof.clone(),
            root: m.root,
            nullifier_hash: m.nullifier_hash,
            recipient: recipient_kp.pubkey(),
            relayer: m.relayer,
            fee: m.fee,
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
            validator,
        },
        recipient_keypairs,
    )
}

pub fn build_round_fixture_cached(k_floor: u16, n: usize) -> (RoundFixture, Vec<Keypair>) {
    fixture_from_cache(cached_withdraw_materials(), k_floor, n, 0)
}

pub fn build_stake_round_fixture_cached(k_floor: u16, n: usize) -> (RoundFixture, Vec<Keypair>) {
    fixture_from_cache(cached_stake_materials(), k_floor, n, 1)
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
