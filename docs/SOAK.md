# SOAK — live end-to-end verification

`crates/soak` (`cargo run -p soak`) drives one complete live protocol exercise against a real
`solana-test-validator` over real RPC — no LiteSVM anywhere in this path — and asserts every claim
below from chain reads (`getTransaction`, `getAccountInfo`, balances), never from the client's own
bookkeeping. It runs two rounds back to back: a withdraw round at `MAX_K_WITHDRAW = 17` and a
pooled-stake round at `MAX_K_STAKE = 10`. This is the verification tier LiteSVM cannot reach: real
banking-stage sanitization (the 64-account-lock wall is invisible in-VM), real Address Lookup
Table creation/activation timing, real signature verification, and transactions anyone can look up
by signature and re-derive every assertion from. Design: `docs/superpowers/specs/2026-07-20-soak-design.md`.

## 1. What this run proves

Every claim below maps 1:1 to a named assertion in `crates/soak/src/assertions.rs`, computed from
an RPC read and recorded pass/fail in `docs/soak-report.md`. A1/A2/A3/A5/A6/A7 run once per round
(withdraw and stake each get their own row); A4 is withdraw-only (the single-spend property is
protocol-generic, exercised live there); A8 is stake-only.

| Claim | Assertion(s) |
|---|---|
| Zero participant signatures — the cranker/operator is the sole signer on `execute_round`, no recipient/relayer/depositor signs, and every one of those keys still resolved into the transaction via the Address Lookup Table (present but unsigned, not just absent by irrelevance) | A1 |
| Value conservation — the vault's balance drops by exactly `k × denomination` across the execute, pinned to the pre/post-execute window | A2 |
| Byte-uniform settlement — every payout lands in one of a small number of visibly distinct, bucketed amount classes, never an arbitrary per-user value | A3 |
| Single-spend — every nullifier PDA exists and is pool-owned; a duplicate `commit_intent` for an already-spent note fails the send and leaves the existing PDAs byte-unchanged (probed live, while the round is still Open) | A4 (withdraw round) |
| Round lifecycle — the executed round is `Executed`; the next round exists, `Open`, with `intent_count = 0` | A5 |
| The live effective-k number — `crates/effective-k`'s `AnonymityReport`, computed from the run's true funding composition and printed verbatim (never a hand-written formula) | A6 |
| Envelope facts — the resolved account-key count (static + ALT-loaded) stays within the 64-account-lock ceiling; compute units consumed is recorded | A7 |
| Genuine stake delegation, authority handed over — each of the 10 stake accounts is Stake-program-owned, ends the transaction with `authorized.staker == authorized.withdrawer == recipient` (initialized vault-side, handed over via `Authorize(Staker)` in the same transaction — the FINAL state is asserted, never the `Initialize` step's parameters), and is delegated to the pool's own vote account | A8 (stake round) |

## 2. What this run does NOT prove

At equal prominence, because a soak that buries its own limitations is worse than useless:

- **Solo operator ⇒ effective-k collapses to 1.0.** One operator funds every note in both rounds —
  the maximal-whale case in `crates/effective-k`'s own semantics (`m` = the dominant funder's note
  count = `k`, so `effective_k = k/m = 1.0`, `max_funder_share = 1.0`). This is disclosed, not
  hidden: a real deployment's effective-k depends on independent funder clustering, which a
  single-operator soak structurally cannot exercise. See the embedded A6 rows below for the exact
  printed numbers from this run.
- **Local validator ⇒ not mainnet conditions.** `solana-test-validator` reproduces real
  banking-stage sanitization, ALT timing, and signature verification, but not mainnet's validator
  set, stake weight, fee market, or network latency.
- **A self-created vote account is a genuine stake-program delegation mechanic, not a mainnet
  validator relationship.** `DelegateStake` accepts it exactly as it would on a live cluster, but
  it names no real validator and carries no real commission/uptime history.
- **Delegation state, not activated stake.** A8 asserts the on-chain `StakeStateV2::Stake`
  delegation record; it does not await or assert activation (an epoch-boundary process) — the
  claim is deliberately narrower than "this stake is earning rewards."
- **The rent-exemption fee floor is a live-bank-discovered constraint, not a protocol one.** LiteSVM
  never enforces it (`crates/sdk/tests/e2e.rs` runs a `FEE = 1_000` withdraw fine), but a real bank
  requires every account it touches to end at or above rent-exemption — `execute_round` pays fees
  directly into fresh System accounts, so every fee in this run (`WITHDRAW_FEE = 1_000_000`,
  `SOAK_STAKE_FEE = 1_000_000`) is sized to clear that floor with margin on both the recipient and
  relayer legs, not chosen for any protocol-required ratio.

## 3. Reproduce it

```bash
# 1. Build the on-chain program and generate circuit artifacts (once)
anchor build
bash circuits/scripts/setup.sh   # only if circuits/build/* is missing

# 2. Launch a fresh local validator with the program deployed
solana-test-validator --reset --bpf-program 7oHnDkpPbhPacDfqzF38caM3eo1Xo7cBmFugNXJurnn3 \
  target/deploy/pool_program.so

# 3. In another shell, run the soak (both rounds, ~10-15 minutes, dominated by proving)
cargo run -p soak
```

Compare the freshly written `docs/soak-report.md` against the copy embedded below: the assertion
IDs, descriptions, and structural shape (phase list, tx table, notes) should match; exact
signatures, timings, and account addresses will differ every run (fresh keypairs, fresh blockhash).
A clean run ends with `RUN PASSED` and every assertion `PASS`.

## 4. The captured report

The following is `docs/soak-report.md` as committed alongside this document, produced by a single
real run against a freshly reset `solana-test-validator`.

```markdown
{{EMBEDDED_REPORT}}
```
