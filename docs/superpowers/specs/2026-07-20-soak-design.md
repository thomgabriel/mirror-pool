# SOAK + proof doc — live end-to-end verification (F2a)

**Date:** 2026-07-20 · **Status:** approved design, pending fork spec-review
**Grounding:** `docs/superpowers/plans/2026-07-18-finish-roadmap.md` (F2a entry + guard),
`crates/sdk/tests/e2e.rs` (the LiteSVM blueprint), the merged MAX_K work (measured envelope
17/10, v0+ALT cranker path), the merged membership rename (final symbol names).
**Branch:** `feat/soak` off `main` (`9fae169`).

## 1. What this is — and the verification tier it adds

A workspace binary (`crates/soak`, `cargo run -p soak`) that drives one complete live protocol
exercise against a local `solana-test-validator` over real RPC, asserts every claim from chain
reads, and emits the evidence report that `docs/SOAK.md` embeds. This is the tier LiteSVM cannot
reach: real banking-stage sanitization (the 64-account-lock wall is invisible in-VM — measured
during F1), real ALT creation/activation timing, real signature verification, and transactions a
judge can look up by signature and re-derive every assertion from.

**Decisions locked (user-approved 2026-07-20):** full-envelope round sizes — withdraw at
`MAX_K_WITHDRAW = 17`, stake at `MAX_K_STAKE = 10`; the binary emits the structured report and
`SOAK.md` is the hand-written frame embedding a captured copy. Runtime `solana-test-validator`
(decided earlier; universally reproducible, no third-party fork tooling).

## 2. Run shape (one invocation, sequential phases)

1. **Preflight (fail with actionable messages, not panics):** RPC endpoint reachable (default
   `http://127.0.0.1:8899`, overridable via `--url`); `pool_program` account exists and is
   executable at the canonical `declare_id!` (launch is documented, not automated:
   `solana-test-validator --reset --bpf-program 7oHnDkpPbhPacDfqzF38caM3eo1Xo7cBmFugNXJurnn3
   target/deploy/pool_program.so`); circuit artifacts present (`circuits/build/membership.wasm`
   `.r1cs` `.zkey` — pointing at `circuits/scripts/setup.sh` if absent); a funded operator
   keypair (airdropped from the validator faucet).
2. **Setup:** create a real, delegable vote account via RPC (the RPC analogue of the test
   fixtures' `create_validator_vote_account` — real `CreateAccount` + vote-program
   `InitializeAccount`); `initialize_pool` twice: a withdraw pool (`k_floor = 2`, uniform `fee`)
   and a stake pool (denomination sized per `stake_split` to clear fee + rent + 1 SOL minimum
   delegation).
3. **Withdraw round, k = 17:** 17 deposits (fresh `Note`s) → 17 client-side `prove_membership`
   proofs (ark-circom, pure Rust — the no-snarkjs differentiator exercised live) → 17
   `commit_intent`s → create + extend an on-chain Address Lookup Table with the per-intent keys,
   wait for activation (≥1 slot) → **one v0+ALT `execute_round` transaction carrying
   `SetComputeUnitLimit`** — exactly the cranker path the MAX_K spec requires, now proven against
   real banking-stage limits at the enforced envelope.
4. **Stake round, k = 10:** same flow through the stake arm (`build_execute_stake_round_ix` +
   the stake tail); asserts each of the 10 stake accounts is Stake-program-owned, initialized
   with `staker = recipient` and `withdrawer = recipient`, and **delegated** to the run's vote
   account. Delegation *activation* (an epoch process) is deliberately not awaited — the honest
   claim is the delegation state, and the report says so.
5. **Assertions — every one a chain read; none trusts the client code** (§3).
6. **Report:** structured markdown written to `docs/soak-report.md` (§4).

Failure behavior: any failed assertion or phase aborts the run with a non-zero exit and the
failure in the report draft — the soak never emits a "partial pass" report that could be mistaken
for a full one.

## 3. The assertion set (the claims, and only the claims)

Per executed round, all derived from RPC reads (`getTransaction` with full meta,
`getAccountInfo`, balances):

- **A1 — the headline, zero participant signatures:** fetch the landed `execute_round`
  transaction by signature and read its **actual signer set** from the message header. Assert it
  contains exactly the cranker (fee payer) and NO recipient, relayer, or depositor key — the
  uniform-actor property read from the wire, not from our code.
- **A2 — value conservation:** vault balance delta across the round == `k × denomination`
  (withdraw: paid out to recipients+relayers; stake: moved into stake accounts + fees).
- **A3 — byte-uniform settlement:** withdraw — every recipient credited exactly
  `denomination − fee` and every relayer exactly `fee` (k identical pairs); stake — every stake
  account funded/delegated to the identical amount (`stake_split` values).
- **A4 — single-spend:** all k nullifier PDAs exist with `spent = true`; a duplicate
  `commit_intent` for one already-spent note is submitted and **asserted to fail** (the one
  negative probe in the run).
- **A5 — round lifecycle:** executed round PDA is `Executed`; the next round PDA exists and is
  `Open` with `intent_count = 0`.
- **A6 — the live effective-k report:** feed the run's true funding composition into
  `crates/effective-k` and print `AnonymityReport` verbatim. A single-operator soak is the
  disclosed whale case: `m = 1`, so `effective_k = k/m … = k/1` nominal but the guessing
  advantage line tells the truth. This number is REPORTED, never gated on — and the report text
  states plainly that a solo run demonstrates the mechanism, not a real anonymity set.
- **A7 — envelope facts:** the execute transaction's resolved account-key count (≤ 64) and
  consumed CU, read from the transaction meta — the live counterparts of F1's measured numbers.

## 4. The report and the proof doc

**`docs/soak-report.md` (binary-emitted, overwritten per run):** header (date, toolchain +
program-id + commit hash, validator version); per-phase timing; per-round: every tx signature
(deposits, commits, ALT create/extend, execute), the A1 signer-set listing, A2/A3 balance tables,
A4–A7 results; final PASS/FAIL per assertion. Format is stable and diffable across runs.

**`docs/SOAK.md` (hand-written frame, written once at build time; commit includes a real
captured report):**
1. *What this run proves* — each claim mapped 1:1 to the assertion (A1–A7) that checks it.
2. *What it does NOT prove* — same prominence, no burying: single operator ⇒ no real anonymity
   set (the effective-k line shows it); local validator ⇒ not mainnet conditions; self-created
   vote account ⇒ a genuine stake-program delegation, **not** a mainnet validator relationship;
   delegation state, not activated stake.
3. *Reproduce it* — exact commands (validator launch, artifact build, `cargo run -p soak`) and
   what to compare.
4. The embedded captured report.

Built from first principles for this protocol; explicitly NOT templated on any competitor's
proof document.

## 5. Honesty ledger

- Every SOAK.md claim maps to a named assertion computed from chain reads; anything not
  chain-checkable is not claimed.
- The whale-case effective-k of a solo run is disclosed in both the report and the doc — the
  soak proves the *mechanism* (uniform actor, value conservation, envelope); the §6.5
  adversarial simulation (F2b, separate work) is what probes the anonymity math.
- No "mainnet" language anywhere; no activation claims for stake; the report carries raw
  signatures so every claim is independently re-derivable.

## 6. Non-goals

No coordinator service (the soak binary IS the cranker for this run). No surfpool/mainnet-fork.
No multi-machine or long-duration soak (one deterministic pass; "soak" is the project's
historical name for the live e2e tier). No CI wiring (a local validator in CI is a separate
decision — the doc records how to run it manually). No new on-chain code, no SDK signature
changes (additions to the SDK are allowed only if the RPC path genuinely needs a helper that the
LiteSVM path didn't — and then as pure additions).

## 7. Build notes for the plan

- Reuse: `sdk::{Note, MerkleTree, build_*_ix, stake_account_pda, round_pda,
  compute_ext_data_hash, MembershipInputs}`, `prover::prove_membership`, `crates/effective-k`.
  The LiteSVM e2e (`crates/sdk/tests/e2e.rs`) is the sequence blueprint; every send becomes an
  RPC send-and-confirm.
- New dependency surface: `solana-client` (RPC) + `solana-address-lookup-table-*` for the ALT
  instructions — client-side only, in `crates/soak`; run `cargo deny check` before commit (new
  transitive trees are likely; justify or resolve any new advisory).
- Proof generation is ~27 × ~10 s single-threaded — parallelize with the same pattern the test
  fixtures use or accept ~5 min; report phase timings either way.
- Likely 3 plan tasks: (1) crate + preflight + setup + report skeleton; (2) withdraw round +
  A1–A7 assertions; (3) stake round + SOAK.md + captured report. The soak has no test suite of
  its own beyond `cargo build`/clippy cleanliness and smoke-running phases 1–2 against a live
  validator during development — it IS the test; the review bar is assertion-honesty (every doc
  claim ↔ a chain-read assertion in code).

## 8. Process

`feat/soak` → internal opus spec review → fork spec gate → plan (`writing-plans`) → fork plan
gate → SDD build → opus whole-branch review (bar: assertion-honesty + the claims/doc mapping) →
fork merge gate → local merge. No push without the user's explicit yes.
