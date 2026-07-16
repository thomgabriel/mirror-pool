pragma circom 2.1.6;
include "merkle_proof.circom"; // standalone entry point for the Task 2 parity test;
                                // merkle_proof.circom stays include-only (no main) so
                                // withdraw.circom can include it without a duplicate main
component main = MerkleProof(20);
