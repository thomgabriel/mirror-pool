pragma circom 2.1.6;
include "poseidon.circom";
template Poseidon1() { signal input in[1]; signal output out;
    component h = Poseidon(1); h.inputs[0] <== in[0]; out <== h.out; }
component main = Poseidon1();
