# pool-program

The on-chain program — mirror-pool's custody vault and behavioral round engine. Anchor 0.31.1,
built for the Solana SBF target. This is the security-critical core: it holds user funds and
enforces the anonymity invariant, so it is written to *fail closed* everywhere.

## Instructions (the public API)

| Instruction | What it does |
|---|---|
| `initialize_pool` | Fix a pool's parameters *once*: denomination, `k_floor`, action kind (Withdraw/Stake), the pool-wide `fee`, and (for stake) the validator. Creates the program-owned vault PDA. |
| `deposit` | Custody the denomination and append the commitment leaf to the Poseidon accumulator (push the new root into the ring). |
| `commit_intent` | Verify a Groth16 membership proof against a *known* recent root, **atomically burn the note's nullifier** (double-spend fails closed), and record an `Intent` for the open round — payout keys bound via `extDataHash`. No payout. |
| `execute_round` | Enforce the on-chain **`k`-floor**, then execute *all* live intents in **one vault-signed batch** through the `PooledAction` trait — the uniform actor. Rolls the pool to the next round. |
| `cancel_intent` | A **timeout-gated**, recipient-authorized, single-note reclaim — a liveness valve for a round that never fills. Not `k`-anonymous (disclosed); the nullifier stays burned. |

## The invariants it enforces

- **`k`-floor on-chain** — `execute_round` never fires below `k_floor` intents. This is *the*
  behavioral-anonymity gate, and it lives here (not just in an off-chain coordinator).
- **Uniform actor** — the vault signs the entire batch; no per-intent signature leaks, and every
  action in a round is byte-shape identical (same denomination, same `fee`, so identical payouts).
- **Single-spend** — a nullifier PDA's *existence* is the spent-marker; `commit_intent`'s `init`
  fails atomically on a double-spend.
- **Value conservation** — every payout/split is a checked, host-tested pure function
  (`invariants.rs`); `overflow-checks` stay on; no path can over-drain the vault.

## Module layout

Domain logic is split by responsibility; `lib.rs` holds the Anchor instruction layer (handlers +
`Accounts` contexts + `PoolError`).

```
src/lib.rs         #[program] handlers, account contexts, PoolError, DepositEvent
src/state.rs       the zero_copy Pool custody account (bytemuck Pod — see the E0793 note)
src/round.rs       Round / Intent / ActionKind
src/action.rs      the PooledAction trait + WithdrawAction + StakeAction adapters
src/invariants.rs  pure, host-tested fns (k-floor, split_payout, stake_split, cancel_unlock_slot)
src/merkle.rs      incremental Poseidon Merkle tree (height 20)
src/poseidon.rs    Poseidon (BN254, circom-parity-verified)
src/roots.rs       bounded root-history ring (100 roots)
src/nullifier.rs   the nullifier PDA record
src/verifier.rs    groth16-solana verification glue
src/vk.rs          @generated verifying key (from crates/vk-gen — do not edit)
```

`Pool` is `zero_copy` (an `AccountLoader`, mutated in place) because it is multiple KB and would
overflow the 4 KB SBF stack if copied — see the rationale comment in `state.rs`.

## Build & test

```bash
cargo build-sbf --manifest-path programs/pool-program/Cargo.toml   # produces the deployable .so
cargo test -p pool-program                                          # host unit + LiteSVM in-VM tests
```

Coverage note: `cargo-llvm-cov` can only truthfully measure the *host* build, so all invariant
logic lives in pure `pub fn`s with host unit tests (see `docs/research/cicd-and-testing.md`).
