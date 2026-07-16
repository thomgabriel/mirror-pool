const CIRCOMLIB = ["node_modules/circomlib/circuits"]; // pinned include path
const beHexToDec = (hex) => BigInt("0x" + hex).toString();
const bigToBeHex = (x) => (typeof x === "bigint" ? x : BigInt(x)).toString(16).padStart(64, "0");
module.exports = { CIRCOMLIB, beHexToDec, bigToBeHex };
