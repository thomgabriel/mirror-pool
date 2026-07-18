//! Golden-fixture generator for circuit↔chain hash parity.
//!
//! Emits the canonical Poseidon(1)/Poseidon(2), Merkle-root, and `extDataHash` values —
//! computed with `pool-program`'s own `sol_poseidon`/`merkle` and `ext-data`, i.e. the exact
//! implementations the on-chain program checks — which the circom parity tests
//! (`circuits/test/*_parity`, `withdraw`) assert against. This is how "the circuit's hashing is
//! byte-identical to what the chain recomputes" is kept true; any drift here would silently
//! break every honest membership proof. Build/test tool — never deployed.

use pool_program::merkle::{empty_root, insert, zeros, TREE_HEIGHT};
use pool_program::poseidon::{hash1, hash2};

fn fe(bytes: [u8; 32]) -> [u8; 32] {
    bytes
}
fn feb(n: u8) -> [u8; 32] {
    let mut b = [0u8; 32];
    b[31] = n;
    b
}
fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Canonical nullifier hash — single-input Poseidon, same params as hash2.
fn nullifier_hash(nullifier: &[u8; 32]) -> [u8; 32] {
    hash1(nullifier).expect("in-field")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("poseidon-vectors") | None => {
            // include a high-order-byte case so any future BE/LE regression is caught.
            let mut hi = [0u8; 32];
            hi[0] = 0x12;
            hi[1] = 0x34;
            hi[31] = 0x56;
            let two = [(feb(1), feb(2)), (feb(0), feb(0)), (fe(hi), feb(9))];
            let one = [feb(1), fe(hi), feb(7)];
            let mut out = String::from("{\"poseidon2\":[");
            for (i, (a, b)) in two.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!(
                    "{{\"a\":\"{}\",\"b\":\"{}\",\"h\":\"{}\"}}",
                    hex(a),
                    hex(b),
                    hex(&hash2(a, b).expect("in-field"))
                ));
            }
            out.push_str("],\"poseidon1\":[");
            for (i, x) in one.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!(
                    "{{\"x\":\"{}\",\"h\":\"{}\"}}",
                    hex(x),
                    hex(&nullifier_hash(x))
                ));
            }
            out.push_str("]}");
            println!("{out}");
        }
        Some("note-bundle") => {
            let z = zeros().expect("zeros");
            let mut next_index = 0u32;
            let mut root = empty_root(&z).expect("empty_root");
            let mut filled = z;

            // decoy leaf at index 0 (so the real note is index 1 -> right child at level 0)
            let decoy = hash2(&feb(111), &feb(222)).expect("in-field");
            insert(&mut next_index, &mut root, &mut filled, decoy).expect("insert decoy");

            // real note
            let nullifier = feb(7);
            let secret = feb(9);
            let commitment = hash2(&nullifier, &secret).expect("in-field");

            // path for the note's index from the snapshot BEFORE its insert
            let filled_before = filled;
            let target = next_index; // 1
            let mut path_elements = [[0u8; 32]; TREE_HEIGHT];
            let mut path_indices = [0u8; TREE_HEIGHT];
            let mut idx = target;
            for i in 0..TREE_HEIGHT {
                let bit = (idx % 2) as u8;
                path_indices[i] = bit;
                path_elements[i] = if bit == 0 { z[i] } else { filled_before[i] };
                idx /= 2;
            }
            // authoritative root from the real insert
            insert(&mut next_index, &mut root, &mut filled, commitment).expect("insert note");

            let nh = nullifier_hash(&nullifier);
            let pe = path_elements
                .iter()
                .map(hex)
                .collect::<Vec<_>>()
                .join("\",\"");
            let pi = path_indices
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(",");

            // Example payout accounts for the withdraw's extDataHash binding
            // (arbitrary but fixed, so the fixture is reproducible).
            let recipient = [0x11u8; 32];
            let relayer = [0x22u8; 32];
            let fee: u64 = 1_000;
            let ext_data_hash = ext_data::ext_data_hash(&recipient, &relayer, fee);

            println!(
                "{{\"nullifier\":\"{}\",\"secret\":\"{}\",\"commitment\":\"{}\",\"nullifierHash\":\"{}\",\"root\":\"{}\",\"pathElements\":[\"{}\"],\"pathIndices\":[{}],\"recipient\":\"{}\",\"relayer\":\"{}\",\"fee\":{},\"extDataHash\":\"{}\"}}",
                hex(&nullifier), hex(&secret), hex(&commitment), hex(&nh), hex(&root), pe, pi,
                hex(&recipient), hex(&relayer), fee, hex(&ext_data_hash)
            );
        }
        Some(other) => {
            eprintln!("unknown subcommand: {other}");
            std::process::exit(2);
        }
    }
}
