const { wasm } = require("circom_tester");
const path = require("path"); const fs = require("fs");
const { CIRCOMLIB, beHexToDec } = require("./util");

describe("Withdraw circuit: Merkle membership + nullifier derivation", () => {
  const v = JSON.parse(fs.readFileSync(path.join(__dirname, "withdraw_vectors.json")));
  let circuit;

  const input = () => ({
    root: beHexToDec(v.root),
    nullifierHash: beHexToDec(v.nullifierHash),
    extDataHash: beHexToDec(v.extDataHash),
    nullifier: beHexToDec(v.nullifier),
    secret: beHexToDec(v.secret),
    pathElements: v.pathElements.map(beHexToDec),
    pathIndices: v.pathIndices,
  });

  before(async () => {
    circuit = await wasm(path.join(__dirname, "../circom/withdraw.circom"), { include: CIRCOMLIB });
  });

  it("accepts the committed note bundle (commitment, nullifierHash, and root all bind)", async () => {
    const w = await circuit.calculateWitness(input(), true);
    await circuit.checkConstraints(w);
  });

  it("rejects a witness with a wrong public nullifierHash", async () => {
    const bad = input();
    bad.nullifierHash = beHexToDec("0".repeat(63) + "1"); // != Poseidon(nullifier)
    let threw = false;
    try {
      await circuit.calculateWitness(bad, true);
    } catch (e) {
      threw = true;
    }
    if (!threw) throw new Error("expected witness calculation to reject a forged nullifierHash, but it succeeded");
  });

  it("rejects a witness with a corrupted Merkle path element (root won't match)", async () => {
    const bad = input();
    bad.pathElements = [...bad.pathElements];
    bad.pathElements[0] = (BigInt(bad.pathElements[0]) + 1n).toString(); // corrupt one sibling
    let threw = false;
    try {
      await circuit.calculateWitness(bad, true);
    } catch (e) {
      threw = true;
    }
    if (!threw) throw new Error("expected witness calculation to reject a corrupted Merkle path, but it succeeded");
  });

  it("accepts any extDataHash value (bound into the proof, not constrained in-circuit)", async () => {
    const changed = input();
    changed.extDataHash = beHexToDec("0".repeat(63) + "1");
    const w = await circuit.calculateWitness(changed, true);
    await circuit.checkConstraints(w);
  });
});
