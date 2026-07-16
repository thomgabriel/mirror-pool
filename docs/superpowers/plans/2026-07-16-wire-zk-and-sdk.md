# wire-ZK + minimal SDK Implementation Plan (Plan 3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the two merged subsystems into a working, secure **deposit→withdraw shielded pool**: extend the withdraw circuit to bind the recipient/relayer/fee, verify the Groth16 proof **on-chain**, enforce a known root + single-spend, pay out a fixed denomination, and ship a minimal Rust SDK — proven end-to-end (deposit → real proof → withdraw → funds land; double-spend rejected).

**Architecture:** Extend `circuits/` (one new public input + VK regen), add an on-chain Groth16 verifier + `withdraw` instruction to `pool-program` (reusing `crates/prover`'s byte-layout helpers and `groth16-solana`), make the pool single-denomination, and add `crates/sdk` (note management, proof generation, instruction builders). This is subsystem 3 of Phase 1 (see spec §2); it consumes Plans 1 (pool-program) + 2 (circuits/prover), both merged to `main`.

**Tech Stack:** circom 2.1.6 · snarkjs · Anchor 0.31.1 · `groth16-solana 0.2` (on-chain verify via `alt_bn128` syscalls) · `ark-circom 0.5` (client proving) · Rust.

**Design spec:** [`../specs/2026-07-15-mirror-pool-design.md`](../specs/2026-07-15-mirror-pool-design.md) · **Depends on (merged):** [`2026-07-15-pool-program-foundations.md`](./2026-07-15-pool-program-foundations.md), [`2026-07-15-circuits.md`](./2026-07-15-circuits.md)

## Global Constraints

- **Recipient/relayer/fee binding via `extDataHash` (front-running protection):** the withdraw circuit gains ONE public input `extDataHash`. Off-chain (SDK) AND on-chain compute it identically: `extDataHash = keccak256(recipient(32) ‖ relayer(32) ‖ fee_le(8)) reduced mod BN254_MODULUS` (big-endian interpret the 32-byte keccak output, then `mod r`). A 32-byte Solana pubkey does NOT fit in one ~254-bit field element, so we bind the *hash*, not the raw pubkey. The instruction recomputes `extDataHash` from the passed `recipient`/`relayer`/`fee` and the Groth16 verification over public inputs `[root, nullifierHash, extDataHash]` fails if any of them was tampered with.
- **Public input order (MUST match circuit ↔ prover ↔ on-chain):** `[root, nullifierHash, extDataHash]`, each a 32-byte **big-endian** field element `< BN254_MODULUS`.
- **Byte layout:** reuse `crates/prover`'s existing `proof_a_to_solana_be` / `g1_to_solana_be` / `g2_to_solana_be` helpers (negated `proof.A`, BE G1/G2 with G2 `Fq2` swap) — do NOT re-derive. The embedded VK must be in the same `groth16-solana` byte format.
- **Single denomination:** the pool is single-denomination. `initialize_pool(denomination: u64)` stores it; `deposit` requires `amount == pool.denomination`; `withdraw` pays `denomination - fee` to `recipient` and `fee` to `relayer` from the vault.
- **Single-spend:** fold the standalone `mark_spent` INTO `withdraw` — the nullifier PDA (`["nullifier", pool, nullifier_hash]`) is `init`'d inside `withdraw` (atomic double-spend guard), gated behind proof verification. Remove the standalone `mark_spent` instruction.
- **Root check:** `withdraw` requires the proof's `root` public input to be a known root in the 100-entry ring (`roots::is_known`).
- **VK integrity:** the on-chain VK is generated from `circuits/build/verification_key.json` into `groth16-solana` format and embedded as a `const`; a `check-vk` step regenerates and byte-compares (drift guard). Dev VK is dev-only; production ceremony deferred (spec §5).
- **Compute budget:** on-chain Groth16 verification (a multi-pairing via `alt_bn128`) is CU-heavy; `withdraw` transactions prepend a `ComputeBudgetInstruction::set_compute_unit_limit(...)` — measure and set from the real cost (likely 400k–1.4M).
- Rust for on-chain/SDK/proving; circom+JS only for circuits. Every task green (`cargo test`, `npm test`) and committed.

---

### Task 1: Extend withdraw circuit with `extDataHash` + regenerate VK

**Files:**
- Modify: `circuits/circom/withdraw.circom`
- Modify: `crates/parity-fixtures/src/main.rs` (note-bundle emits `extDataHash` + example recipient/relayer/fee)
- Modify: `circuits/test/withdraw.test.js`, `circuits/test/withdraw_vectors.json`, `circuits/test/input.json`
- Modify: `crates/prover/src/lib.rs` (+ `tests/prove_verify.rs`) — `prove_withdraw` takes `extDataHash`; public inputs become 3

**Interfaces:**
- Produces: withdraw circuit with public `[root, nullifierHash, extDataHash]`; a regenerated VK/zkey (deterministic); `prover::prove_withdraw` accepting `ext_data_hash: [u8;32]`.

- [ ] **Step 1: Add the bound public input to the circuit (write failing test)**

`circuits/circom/withdraw.circom` — add `extDataHash` as a public input with a Tornado-style dummy constraint so the compiler keeps the signal:
```circom
template Withdraw(depth) {
    signal input root;           // public
    signal input nullifierHash;  // public
    signal input extDataHash;    // public — binds recipient/relayer/fee (hash computed off-circuit)
    signal input nullifier;      // private
    signal input secret;         // private
    signal input pathElements[depth];
    signal input pathIndices[depth];

    // Bind extDataHash into the proof without constraining its value (Tornado pattern):
    // a nonzero-degree constraint forces the compiler to keep the signal, so the proof
    // is bound to this exact public input; any change invalidates verification.
    signal extDataHashSq;
    extDataHashSq <== extDataHash * extDataHash;

    component cm = Poseidon(2); cm.inputs[0] <== nullifier; cm.inputs[1] <== secret;
    component nh = Poseidon(1); nh.inputs[0] <== nullifier; nh.out === nullifierHash;
    component mp = MerkleProof(depth); mp.leaf <== cm.out;
    for (var i = 0; i < depth; i++) { mp.pathElements[i] <== pathElements[i]; mp.pathIndices[i] <== pathIndices[i]; }
    mp.root === root;
}
component main {public [root, nullifierHash, extDataHash]} = Withdraw(20);
```

- [ ] **Step 2: Fixtures emit `extDataHash`**

Extend the `note-bundle` subcommand (`crates/parity-fixtures/src/main.rs`) to also compute+emit example ext-data + its hash. Add a canonical helper (also used by the SDK later):
```rust
use anchor_lang::solana_program::keccak; // or solana_program::keccak
// ... but parity-fixtures shouldn't pull anchor; use `sha3`/`tiny-keccak` or solana-program's keccak.
fn ext_data_hash(recipient: &[u8; 32], relayer: &[u8; 32], fee: u64) -> [u8; 32] {
    let mut buf = Vec::with_capacity(72);
    buf.extend_from_slice(recipient);
    buf.extend_from_slice(relayer);
    buf.extend_from_slice(&fee.to_le_bytes());
    let h = /* keccak256(buf) */;               // 32-byte big-endian digest
    reduce_mod_bn254_be(h)                       // interpret BE, reduce mod r, return 32-byte BE
}
```
> **VERIFY AT IMPLEMENTATION TIME:** pick ONE keccak impl and reduction that the on-chain instruction (Task 3, `solana_program::keccak::hashv`) and the SDK will match EXACTLY (same byte concatenation order, same LE `fee`, same BE-interpret + `mod r`). This cross-boundary agreement is the security property — add a fixture that pins it.
Emit `extDataHash`, `recipient`, `relayer`, `fee` into `withdraw_vectors.json`.

- [ ] **Step 3: Update prover + tests for 3 public inputs**

`crates/prover/src/lib.rs`: `prove_withdraw` gains `ext_data_hash: [u8;32]`; push it as the `extDataHash` circuit input; public inputs returned as `[root, nullifierHash, extDataHash]` (BE). `tests/prove_verify.rs`: feed `extDataHash` from the bundle; the ark-groth16 AND groth16-solana verifies now check 3 public inputs; the tamper test flips a bit of `extDataHash` (as well as the existing `nullifierHash` case) → rejected.

- [ ] **Step 4: Regenerate VK + run all tests**

Run:
```bash
cd circuits && bash scripts/setup.sh   # deterministic VK regen for the new circuit
npm test                                # poseidon+merkle+withdraw (now with extDataHash) green
cd .. && cargo test -p prover           # ark-groth16 + groth16-solana verify + tamper reject (3 public inputs)
```
Expected: all green; regenerated VK/zkey deterministic (re-run twice → byte-identical).

- [ ] **Step 5: Commit**

```bash
git add circuits/ crates/parity-fixtures/src/main.rs crates/prover/ Cargo.lock
git commit -m "feat(circuits,prover): bind extDataHash (recipient/relayer/fee) in withdraw + VK regen"
```

---

### Task 2: On-chain Groth16 verifier module (embed VK)

**Files:**
- Create: `programs/pool-program/src/verifier.rs`
- Create: `programs/pool-program/src/vk.rs` (generated `groth16-solana`-format VK constant)
- Create: `xtask/` (a small Rust bin) OR `crates/vk-gen/` — generates `vk.rs` from `verification_key.json`
- Modify: `programs/pool-program/Cargo.toml` (add `groth16-solana = "0.2"`), `lib.rs` (`pub mod verifier; pub mod vk;`)

**Interfaces:**
- Produces: `verifier::verify_withdraw(proof_bytes: &WithdrawProof, public_inputs: &[[u8;32]; 3]) -> Result<()>` returning a program error on invalid proof; the embedded `vk::WITHDRAW_VK` constant.

- [ ] **Step 1: VK generator (from verification_key.json → groth16-solana format)**

A small Rust tool (`crates/vk-gen`, workspace member, NOT the on-chain program) that reads `circuits/build/verification_key.json`, converts each VK element to the `groth16-solana` byte layout (BE G1/G2 with the G2 `Fq2` swap — reuse `crates/prover`'s conversion helpers; make them `pub`), and emits `programs/pool-program/src/vk.rs`:
```rust
// @generated by crates/vk-gen from circuits/build/verification_key.json — do not edit.
pub const WITHDRAW_VK: groth16_solana::groth16::Groth16Verifyingkey = groth16_solana::groth16::Groth16Verifyingkey {
    nr_pubinputs: 3,
    vk_alpha_g1: [ /* 64 bytes */ ],
    vk_beta_g2:  [ /* 128 bytes */ ],
    vk_gamme_g2: [ /* 128 bytes — note upstream's typo'd field name */ ],
    vk_delta_g2: [ /* 128 bytes */ ],
    vk_ic: &[ /* nr_pubinputs+1 G1 points */ ],
};
```
> **VERIFY AT IMPLEMENTATION TIME:** the exact `Groth16Verifyingkey` struct shape + field names in `groth16-solana 0.2` (the `vk_gamme_g2` typo is real upstream — the prover's Task-5 tests already reference it). `vk_ic` must be `'static`.

- [ ] **Step 2: verifier module (write failing test)**

`programs/pool-program/src/verifier.rs`:
```rust
use anchor_lang::prelude::*;
use groth16_solana::groth16::Groth16Verifier;
use crate::vk::WITHDRAW_VK;

pub struct WithdrawProof { pub a: [u8; 64], pub b: [u8; 128], pub c: [u8; 64] }

/// Verify the withdraw proof over [root, nullifierHash, extDataHash] (each 32-byte BE).
pub fn verify_withdraw(p: &WithdrawProof, public_inputs: &[[u8; 32]; 3]) -> Result<()> {
    let mut v = Groth16Verifier::new(&p.a, &p.b, &p.c, public_inputs, &WITHDRAW_VK)
        .map_err(|_| error!(crate::PoolError::ProofMalformed))?;
    v.verify().map_err(|_| error!(crate::PoolError::ProofInvalid))?;
    Ok(())
}
```
Add `PoolError::{ProofMalformed, ProofInvalid}`. A LiteSVM test in Task 3 exercises this end-to-end; a focused unit test here can feed a known-good proof (from `crates/prover`, via a committed test vector) and assert accept + a tampered one asserts reject.
> **VERIFY AT IMPLEMENTATION TIME:** `Groth16Verifier::new` signature (proof A/B/C sizes; whether `proof.A` must already be negated — Task 1's prover produces the negated BE form; keep it consistent), and the on-chain CU cost of `verify()`.

- [ ] **Step 3–5:** build, run the verifier test (accept real proof / reject tampered), commit `feat(pool-program): on-chain groth16 withdraw verifier + embedded VK`.

---

### Task 3: `withdraw` instruction + single-denomination pool

**Files:**
- Modify: `programs/pool-program/src/lib.rs` (add `withdraw`; remove standalone `mark_spent`; add `denomination` handling to `initialize_pool`/`deposit`)
- Modify: `programs/pool-program/src/state.rs` (`Pool` gains `denomination: u64`; re-check zero-copy padding + `SPACE`)
- Modify: `programs/pool-program/src/nullifier.rs` (unchanged record; now created inside `withdraw`)
- Create: `programs/pool-program/tests/withdraw.rs`

**Interfaces:**
- Produces: `initialize_pool(denomination: u64)`, `deposit` enforcing `amount == denomination`, `withdraw(proof, root, nullifier_hash, recipient, relayer, fee)`.

- [ ] **Step 1: Denomination in state + init + deposit**

`Pool` gains `pub denomination: u64` (re-run `offset_of!` checks; adjust `_reserved`/`SPACE` for the new field's alignment — it's `u64`, align 8, place carefully). `initialize_pool` takes + stores `denomination`. `deposit` adds `require!(amount == pool.load()?.denomination, PoolError::WrongDenomination)`.

- [ ] **Step 2: `withdraw` handler (write failing test)**

```rust
pub fn withdraw(
    ctx: Context<Withdraw>,
    proof: crate::verifier::WithdrawProof,
    root: [u8; 32],
    nullifier_hash: [u8; 32],
    recipient: Pubkey,
    relayer: Pubkey,
    fee: u64,
) -> Result<()> {
    // 1. root must be a known historical root
    {
        let pool = ctx.accounts.pool.load()?;
        require!(crate::roots::is_known(&pool.roots, &root), PoolError::UnknownRoot);
        require!(fee <= pool.denomination, PoolError::FeeExceedsDenomination);
    }
    // 2. recompute extDataHash and verify the proof binds it
    let ext = crate::verifier::compute_ext_data_hash(&recipient, &relayer, fee); // keccak+reduce, matches SDK/circuit
    crate::verifier::verify_withdraw(&proof, &[root, nullifier_hash, ext])?;
    // 3. single-spend: the `nullifier` PDA is init'd by the Accounts struct (fails if already spent)
    ctx.accounts.nullifier.spent = true;
    // 4. payout from the vault: (denomination - fee) -> recipient, fee -> relayer
    let denom = ctx.accounts.pool.load()?.denomination;
    let vault_seeds = /* ["vault", pool.key(), &[vault_bump]] */;
    // system_program::transfer with vault PDA signer (invoke_signed) for denom-fee to recipient, fee to relayer
    Ok(())
}
```
`Withdraw` accounts: `pool: AccountLoader<Pool>` (mut), `vault` (mut, PDA), `nullifier` (`init`, seeds `["nullifier", pool, nullifier_hash]`), `recipient` (mut, `SystemAccount`), `relayer` (mut, `SystemAccount`), `payer`/fee-payer (the relayer submits), `system_program`. The nullifier's `init` gives the atomic double-spend guard; a second withdraw with the same `nullifier_hash` fails on the existing PDA.
> **VERIFY AT IMPLEMENTATION TIME:** the vault is a system-owned PDA — to move lamports OUT you either use `invoke_signed(system_transfer, &[vault_seeds])` (vault must be a PDA the System program lets sign) or directly debit/credit `**lamports.borrow_mut()` on the account infos (simpler for program-owned accounts, but the vault is system-owned — use `invoke_signed` with the vault PDA seeds). Reconcile with how `deposit` funds it.

- [ ] **Step 3: Remove standalone `mark_spent`** (its guard now lives in `withdraw`).

- [ ] **Step 4: LiteSVM test — full happy path + guards**

`tests/withdraw.rs`: init a pool (denomination D), deposit a commitment (from a committed proof fixture whose note is known), then `withdraw` with a **real proof** (generated by `crates/prover` — either at build time or from a committed proof vector) and assert: recipient balance += D-fee, relayer += fee, vault -= D; a second identical withdraw fails (nullifier spent); a withdraw with an unknown `root` fails; a withdraw with a tampered `recipient` (extDataHash mismatch) fails. Prepend a `set_compute_unit_limit` and record the CU.
> This test depends on `circuits/build/*` + a proof for the deposited note — reuse the `crates/prover` path (serialize a proof to a committed test vector, or generate in a build step guarded like `crates/prover`'s `ensure_build_artifacts`).

- [ ] **Step 5: Commit** `feat(pool-program): withdraw (groth16 verify + root-check + single-spend + denominated payout)`.

---

### Task 4: `check-vk` drift guard (CI + xtask)

**Files:**
- Modify: `crates/vk-gen` (add a `--check` mode: regenerate in-memory, byte-compare to committed `vk.rs`, exit nonzero on drift)
- Modify: `.github/workflows/ci.yml` (a `check-vk` step: `bash circuits/scripts/setup.sh && cargo run -p vk-gen -- --check`)

- [ ] Implement `--check`, wire the CI step, verify it passes on the committed VK and fails if `vk.rs` is edited. Commit `ci(pool-program): check-vk guard against verifier-key drift`.

---

### Task 5: Minimal SDK (`crates/sdk`)

**Files:**
- Create: `crates/sdk/Cargo.toml`, `crates/sdk/src/lib.rs`, `crates/sdk/tests/sdk.rs`

**Interfaces:**
- Produces: `Note::new() -> Note` (random nullifier+secret), `note.commitment()`, `note.nullifier_hash()`; `build_deposit_ix(...)`; `build_withdraw_ix(note, merkle_path, recipient, relayer, fee, ...)` that generates the proof (via `crates/prover`) and assembles the instruction with the correct proof bytes + public inputs + accounts; `compute_ext_data_hash` (shared with the program via a common crate or duplicated-with-a-parity-test).

- [ ] Implement note/commitment/nullifier (matching the on-chain hashes — reuse `pool_program::poseidon`), proof generation (wrap `prover::prove_withdraw`), and instruction builders. A test builds a deposit + withdraw ix and asserts the encoded public inputs + extDataHash match what the program will recompute. Commit `feat(sdk): note management + deposit/withdraw instruction builders + proof gen`.

---

### Task 6: End-to-end integration test

**Files:**
- Create: `crates/sdk/tests/e2e.rs` (or a top-level `tests/`)

- [ ] LiteSVM e2e using the SDK: `initialize_pool(D)` → `build_deposit_ix` (SDK note) → advance/scan the leaf+path → `build_withdraw_ix` (real proof) → submit → assert recipient/relayer/vault balances, then assert double-spend + unknown-root + tampered-recipient all fail. This is the load-bearing proof that circuit ↔ prover ↔ on-chain verifier ↔ SDK all agree. Commit `test(e2e): deposit -> prove -> withdraw shielded-pool round trip`.

---

## What this plan delivers

A working, secure single-denomination shielded pool: deposit a note, generate a real Groth16 proof client-side (SDK), and withdraw to a recipient the proof cryptographically binds (front-run-safe via `extDataHash`), with on-chain proof verification, known-root enforcement, and atomic single-spend — proven end-to-end.

## Explicitly deferred to later plans

- **Behavioral rounds / `PooledAction` / `k`-floor / coordinator** — Plan 4 (the pool-as-uniform-actor model; this plan is a standalone shielded-pool withdraw).
- **Value-conservation / arbitrary amounts (2-in/2-out)** — a later circuit; this plan is single-denomination.
- **Multiple denomination buckets** — this plan is one denomination per pool.
- **Production trusted-setup ceremony** — hardening (spec §5).
- **Relayer/coordinator service + gasless UX** — the `withdraw` already supports a relayer paying gas + taking `fee`; the coordinator service is Plan 4.

## Open questions / risks

- **`extDataHash` cross-boundary agreement** (circuit input ↔ SDK ↔ on-chain keccak+reduce) is the security crux — pin it with a fixture test in Task 1.
- **On-chain Groth16 CU cost** — the `alt_bn128` multi-pairing may need a large `set_compute_unit_limit`; measure in Task 3.
- **`groth16-solana 0.2` on-chain API** (`Groth16Verifyingkey` shape, `Groth16Verifier::new` signature, proof.A negation expectation) — verify against its source at implementation time (Task 5 of Plan 2 already used its verifier off-chain, so the byte format is known-good).
- **Vault lamport-out mechanism** (system-owned PDA `invoke_signed` vs. direct lamport edit) — reconcile with `deposit`'s funding in Task 3.
- **zero-copy `Pool` layout change** — adding `denomination: u64` shifts offsets/padding; re-verify `offset_of!`-based tests.
