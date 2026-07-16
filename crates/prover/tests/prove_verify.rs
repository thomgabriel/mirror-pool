//! Load-bearing end-to-end test: prove the withdraw circuit from the
//! committed note bundle and verify the real proof both against
//! `ark-groth16`/`verification_key.json` and (where feasible) the
//! `groth16-solana` on-chain verifier byte format.

use ark_bn254::{Bn254, Fq, Fq2, G1Affine, G1Projective, G2Affine, G2Projective};
use ark_groth16::{Groth16, VerifyingKey};
use groth16_solana::groth16::{Groth16Verifier, Groth16Verifyingkey};
use prover::{FieldBytes, WithdrawInputs, TREE_DEPTH};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/prover is two levels below the workspace root")
        .to_path_buf()
}

/// The circuits/build/* artifacts are gitignored build products of
/// `circuits/scripts/setup.sh` (circom compile + trusted setup). If they're
/// missing, run the setup script rather than silently skipping the real
/// prove/verify this test exists to exercise.
///
/// Both `#[test]` functions in this binary call this, and the default test
/// harness runs them concurrently — without serialization, two threads would
/// race to spawn `setup.sh` against the same output paths and `circom` would
/// clobber itself. `OnceLock::get_or_init` runs the closure exactly once and
/// blocks the other caller until it completes, so the build happens a single
/// time no matter how many tests need it.
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
                         Run circuits/scripts/setup.sh first, then re-run `cargo test -p prover`."
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
        nullifier: str_field("nullifier"),
        secret: str_field("secret"),
        path_elements: path_elements.try_into().unwrap(),
        path_indices: path_indices.try_into().unwrap(),
    }
}

fn fq_from_decimal(s: &str) -> Fq {
    use std::str::FromStr;
    Fq::from(num_bigint::BigUint::from_str(s).expect("decimal field element"))
}

fn g1_from_json(el: &Value) -> G1Affine {
    let a = el.as_array().unwrap();
    let x = fq_from_decimal(a[0].as_str().unwrap());
    let y = fq_from_decimal(a[1].as_str().unwrap());
    let z = fq_from_decimal(a[2].as_str().unwrap());
    G1Affine::from(G1Projective::new(x, y, z))
}

fn g2_from_json(el: &Value) -> G2Affine {
    let a = el.as_array().unwrap();
    let coord = |v: &Value| -> Fq2 {
        let c = v.as_array().unwrap();
        Fq2::new(
            fq_from_decimal(c[0].as_str().unwrap()),
            fq_from_decimal(c[1].as_str().unwrap()),
        )
    };
    G2Affine::from(G2Projective::new(coord(&a[0]), coord(&a[1]), coord(&a[2])))
}

/// Parses a snarkjs-exported `verification_key.json` (groth16/bn128) into an
/// `ark_groth16::VerifyingKey`. Test-only: the library itself doesn't need a
/// JSON parser (`serde_json` is a dev-dependency), the test does since it
/// exercises the exact artifact Plan 3 will embed on-chain.
fn load_verification_key(path: &Path) -> VerifyingKey<Bn254> {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let v: Value = serde_json::from_str(&raw).expect("valid JSON");
    assert_eq!(v["protocol"], "groth16");
    assert_eq!(v["curve"], "bn128");

    VerifyingKey {
        alpha_g1: g1_from_json(&v["vk_alpha_1"]),
        beta_g2: g2_from_json(&v["vk_beta_2"]),
        gamma_g2: g2_from_json(&v["vk_gamma_2"]),
        delta_g2: g2_from_json(&v["vk_delta_2"]),
        gamma_abc_g1: v["IC"]
            .as_array()
            .unwrap()
            .iter()
            .map(g1_from_json)
            .collect(),
    }
}

#[test]
fn real_proof_verifies_and_rejects_a_tampered_public_input() {
    let build_dir = ensure_build_artifacts();
    let bundle = load_bundle();

    let (proof, public_inputs) = prover::prove_withdraw(
        build_dir.join("withdraw_js").join("withdraw.wasm"),
        build_dir.join("withdraw.r1cs"),
        build_dir.join("withdraw.zkey"),
        &bundle,
    )
    .expect("proving the committed note bundle must succeed");

    // The witness's own public outputs must match the bundle's committed
    // root/nullifierHash — the circuit's `===` constraints guarantee this,
    // but assert it directly so a future circuit change can't silently drift.
    assert_eq!(public_inputs.root, bundle.root);
    assert_eq!(public_inputs.nullifier_hash, bundle.nullifier_hash);

    let vk = load_verification_key(&build_dir.join("verification_key.json"));
    let pvk = ark_groth16::prepare_verifying_key(&vk);

    let verified = Groth16::<Bn254>::verify_proof(&pvk, &proof, &public_inputs.as_fr())
        .expect("verify_proof itself must not error on a well-formed proof");
    assert!(
        verified,
        "a real proof for the committed bundle must verify"
    );

    // Tamper: swap in a nullifierHash that isn't Poseidon(nullifier). The
    // proof was generated for the real value, so the pairing check must fail.
    let mut forged = public_inputs.clone();
    forged.nullifier_hash[31] ^= 0x01;
    let forged_verified = Groth16::<Bn254>::verify_proof(&pvk, &proof, &forged.as_fr())
        .expect("verify_proof itself must not error on a mismatched public input");
    assert!(
        !forged_verified,
        "a proof must NOT verify against a tampered public nullifierHash"
    );
}

#[test]
fn real_proof_verifies_against_the_groth16_solana_on_chain_byte_format() {
    let build_dir = ensure_build_artifacts();
    let bundle = load_bundle();

    let (proof, public_inputs) = prover::prove_withdraw(
        build_dir.join("withdraw_js").join("withdraw.wasm"),
        build_dir.join("withdraw.r1cs"),
        build_dir.join("withdraw.zkey"),
        &bundle,
    )
    .expect("proving the committed note bundle must succeed");

    let vk = load_verification_key(&build_dir.join("verification_key.json"));

    let proof_a = prover::proof_a_to_solana_be(&proof.a).unwrap();
    let proof_b = prover::g2_to_solana_be(&proof.b).unwrap();
    let proof_c = prover::g1_to_solana_be(&proof.c).unwrap();

    let vk_alpha_g1 = prover::g1_to_solana_be(&vk.alpha_g1).unwrap();
    let vk_beta_g2 = prover::g2_to_solana_be(&vk.beta_g2).unwrap();
    let vk_gamma_g2 = prover::g2_to_solana_be(&vk.gamma_g2).unwrap();
    let vk_delta_g2 = prover::g2_to_solana_be(&vk.delta_g2).unwrap();
    let vk_ic: Vec<[u8; 64]> = vk
        .gamma_abc_g1
        .iter()
        .map(|p| prover::g1_to_solana_be(p).unwrap())
        .collect();

    let solana_vk = Groth16Verifyingkey {
        nr_pubinputs: 2,
        vk_alpha_g1,
        vk_beta_g2,
        vk_gamme_g2: vk_gamma_g2,
        vk_delta_g2,
        vk_ic: &vk_ic,
    };

    let public_inputs_be: [[u8; 32]; 2] = [public_inputs.root, public_inputs.nullifier_hash];

    let mut verifier =
        Groth16Verifier::new(&proof_a, &proof_b, &proof_c, &public_inputs_be, &solana_vk)
            .expect("well-formed proof/VK byte lengths");
    verifier
        .verify()
        .expect("the on-chain byte-format verifier must accept our own proof");

    // Tamper check mirrors the ark-groth16 test above, against the on-chain
    // verifier's own byte-level API this time.
    let mut forged_public_inputs_be = public_inputs_be;
    forged_public_inputs_be[1][31] ^= 0x01;
    let mut forged_verifier = Groth16Verifier::new(
        &proof_a,
        &proof_b,
        &proof_c,
        &forged_public_inputs_be,
        &solana_vk,
    )
    .expect("well-formed proof/VK byte lengths");
    assert!(
        forged_verifier.verify().is_err(),
        "the on-chain byte-format verifier must reject a tampered public nullifierHash"
    );
}
