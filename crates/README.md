# crates/

The host-side (non-SBF) crates around the on-chain program. They fall into three groups: the
**client machinery** to *use* the pool, **build-time tooling** that keeps the circuit and the
chain in lockstep, and an **analysis instrument**.

The organizing principle: **there is exactly one canonical implementation of every hash and byte
format**, shared across the program, the client, and the circuit's witness generator. In a
custody protocol, an off-chain value that silently disagrees with what the chain recomputes means
an honest proof is rejected — or worse — so nothing here re-derives a hash the program already
defines.

| Crate | Role | Group |
|---|---|---|
| [`sdk`](sdk) | Client library: creates `Note`s, rebuilds the Merkle tree client-side, and builds every Solana instruction (deposit/commit/execute/cancel). Calls the program's hashes *directly* — never re-derives. | client |
| [`prover`](prover) | The Rust ZK prover (`ark-circom` + `ark-groth16`): generates the membership proof in the exact big-endian byte format the on-chain verifier accepts. | client |
| [`ext-data`](ext-data) | The single shared `extDataHash` — binds a withdraw's recipient + relayer + fee into one field element, so a relayer can't redirect the payout. Shipped with a committed known-answer test. | shared |
| [`vk-gen`](vk-gen) | Build-time codegen: converts the circuit's verifying key (`verification_key.json`) into the Rust constant the program embeds (`vk.rs`), and a CI drift-guard. | build tool |
| [`parity-fixtures`](parity-fixtures) | Build/test tool: generates the circuit's witness inputs from the *same* canonical Rust hashes the chain uses — this is what proves the circom circuit is byte-identical to the on-chain hashing. | build tool |
| [`effective-k`](effective-k) | The honesty instrument: measures a round's *real* (min-entropy) anonymity from a ground-truth funder→note composition. Host-only analysis — never an on-chain gate. | analysis |

**Dependency spine** (everything routes back to one implementation): `pool-program → ext-data`;
`sdk → pool-program + ext-data + prover`; `parity-fixtures → pool-program + ext-data`;
`vk-gen → prover`.

All crates are `publish = false`, MIT, dependency-minimal (no `solana`/`anchor` deps in the pure
ones like `ext-data` and `effective-k`). The workspace `members = ["programs/*", "crates/*"]` glob
picks them up automatically.

Each crate's `//!` module-doc header (top of its `src/lib.rs` / `src/main.rs`) is its full description.
