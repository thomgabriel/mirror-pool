# Structure-polish — design brief (for the building lane)

> **What this is:** reviewer-lane input for a **behavior-identical** hygiene pass — file splits and a
> rename cascade, zero logic change. The building lane runs its own `brainstorm → spec → plan → TDD`;
> the fork reviews the spec, the plan, and the merged branch. Own feature branch off `main`
> (`feat/structure-polish`). Sequence **after F1** (merged `e9f9c68`) and land the circom rename
> **before the soak is built** (the soak uses the renamed prover/SDK symbols).

## The honest framing (do not gold-plate)

The `code-craft-and-repo-hygiene.md` checklist files the `lib.rs`/SDK split as a **watch-item,
explicitly YAGNI, "not a violation"** — each handler already delegates its logic to
`action.rs`/`invariants.rs`/`merkle.rs`. We're doing this for **reviewability / presentation** on a
code-reviewed bounty, not because it's required. So keep it tight: pure moves and renames, no new
abstractions, no "while I'm here" changes.

## The overriding guard — BEHAVIOR-IDENTICAL

- **Zero logic change.** Every diff is a move or a rename. If a reviewer sees a changed condition, a
  reordered check, or a new/removed line of logic, that's a defect.
- **Full suite green at *every* commit.** Each of the five pieces below is its own commit (or task)
  and lands only with `cargo test --workspace` + `cargo fmt --check` + `cargo clippy --all-targets --
  -D warnings` all green. No "fix it in the next commit."
- The review's job is to confirm the *absence* of a behavior delta — which is only tractable if each
  commit is small and single-purpose. Do not combine pieces.

## The five pieces — sequenced low-risk → high-risk

### 1. Punch-list fixes (trivial warm-up)
Fix the Part-2 nitpicks from `code-craft-and-repo-hygiene.md`, notably **#1**: the doc comment at
`crates/sdk/src/lib.rs` on `stake_account_pda` cites `round.rs::execute_round`, but `execute_round`
lives in `lib.rs` (`round.rs` holds only `RoundState`/`Round`/`ActionKind`/`Intent`). Correct the
cross-reference. Sweep for any other stale `lib.rs:NNN` / wrong-file doc comments.

### 2. `PoolError → errors.rs` (mechanical)
Extract the `#[error_code] enum PoolError` from `lib.rs` into `errors.rs`, `pub use` it back.
- **CRITICAL — append-only ABI:** the variant *order* is the error-code ABI (6000–6024). Moving the
  enum must preserve the exact order byte-for-byte; **reordering shifts every downstream code and
  breaks tests + any client.** Guard: the tests that assert specific variants/codes
  (`RoundFull`=6023, `KFloorTooHigh`=6024, `FeeNotUniform`=6022, …) stay green unchanged.

### 3. `lib.rs → instructions/` (the Squads-v4 split)
One file per instruction — `instructions/{initialize_pool,deposit,commit_intent,cancel_intent,execute_round}.rs`
— each holding **its handler + its `#[derive(Accounts)]` context struct together**; `instructions/mod.rs`
re-exports. `lib.rs` becomes `declare_id!` + the `#[program]` mod (each fn delegating to
`instructions::foo::handler(...)`) + module wiring. `DepositEvent` → `events.rs` or its instruction file.
- **Guard:** the per-instruction test binaries (`initialize_pool`, `deposit`, `commit_intent`,
  `cancel_intent`, `execute_round`, `max_k`, `stake_round`) each exercise a handler end-to-end — they
  must pass **unchanged**. The `program_id` (`declare_id!`) must not move.
- Keep the existing `'info`-lifetime and no-`round.state`-check comments on `execute_round` verbatim —
  they're load-bearing rationale, not boilerplate.

### 4. SDK `note`/`tree`/`ix` split
Split `crates/sdk/src/lib.rs` (725 lines: `Note`, `MerkleTree`, the instruction builders) into modules
(`note.rs`, `tree.rs`, `ix.rs` or similar), **`pub use`-d from `lib.rs` so every public path is
unchanged** (`sdk::Note`, `sdk::MerkleTree`, `sdk::build_execute_round_ix`, … resolve exactly as
before). Guard: `e2e.rs` + `sdk.rs` + the soak (later) see an identical public API; tests pass unchanged.

### 5. circom rename cascade — **withdraw → membership** (RISKIEST; own task; last)
The membership circuit is misnamed `withdraw.circom` (legacy — it proves note membership for *every*
action). Rename the file to `membership.circom` and cascade the **circuit/proof** symbols
(~62 code refs + ~87 artifact-filename refs across `circuits/`, `crates/prover`, `crates/vk-gen`,
`crates/sdk`, `programs/pool-program/{verifier,vk,lib}.rs`, and the tests):
`WithdrawProof`, `verify_withdraw`, `WITHDRAW_VK`, `prove_withdraw`, `WithdrawInputs`,
`WithdrawArtifacts`, `withdraw.circom`/`.r1cs`/`.zkey`/`withdraw_js`/`withdraw.wasm` → `Membership*` /
`membership.*`.

- **CRITICAL NUANCE — rename by SYMBOL, never `sed s/withdraw/membership/g`.** The **Withdraw
  *action*** surface must stay: `WithdrawAction`, `MAX_K_WITHDRAW`, `ActionKind::Withdraw`,
  `build_execute_round_ix` (the withdraw-*round* executor). Those are the withdraw action (one of two),
  not the shared membership circuit. A blind text substitution corrupts them — rename each symbol
  deliberately.
- **CRYPTO GUARD (this is a rename, not a circuit change):** the circuit's *constraints* are untouched,
  so re-running `circuits/scripts/setup.sh` + `crates/vk-gen` must regenerate a **byte-identical VK**
  (`vk.rs`). Verify: the Poseidon/Merkle parity tests, the prover's `prove_verify` test, and the SDK
  `e2e` real-proof test all still pass on-chain. **If the VK bytes change, STOP** — the rename leaked
  into the circuit content.
- **Why last & why before the soak:** it's the highest-regression piece, and it's the one that couples
  to the soak (the soak calls `prove_*` and the proof types). Land it before the soak is built so the
  soak is written once against the final names.

## Sequencing & review

- Order **1 → 2 → 3 → 4 → 5**, each its own commit, each green. Do the circom rename (5) before the
  soak brainstorm resumes.
- **Fork review focus** (behavior-identical, so the bar is *no behavior delta*): the diff is pure
  moves/renames; tests green and *unchanged* (a changed test assertion is a red flag — the behavior it
  asserts shouldn't have moved); error-ABI order preserved (#2); VK bytes identical + proofs verify
  (#5); public API paths unchanged via re-exports (#3, #4).
- Not pushed without the user's explicit yes.
