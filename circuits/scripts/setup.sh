#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p build
PTAU="ptau/pot14_final.ptau"   # DEV ONLY — public Hermez powers-of-tau, NOT a production ceremony
PTAU_SHA256="489be9e5ac65d524f7b1685baac8a183c6e77924fdb73d2b8105e335f277895d"   # see ptau/README.md

if [ ! -f "$PTAU" ]; then
  echo "missing $PTAU — see ptau/README.md to fetch it" >&2
  exit 1
fi

# A same-power ptau swap still yields a "valid" VK (proof/verify consistency
# doesn't depend on ptau authenticity) but a different, non-reproducible one —
# so the file's authenticity must be checked, not just its presence.
if command -v shasum >/dev/null 2>&1; then
  PTAU_ACTUAL="$(shasum -a 256 "$PTAU" | cut -d' ' -f1)"
else
  PTAU_ACTUAL="$(sha256sum "$PTAU" | cut -d' ' -f1)"
fi
if [ "$PTAU_ACTUAL" != "$PTAU_SHA256" ]; then
  echo "sha256 mismatch for $PTAU — expected $PTAU_SHA256, got $PTAU_ACTUAL" >&2
  exit 1
fi

circom circom/membership.circom --r1cs --wasm --sym -l node_modules/circomlib/circuits -o build
# circom emits build/membership_js/membership.wasm (+ generate_witness.js) and build/membership.r1cs
npx snarkjs r1cs info build/membership.r1cs   # prints constraint count — confirm it fits 2^14

npx snarkjs groth16 setup build/membership.r1cs "$PTAU" build/membership_0000.zkey
# DETERMINISTIC dev contribution via beacon (repeatable given fixed beacon+iters):
npx snarkjs zkey beacon build/membership_0000.zkey build/membership.zkey \
    0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20 10 -n="dev-beacon"
npx snarkjs zkey export verificationkey build/membership.zkey build/verification_key.json
echo "wrote build/membership_js/membership.wasm, build/membership.zkey, build/verification_key.json"
