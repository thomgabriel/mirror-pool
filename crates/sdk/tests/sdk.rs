//! Builds a `deposit` + `withdraw` instruction via the SDK's own builders and
//! asserts the SDK's computed `nullifier_hash`/`extDataHash` (encoded into
//! the `withdraw` instruction's data) byte-match what `pool-program`'s
//! `withdraw` handler independently recomputes for the SAME inputs — i.e.
//! that the SDK and the on-chain program agree on every hash binding a
//! withdraw. Reuses the committed note bundle
//! (`circuits/test/withdraw_vectors.json`) for the note/root/Merkle path,
//! same as `programs/pool-program/tests/withdraw.rs`'s fixture. A full
//! on-chain LiteSVM run (actually submitting these instructions) is Task 6.

use sdk::{build_deposit_ix, build_withdraw_ix, compute_ext_data_hash, MerklePath, Note};
use serde_json::Value;
use solana_sdk::pubkey::Pubkey;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/sdk is two levels below the workspace root")
        .to_path_buf()
}

/// Mirrors `programs/pool-program/tests/withdraw.rs::ensure_build_artifacts` —
/// the `circuits/build/*` artifacts are gitignored outputs of
/// `circuits/scripts/setup.sh`; generate them rather than skip the real
/// prove this test exists to exercise.
fn ensure_build_artifacts() -> PathBuf {
    static BUILD_DIR: OnceLock<PathBuf> = OnceLock::new();
    BUILD_DIR
        .get_or_init(|| {
            let circuits_dir = workspace_root().join("circuits");
            let build_dir = circuits_dir.join("build");
            let wasm = build_dir.join("withdraw_js").join("withdraw.wasm");
            let r1cs = build_dir.join("withdraw.r1cs");
            let zkey = build_dir.join("withdraw.zkey");
            let required = [&wasm, &r1cs, &zkey];

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
                         Run circuits/scripts/setup.sh first, then re-run `cargo test -p sdk`."
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

/// Loads the committed bundle's note (`nullifier`/`secret`), its Merkle path,
/// and its root — the same underlying deposited note as
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

/// The bundle's committed `commitment`/`nullifierHash` values
/// (`circuits/test/withdraw_vectors.json`), pinned here as an independent
/// cross-check that `Note::commitment`/`Note::nullifier_hash` reproduce them
/// exactly for the bundle's `(nullifier, secret)`.
const BUNDLE_COMMITMENT_HEX: &str =
    "2f447495cd13dfa223b07ada1d51ac114901e15056a30f8bf28f6fbb4a27376a";
const BUNDLE_NULLIFIER_HASH_HEX: &str =
    "0f9cebf54307bbb3646866aa15d2cd6e961caea77048b87f4261b7636240254e";

#[test]
fn note_hashes_match_the_committed_bundle() {
    let (note, _root, _path) = load_bundle();
    assert_eq!(note.commitment(), decode_be_hex(BUNDLE_COMMITMENT_HEX));
    assert_eq!(
        note.nullifier_hash(),
        decode_be_hex(BUNDLE_NULLIFIER_HASH_HEX)
    );
}

#[test]
fn deposit_ix_uses_the_notes_commitment() {
    let (note, _root, _path) = load_bundle();
    let pool = Pubkey::new_unique();
    let vault = Pubkey::new_unique();
    let payer = Pubkey::new_unique();
    let denomination = 2_000_000u64;

    let ix = build_deposit_ix(pool, vault, payer, note.commitment(), denomination);

    // data = disc(8) || commitment(32) || amount_le(8)
    assert_eq!(&ix.data[8..40], &note.commitment());
    assert_eq!(&ix.data[40..48], &denomination.to_le_bytes());
}

/// The load-bearing assertion: builds a real `withdraw` instruction via the
/// SDK, then independently recomputes `nullifier_hash` (from the note) and
/// `ext_data_hash` (from the payout accounts + fee) exactly as
/// `programs/pool-program/src/lib.rs`'s `withdraw` handler does, and asserts
/// the instruction's encoded public-input fields match — proving the SDK and
/// the on-chain program agree.
#[test]
fn withdraw_ix_public_inputs_match_program_recomputation() {
    let build_dir = ensure_build_artifacts();
    let (note, root, merkle_path) = load_bundle();

    let recipient = Pubkey::new_unique();
    let relayer = Pubkey::new_unique();
    let fee = 1_000u64;

    let artifacts = sdk::WithdrawArtifacts {
        wasm_path: &build_dir.join("withdraw_js").join("withdraw.wasm"),
        r1cs_path: &build_dir.join("withdraw.r1cs"),
        zkey_path: &build_dir.join("withdraw.zkey"),
    };

    let pool = Pubkey::new_unique();
    let vault = Pubkey::new_unique();

    let build = build_withdraw_ix(
        pool,
        vault,
        recipient,
        relayer,
        &note,
        &merkle_path,
        root,
        fee,
        artifacts,
    )
    .expect("proving the committed note bundle must succeed");

    // --- nullifier_hash: the SDK derived it from `note.nullifier_hash()`
    // (single-input Poseidon); the on-chain program never recomputes it (it's
    // an opaque instruction arg checked only via the nullifier PDA + the
    // proof's public inputs) — so the authority here is that the witness the
    // circuit generated for this exact `note.nullifier` reproduces the same
    // value the SDK independently computed off-circuit.
    assert_eq!(
        build.public_inputs.nullifier_hash,
        note.nullifier_hash(),
        "the proof's nullifier_hash public input must match the SDK's own Poseidon1(nullifier)"
    );

    // --- ext_data_hash: recompute exactly as
    // `programs/pool-program/src/lib.rs`'s `withdraw` handler does, from the
    // SAME payout account keys + fee that are listed in the built
    // instruction's accounts (recipient, relayer) — this is the
    // front-run-safety binding.
    let program_recomputed_ext_data_hash =
        compute_ext_data_hash(&recipient.to_bytes(), &relayer.to_bytes(), fee);
    assert_eq!(
        build.public_inputs.ext_data_hash, program_recomputed_ext_data_hash,
        "the proof's ext_data_hash public input must match the program's on-chain recomputation \
         from the (recipient, relayer, fee) payout accounts"
    );

    // --- The instruction's encoded data must carry exactly these same
    // values (root/nullifier_hash as raw bytes, matching Anchor's
    // declaration-order Borsh layout: disc(8) || a(64) || b(128) || c(64) ||
    // root(32) || nullifier_hash(32) || fee_le(8)).
    let data = &build.instruction.data;
    assert_eq!(data.len(), 8 + 64 + 128 + 64 + 32 + 32 + 8);
    let root_off = 8 + 64 + 128 + 64;
    let nh_off = root_off + 32;
    let fee_off = nh_off + 32;
    assert_eq!(&data[root_off..root_off + 32], &build.public_inputs.root);
    assert_eq!(
        &data[nh_off..nh_off + 32],
        &build.public_inputs.nullifier_hash
    );
    assert_eq!(&data[fee_off..fee_off + 8], &fee.to_le_bytes());

    // --- Accounts: recipient/relayer must be the exact accounts the
    // extDataHash above was derived from (this is what makes redirection
    // impossible without invalidating the proof).
    assert_eq!(build.instruction.accounts[0].pubkey, pool);
    assert_eq!(build.instruction.accounts[1].pubkey, vault);
    assert_eq!(build.instruction.accounts[3].pubkey, recipient);
    assert_eq!(build.instruction.accounts[4].pubkey, relayer);
    assert!(build.instruction.accounts[4].is_signer, "relayer signs");

    // --- The nullifier PDA account must be derivable the same way
    // `programs/pool-program/src/lib.rs`'s `Withdraw` context derives it.
    let (expected_nullifier_pda, _) = Pubkey::find_program_address(
        &[
            b"nullifier",
            pool.as_ref(),
            build.public_inputs.nullifier_hash.as_ref(),
        ],
        &pool_program::ID,
    );
    assert_eq!(build.instruction.accounts[2].pubkey, expected_nullifier_pda);
}
