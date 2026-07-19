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

#![allow(deprecated)] // `stake::config::ID` (see round_support.rs's identical allow)

use litesvm::LiteSVM;
use sdk::{
    build_commit_intent_ix, build_deposit_ix, build_execute_round_ix, build_execute_stake_round_ix,
    build_initialize_pool_ix, round_pda, stake_account_pda, MembershipArtifacts, MerkleTree, Note,
};
use solana_sdk::{
    account::ReadableAccount,
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    message::Message,
    pubkey::Pubkey,
    rent::Rent,
    signature::{Keypair, Signer},
    stake::state::StakeStateV2,
    system_instruction, system_program,
    transaction::Transaction,
    vote::{
        instruction::{create_account_with_config, CreateVoteAccountConfig},
        state::{VoteInit, VoteStateVersions},
    },
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
            let wasm = build_dir.join("membership_js").join("membership.wasm");
            let r1cs = build_dir.join("membership.r1cs");
            let zkey = build_dir.join("membership.zkey");
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

/// Create a real, delegable validator vote account: a funded node identity plus
/// a Vote-program-owned account initialized via the real `CreateAccount` +
/// `InitializeAccount` CPI pair (not a hand-serialized `VoteState`), so
/// `DelegateStake` accepts it exactly as it would on a live cluster. This is a
/// standalone copy of `programs/pool-program/tests/round_support.rs`'s
/// `create_validator_vote_account` — this test lives in a different crate
/// (`crates/sdk`) so it can't import that test-only helper. Returns the vote
/// account's pubkey (the pool's `validator`).
fn create_validator_vote_account(svm: &mut LiteSVM, payer: &Keypair) -> Pubkey {
    let node = Keypair::new();
    let vote_account = Keypair::new();
    let rent = Rent::default();
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
        0,
        Pubkey::default(),
        FEE,
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
            MembershipArtifacts {
                wasm_path: &build_dir.join("membership_js").join("membership.wasm"),
                r1cs_path: &build_dir.join("membership.r1cs"),
                zkey_path: &build_dir.join("membership.zkey"),
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

/// The pooled-stake counterpart of `sdk_driven_round_trip_two_intents`: a
/// full deposit -> commit(k=2) -> execute round trip for a STAKE pool, driven
/// entirely through the SDK's public builders, asserting the on-chain
/// `StakeStateV2` is a REAL delegation (not a no-op) to the pool's validator
/// with authority handed to each intent's own recipient.
#[test]
fn sdk_driven_stake_round() {
    let build_dir = ensure_build_artifacts();
    let mut svm = LiteSVM::new();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    svm.add_program_from_file(pool_program::ID, so_path())
        .unwrap();

    let validator = create_validator_vote_account(&mut svm, &payer);

    let stake_fee = 5_000u64;
    let stake_rent = Rent::default().minimum_balance(pool_program::invariants::STAKE_ACCOUNT_SIZE);
    // Enough to clear stake_fee + stake_rent + MIN_STAKE_DELEGATION with slack
    // (mirrors `programs/pool-program/tests/round_support.rs::stake_pool_denomination`).
    let denomination =
        pool_program::invariants::MIN_STAKE_DELEGATION + stake_rent + stake_fee + 1_000_000;

    let mint = Pubkey::new_unique();
    let (pool, _) = Pubkey::find_program_address(&[b"pool", mint.as_ref()], &pool_program::ID);
    let (vault, _) = Pubkey::find_program_address(&[b"vault", pool.as_ref()], &pool_program::ID);

    // init(k_floor=2, action_kind=1/Stake, validator, stake_fee)
    let init = build_initialize_pool_ix(
        pool,
        vault,
        round_pda(pool, 0),
        mint,
        payer.pubkey(),
        denomination,
        2,
        1,
        validator,
        stake_fee,
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
            build_deposit_ix(pool, vault, payer.pubkey(), note.commitment(), denomination),
        );
    }
    let root = tree.root();

    // commit both — extDataHash now binds the stake authority (recipient == the
    // future staker/withdrawer authority handed over by StakeAction's Authorize CPI).
    let mut triples = Vec::new();
    let mut recipients = Vec::new();
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
            stake_fee,
            0,
            MembershipArtifacts {
                wasm_path: &build_dir.join("membership_js").join("membership.wasm"),
                r1cs_path: &build_dir.join("membership.r1cs"),
                zkey_path: &build_dir.join("membership.zkey"),
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
        triples.push((intent, stake_account_pda(pool, intent), relayer));
        recipients.push(recipient);
    }

    // execute (stake layout)
    let cranker = Keypair::new();
    svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    svm.expire_blockhash();
    let exec = build_execute_stake_round_ix(pool, vault, cranker.pubkey(), 0, validator, &triples);
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            exec,
        ],
        Some(&cranker.pubkey()),
    );
    svm.send_transaction(Transaction::new(&[&cranker], msg, svm.latest_blockhash()))
        .expect("a full k-round stake execution, built entirely through the SDK, must execute");

    for ((_, stake_pda, _), recipient) in triples.iter().zip(recipients.iter()) {
        let acct = svm.get_account(stake_pda).unwrap();
        assert_eq!(
            acct.owner,
            solana_sdk::stake::program::ID,
            "stake account owned by the Stake program"
        );
        match bincode::deserialize::<StakeStateV2>(&acct.data).unwrap() {
            StakeStateV2::Stake(meta, stake, _) => {
                assert_eq!(
                    stake.delegation.voter_pubkey, validator,
                    "delegated to the pool's validator"
                );
                assert_eq!(
                    meta.authorized.staker, *recipient,
                    "staker authority handed to the recipient post-Authorize"
                );
                assert_eq!(
                    meta.authorized.withdrawer, *recipient,
                    "withdrawer authority is the recipient"
                );
            }
            other => panic!("expected StakeStateV2::Stake, got {other:?}"),
        }
    }
}
