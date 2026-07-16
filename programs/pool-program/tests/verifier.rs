//! Exercises `pool_program::verifier::verify_withdraw` against a REAL Groth16
//! proof generated in-test via `prover::prove_withdraw`, verified against the
//! embedded `pool_program::vk::WITHDRAW_VK` — the same on-chain byte format
//! and VK the `withdraw` instruction (Task 3) will use.

use pool_program::verifier::{verify_withdraw, WithdrawProof};
use prover::{FieldBytes, WithdrawInputs, TREE_DEPTH};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("programs/pool-program is two levels below the workspace root")
        .to_path_buf()
}

/// Mirrors `crates/prover/tests/prove_verify.rs::ensure_build_artifacts` —
/// the `circuits/build/*` artifacts are gitignored outputs of
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

fn load_bundle() -> WithdrawInputs {
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
        ext_data_hash: str_field("extDataHash"),
        nullifier: str_field("nullifier"),
        secret: str_field("secret"),
        path_elements: path_elements.try_into().unwrap(),
        path_indices: path_indices.try_into().unwrap(),
    }
}

#[test]
fn real_proof_is_accepted_and_a_tampered_public_input_is_rejected() {
    let build_dir = ensure_build_artifacts();
    let bundle = load_bundle();

    let (proof, public_inputs) = prover::prove_withdraw(
        build_dir.join("withdraw_js").join("withdraw.wasm"),
        build_dir.join("withdraw.r1cs"),
        build_dir.join("withdraw.zkey"),
        &bundle,
    )
    .expect("proving the committed note bundle must succeed");

    let withdraw_proof = WithdrawProof {
        a: prover::proof_a_to_solana_be(&proof.a).unwrap(),
        b: prover::g2_to_solana_be(&proof.b).unwrap(),
        c: prover::g1_to_solana_be(&proof.c).unwrap(),
    };
    let public_inputs_be: [[u8; 32]; 3] = [
        public_inputs.root,
        public_inputs.nullifier_hash,
        public_inputs.ext_data_hash,
    ];

    verify_withdraw(&withdraw_proof, &public_inputs_be)
        .expect("a real proof for the committed bundle must verify against the embedded VK");

    let mut tampered = public_inputs_be;
    tampered[2][31] ^= 0x01; // flip a bit of extDataHash
    let err = verify_withdraw(&withdraw_proof, &tampered)
        .expect_err("a proof must NOT verify against a tampered public extDataHash");
    assert!(
        err.to_string().contains("proof failed verification"),
        "tampered proof must be rejected as ProofInvalid, got {err}"
    );
}
