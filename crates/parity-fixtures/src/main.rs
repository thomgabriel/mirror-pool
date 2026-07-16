use pool_program::poseidon::hash2;
use solana_poseidon::{hashv, Endianness, Parameters};

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
    hashv(
        Parameters::Bn254X5,
        Endianness::BigEndian,
        &[nullifier.as_slice()],
    )
    .expect("in-field")
    .to_bytes()
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
        Some(other) => {
            eprintln!("unknown subcommand: {other}");
            std::process::exit(2);
        }
    }
}
