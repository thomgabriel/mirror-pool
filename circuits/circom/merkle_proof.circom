pragma circom 2.1.6;
include "poseidon.circom";

template MerkleLevel() {
    signal input cur; signal input sibling; signal input index; signal output out;
    index * (index - 1) === 0;                       // boolean
    signal left; signal right;
    left  <== cur + index * (sibling - cur);         // index==0 ? cur : sibling
    right <== sibling + index * (cur - sibling);      // index==0 ? sibling : cur
    component h = Poseidon(2); h.inputs[0] <== left; h.inputs[1] <== right; out <== h.out;
}

template MerkleProof(depth) {
    signal input leaf; signal input pathElements[depth]; signal input pathIndices[depth];
    signal output root;
    component levels[depth]; signal cur[depth + 1]; cur[0] <== leaf;
    for (var i = 0; i < depth; i++) {
        levels[i] = MerkleLevel();
        levels[i].cur <== cur[i];
        levels[i].sibling <== pathElements[i];
        levels[i].index <== pathIndices[i];
        cur[i + 1] <== levels[i].out;
    }
    root <== cur[depth];
}
component main = MerkleProof(20);
