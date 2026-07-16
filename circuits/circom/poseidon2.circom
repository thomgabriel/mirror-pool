pragma circom 2.1.6;
include "poseidon.circom"; // circomlib
template Poseidon2() { signal input in[2]; signal output out;
    component h = Poseidon(2); h.inputs[0] <== in[0]; h.inputs[1] <== in[1]; out <== h.out; }
component main = Poseidon2();
