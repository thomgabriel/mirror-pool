const { wasm } = require("circom_tester");
const path = require("path"); const fs = require("fs");
const { CIRCOMLIB, beHexToDec, bigToBeHex } = require("./util");

describe("Merkle parity: circom MerkleProof vs on-chain incremental tree", () => {
  const v = JSON.parse(fs.readFileSync(path.join(__dirname, "withdraw_vectors.json")));
  it("recomputes the on-chain root for the committed note bundle", async () => {
    const c = await wasm(path.join(__dirname, "../circom/merkle_proof.circom"), { include: CIRCOMLIB });
    const w = await c.calculateWitness(
      {
        leaf: beHexToDec(v.commitment),
        pathElements: v.pathElements.map(beHexToDec),
        pathIndices: v.pathIndices,
      },
      true
    );
    if (bigToBeHex(w[1]) !== v.root) throw new Error(`root mismatch: got ${bigToBeHex(w[1])}, want ${v.root}`);
  });
});
