#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p build
PTAU="ptau/pot14_final.ptau"   # DEV ONLY — public Hermez powers-of-tau, NOT a production ceremony

if [ ! -f "$PTAU" ]; then
  echo "missing $PTAU — see ptau/README.md to fetch it" >&2
  exit 1
fi

circom circom/withdraw.circom --r1cs --wasm --sym -l node_modules/circomlib/circuits -o build
# circom emits build/withdraw_js/withdraw.wasm (+ generate_witness.js) and build/withdraw.r1cs
npx snarkjs r1cs info build/withdraw.r1cs   # prints constraint count — confirm it fits 2^14

npx snarkjs groth16 setup build/withdraw.r1cs "$PTAU" build/withdraw_0000.zkey
# DETERMINISTIC dev contribution via beacon (repeatable given fixed beacon+iters):
npx snarkjs zkey beacon build/withdraw_0000.zkey build/withdraw.zkey \
    0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20 10 -n="dev-beacon"
npx snarkjs zkey export verificationkey build/withdraw.zkey build/verification_key.json
echo "wrote build/withdraw_js/withdraw.wasm, build/withdraw.zkey, build/verification_key.json"
