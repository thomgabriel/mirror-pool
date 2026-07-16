const { wasm } = require("circom_tester");
const path = require("path"); const fs = require("fs");
const { CIRCOMLIB, beHexToDec, bigToBeHex } = require("./util");

describe("Poseidon parity: circomlib vs on-chain solana-poseidon", () => {
  const v = JSON.parse(fs.readFileSync(path.join(__dirname, "poseidon_vectors.json")));
  it("Poseidon(2) matches hash2 vectors", async () => {
    const c = await wasm(path.join(__dirname, "../circom/poseidon2.circom"), { include: CIRCOMLIB });
    for (const { a, b, h } of v.poseidon2) {
      const w = await c.calculateWitness({ in: [beHexToDec(a), beHexToDec(b)] }, true);
      if (bigToBeHex(w[1]) !== h) throw new Error(`P2 mismatch (${a},${b})`);
    }
  });
  it("Poseidon(1) matches nullifier_hash vectors", async () => {
    const c = await wasm(path.join(__dirname, "../circom/poseidon1.circom"), { include: CIRCOMLIB });
    for (const { x, h } of v.poseidon1) {
      const w = await c.calculateWitness({ in: [beHexToDec(x)] }, true);
      if (bigToBeHex(w[1]) !== h) throw new Error(`P1 mismatch (${x})`);
    }
  });
});
