pragma circom 2.1.6;
include "poseidon.circom";
include "merkle_proof.circom";

template Withdraw(depth) {
    signal input root;                 // public
    signal input nullifierHash;        // public
    signal input nullifier;            // private
    signal input secret;               // private
    signal input pathElements[depth];  // private
    signal input pathIndices[depth];   // private

    component cm = Poseidon(2);         // commitment = Poseidon(nullifier, secret)
    cm.inputs[0] <== nullifier; cm.inputs[1] <== secret;

    component nh = Poseidon(1);         // nullifierHash = Poseidon(nullifier)
    nh.inputs[0] <== nullifier; nh.out === nullifierHash;

    component mp = MerkleProof(depth);  // membership
    mp.leaf <== cm.out;
    for (var i = 0; i < depth; i++) { mp.pathElements[i] <== pathElements[i]; mp.pathIndices[i] <== pathIndices[i]; }
    mp.root === root;
}
component main {public [root, nullifierHash]} = Withdraw(20);
