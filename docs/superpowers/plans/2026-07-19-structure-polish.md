# Structure-Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Behavior-identical hygiene pass — five pieces (doc-comment sweep, `errors.rs`,
`instructions/` split, SDK split, withdraw→membership rename cascade), five commits, zero logic
change.

**Architecture:** Pure moves and renames per the fork-approved spec
`docs/superpowers/specs/2026-07-18-structure-polish-design.md` (read it once — its §2 gate and
piece guards bind every task). No new abstractions.

**Tech Stack:** Rust 2021 / Anchor 0.31.1, LiteSVM, circom 2.1.6 + snarkjs (piece 5 only).

## Global Constraints

- **BEHAVIOR-IDENTICAL:** every hunk is a move or a rename. A changed condition/reordered
  check/added-removed logic line is a defect. Test assertions never change (sole sanctioned
  addition: Task 1's error-ABI pin test).
- **Gate at every commit, in this order:** `anchor build` (MANDATORY FIRST on any piece touching
  `programs/pool-program/` — LiteSVM loads the prebuilt `.so`; testing without rebuilding
  false-greens) `&& cargo test --workspace` green + `cargo fmt --check` + `cargo clippy
  --all-targets -- -D warnings`. Test runs are slow (proof-cache regeneration, ~7 min per
  proof-heavy binary): run in the FOREGROUND with timeout 600000ms; never background/park.
- **Frozen surfaces:** error-code ABI order (25 variants, 6000–6024); every `#[program]` fn name
  and `#[derive(Accounts)]`/`#[account]`/`#[event]` struct name; `declare_id!`; every `sdk::*`
  public path; VK bytes; `circuits/test/withdraw_vectors.json` (deliberately NOT renamed — shared
  parity fixture; the residual name is intentional, spec piece 5).
- Five commits exactly, conventional messages as given per task. One piece per commit.
- Branch `feat/structure-polish` (already checked out). Never touch `main`.

---

### Task 1: Piece 1 (stale-comment sweep + vk-gen expect) and Piece 2 (`errors.rs`)

**Files:**
- Modify: `programs/pool-program/src/invariants.rs` (comment only), 
  `programs/pool-program/tests/round_support.rs` (comments only),
  `crates/vk-gen/src/main.rs` (unwrap→expect only)
- Create: `programs/pool-program/src/errors.rs`
- Modify: `programs/pool-program/src/lib.rs` (remove enum, add mod+re-export),
  `programs/pool-program/tests/deposit.rs` (one comment repoint — sanctioned carve-out)

**Interfaces:** Produces `programs/pool-program/src/errors.rs` with `pub enum PoolError`
re-exported as `crate::PoolError`/`pool_program::PoolError` — Tasks 2–4 and all existing code
depend on those paths resolving unchanged.

- [ ] **Step 1 (piece 1): fix the known stale plan-relative comments**

`programs/pool-program/src/invariants.rs` (~line 20-24): the `STAKE_ACCOUNT_SIZE` doc comment ends
"…host-testable; Task 2 adds a compile-time `assert!(STAKE_ACCOUNT_SIZE == StakeStateV2::size_of())`
in `action.rs` (where the stake crate is imported) so the two can never drift." That assert exists
now. Replace the "Task 2 adds…" clause with: "a compile-time
`assert!(STAKE_ACCOUNT_SIZE == StakeStateV2::size_of())` in `action.rs` (where the stake crate is
imported) keeps the two from drifting."

`programs/pool-program/tests/round_support.rs`: two comments read "(roots agree by the
MerkleTree<->pool_program parity proven in Task 1)". Replace "proven in Task 1" with "proven by the
sdk parity tests" in both.

Then sweep for anything else stale:
Run: `grep -rn "Task [0-9]\|Plan [0-9]" programs/pool-program/src crates/*/src | grep -v test`
and `grep -rn "lib\.rs:[0-9]\|round\.rs::" programs crates --include="*.rs"`
Fix any hit that references plan history or a wrong file/line the same way (comment text only);
list every hit + disposition in your report. Punch-list #1 (`stake_account_pda`) is ALREADY FIXED
at HEAD — verify (`grep -n "round.rs" crates/sdk/src/lib.rs` returns nothing) and record that.

- [ ] **Step 2 (piece 1): vk-gen unwrap→expect**

`crates/vk-gen/src/main.rs` — the three bare `.unwrap()`s are at **line 200 and lines 204-205**
(fork plan-gate correction: the earlier ~30-32/41-42 anchors already use `.expect`): they are
arkworks affine-point coordinate accesses `p.x().unwrap()` / `p.y().unwrap()` inside the
JSON-roundtrip helper (`x()`/`y()` return `Option`; `None` = point at infinity). Change each to
`.expect("affine point is not at infinity")`-style messages matching the file's existing
`.expect()` voice. No control-flow change — same panic, now with a message. Touch nothing that
already uses `.expect`.

- [ ] **Step 3: gate + commit piece 1**

Run: `anchor build && cargo test --workspace 2>&1 | tail -5 && cargo fmt --check && cargo clippy --all-targets -- -D warnings 2>&1 | tail -2`
Expected: all green, zero test-count change.

```bash
git add -A && git commit -m "docs(code): sweep stale plan-relative comments + vk-gen expect messages (punch-list #1/#4)"
```

- [ ] **Step 4 (piece 2): move `PoolError` to `errors.rs` with the structural guard**

FIRST capture the pre-move enum body for the byte-diff:
```bash
sed -n '/#\[error_code\]/,/^}/p' programs/pool-program/src/lib.rs > /tmp/poolerror_before.txt
```
Create `programs/pool-program/src/errors.rs`:
```rust
use anchor_lang::prelude::*;

<PASTE the captured block VERBATIM: the `#[error_code]` attribute + `pub enum PoolError { … }`,
 all 25 variants MerkleInit … KFloorTooHigh in exactly the captured order>
```
In `lib.rs`: delete the enum block; add `pub mod errors;` beside the other `pub mod` lines and
`pub use errors::PoolError;` after them.

Structural guard (falsifiable, total):
```bash
sed -n '/#\[error_code\]/,/^}/p' programs/pool-program/src/errors.rs > /tmp/poolerror_after.txt
diff /tmp/poolerror_before.txt /tmp/poolerror_after.txt && echo ABI-BODY-IDENTICAL
```
Expected: no diff output, `ABI-BODY-IDENTICAL`. Any delta = stop and fix; do not proceed.

- [ ] **Step 5 (piece 2): the permanent ABI pin test (write failing first is N/A — it must pass
immediately; its value is failing on any FUTURE reorder)**

Append to `errors.rs`:
```rust
#[cfg(test)]
mod abi_tests {
    use super::*;

    /// The variant order IS the error-code ABI (Anchor code = 6000 + discriminant).
    /// Name-based log assertions travel with the variant and cannot catch a reorder;
    /// this pin can. Append-only: new variants extend this list, never reorder it.
    /// (`Variant as u32` is always valid on a fieldless enum — no derive assumptions.)
    #[test]
    fn error_code_abi_is_pinned() {
        assert_eq!(PoolError::MerkleInit as u32, 0);
        assert_eq!(PoolError::ZeroDeposit as u32, 1);
        assert_eq!(PoolError::CommitmentNotInField as u32, 2);
        assert_eq!(PoolError::TreeFull as u32, 3);
        assert_eq!(PoolError::ProofMalformed as u32, 4);
        assert_eq!(PoolError::ProofInvalid as u32, 5);
        assert_eq!(PoolError::WrongDenomination as u32, 6);
        assert_eq!(PoolError::UnknownRoot as u32, 7);
        assert_eq!(PoolError::FeeExceedsDenomination as u32, 8);
        assert_eq!(PoolError::KFloorTooLow as u32, 9);
        assert_eq!(PoolError::WrongRound as u32, 10);
        assert_eq!(PoolError::RoundClosed as u32, 11);
        assert_eq!(PoolError::RoundOverflow as u32, 12);
        assert_eq!(PoolError::KFloorNotMet as u32, 13);
        assert_eq!(PoolError::IntentAccountsMismatch as u32, 14);
        assert_eq!(PoolError::IntentInvalid as u32, 15);
        assert_eq!(PoolError::IntentAccountMismatch as u32, 16);
        assert_eq!(PoolError::DuplicateIntent as u32, 17);
        assert_eq!(PoolError::WrongActionConfig as u32, 18);
        assert_eq!(PoolError::StakeDenominationTooLow as u32, 19);
        assert_eq!(PoolError::StakeAccountInvalid as u32, 20);
        assert_eq!(PoolError::CancelTooEarly as u32, 21);
        assert_eq!(PoolError::FeeNotUniform as u32, 22);
        assert_eq!(PoolError::RoundFull as u32, 23);
        assert_eq!(PoolError::KFloorTooHigh as u32, 24);
    }
}
```
(If `#[error_code]`'s expansion somehow rejects `as u32`, fall back to Anchor's generated
conversion — the requirement is a numeric pin per variant in declaration order; show the working
form in your report.)

`programs/pool-program/tests/deposit.rs` ~line 22-23: repoint the comment's
"(see `programs/pool-program/src/lib.rs`)" to "(see `programs/pool-program/src/errors.rs`)"
(sanctioned carve-out; codes 6001/6002 asserted there stay).

- [ ] **Step 6: gate + commit piece 2**

Run: `anchor build && cargo test --workspace 2>&1 | tail -5 && cargo test -p pool-program --lib abi 2>&1 | tail -3 && cargo fmt --check && cargo clippy --all-targets -- -D warnings 2>&1 | tail -2`
Expected: all green; workspace count = previous + 1 (the pin test); the name-based error tests
unchanged and green.

```bash
git add -A && git commit -m "refactor(pool-program): PoolError -> errors.rs (verbatim move) + numeric error-code ABI pin test"
```

---

### Task 2: Piece 3 — `lib.rs` → `instructions/` (Squads-v4 split)

**Files:**
- Create: `programs/pool-program/src/instructions/mod.rs`,
  `instructions/{initialize_pool,deposit,commit_intent,cancel_intent,execute_round}.rs`
- Modify: `programs/pool-program/src/lib.rs` (shrinks to declare_id + wiring + delegating
  `#[program]` mod)

**Interfaces:** Consumes Task 1's `crate::PoolError` re-export. Produces
`instructions::<name>::handler(...)` fns + the five `Accounts` structs re-exported at crate root
(struct names unchanged). Task 4 renames symbols inside these files later.

- [ ] **Step 1: create the five instruction files (verbatim body moves)**

Anchor points in the current `lib.rs` (post-Task-1; ranges shift slightly — anchor on the fn/struct
names, not raw numbers): handlers `initialize_pool` / `deposit` / `commit_intent` /
`cancel_intent` / `execute_round<'info>` in the `#[program]` mod; `Accounts` structs
`InitializePool` / `Deposit` / `CommitIntent` / `CancelIntent` / `ExecuteRound` below it;
`DepositEvent` after them.

For each instruction `<name>`, create `instructions/<name>.rs` containing, in order:
1. the `use` lines it needs (start from lib.rs's header: `anchor_lang::prelude::*`,
   `anchor_lang::system_program`, plus `crate::state::Pool` / `crate::PoolError` / merkle/roots/
   verifier/invariants/action paths as each body requires — compile-driven; add only what each
   file uses, no blanket globs),
2. its handler, **body verbatim**, renamed `pub fn handler(…)` with the SAME parameter list and
   return type. `execute_round`'s handler keeps the full lifetime signature verbatim:
   `pub fn handler<'info>(ctx: Context<'_, '_, 'info, 'info, ExecuteRound<'info>>, round_id: u64) -> Result<()>`
   — and its two load-bearing comment blocks (the named-`'info` invariance note above the fn and
   the no-`round.state`-recheck walk-through inside) move verbatim with it,
3. its `#[derive(Accounts)]` struct, **verbatim**, same name (plus its `#[instruction(...)]`
   attribute where present).

`DepositEvent` (with its `#[event]` attribute) moves verbatim into `instructions/deposit.rs`.

`instructions/mod.rs` — **explicit struct re-exports, NOT globs**: five modules each export a
`pub fn handler`, so `pub use module::*;` ×5 would create ambiguous glob re-exports (the
`ambiguous_glob_reexports` warning fails our `-D warnings` gate). Handlers stay reachable as
`instructions::<name>::handler` (how lib.rs calls them); only the unique-named types re-export:
```rust
pub mod cancel_intent;
pub mod commit_intent;
pub mod deposit;
pub mod execute_round;
pub mod initialize_pool;

pub use cancel_intent::CancelIntent;
pub use commit_intent::CommitIntent;
pub use deposit::{Deposit, DepositEvent};
pub use execute_round::ExecuteRound;
pub use initialize_pool::InitializePool;
```

- [ ] **Step 2: rewrite `lib.rs` as the delegating shell**

`lib.rs` becomes exactly: the existing crate-header comment(s) + `use anchor_lang::prelude::*;` +
the existing `pub mod` list (now including `pub mod errors;` and `pub mod instructions;`) +
`pub use errors::PoolError;` + `pub use instructions::*;` + the UNCHANGED
`declare_id!("7oHnDkpPbhPacDfqzF38caM3eo1Xo7cBmFugNXJurnn3");` + the `#[program]` mod:

```rust
#[program]
pub mod pool_program {
    use super::*;

    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        denomination: u64,
        k_floor: u16,
        action_kind: u8,
        validator: Pubkey,
        fee: u64,
    ) -> Result<()> {
        instructions::initialize_pool::handler(ctx, denomination, k_floor, action_kind, validator, fee)
    }

    pub fn deposit(ctx: Context<Deposit>, commitment: [u8; 32], amount: u64) -> Result<()> {
        instructions::deposit::handler(ctx, commitment, amount)
    }

    pub fn commit_intent(
        ctx: Context<CommitIntent>,
        proof: crate::verifier::WithdrawProof,
        root: [u8; 32],
        nullifier_hash: [u8; 32],
        fee: u64,
        round_id: u64,
    ) -> Result<()> {
        instructions::commit_intent::handler(ctx, proof, root, nullifier_hash, fee, round_id)
    }

    pub fn cancel_intent(
        ctx: Context<CancelIntent>,
        _round_id: u64,
        _nullifier_hash: [u8; 32],
    ) -> Result<()> {
        instructions::cancel_intent::handler(ctx, _round_id, _nullifier_hash)
    }

    pub fn execute_round<'info>(
        ctx: Context<'_, '_, 'info, 'info, ExecuteRound<'info>>,
        round_id: u64,
    ) -> Result<()> {
        instructions::execute_round::handler(ctx, round_id)
    }
}
```
Doc comments that sat ON the `#[program]` fns (e.g. `cancel_intent`'s safety-valve comment) stay
on the delegating fns (public API surface). `#[program]` fn names, parameter names/orders/types:
byte-identical (discriminators + IDL derive from them).

- [ ] **Step 3: gate + commit**

Run: `anchor build && cargo test --workspace 2>&1 | tail -5 && cargo fmt --check && cargo clippy --all-targets -- -D warnings 2>&1 | tail -2`
Expected: all green, test counts unchanged (the per-instruction binaries exercise real dispatch
through the freshly-built .so — this is the discriminator/behavior proof).

```bash
git add -A && git commit -m "refactor(pool-program): lib.rs -> instructions/ (one file per instruction, Squads-v4 shape; verbatim moves)"
```

---

### Task 3: Piece 4 — SDK split `note.rs` / `tree.rs` / `ix.rs`

**Files:**
- Create: `crates/sdk/src/{note.rs,tree.rs,ix.rs}`
- Modify: `crates/sdk/src/lib.rs`

**Interfaces:** every existing public path unchanged: `sdk::{SdkError, Note, MerklePath,
MerkleTree, WithdrawArtifacts, CommitIntentBuild, compute_ext_data_hash, WithdrawInputs,
PublicInputs, FieldBytes, ProverError, TREE_DEPTH, build_initialize_pool_ix, build_deposit_ix,
build_commit_intent_ix, build_execute_round_ix, build_execute_stake_round_ix,
build_cancel_intent_ix, round_pda, stake_account_pda}`.

- [ ] **Step 1: allocate and move (verbatim bodies)**

- `note.rs`: `Note` + its `impl` + `impl Default` (lib.rs ~51-133).
- `tree.rs`: `MerklePath`, `MerkleTree` + impl (~205-284).
- `ix.rs`: everything else public-facing that remains: `WithdrawArtifacts`, `CommitIntentBuild`,
  all six `build_*_ix` fns, `round_pda`, `stake_account_pda`, the private `discriminator` helper
  and any other private helpers the builders use (~136-203, 286-524).
- **Allocation is by ITEM NAME; the line ranges are approximate hints** (fork plan-gate note —
  e.g. the `discriminator` helper sits at ~:127, inside note.rs's numeric range, but it is an
  ix.rs item by name). When a range and a name disagree, the name wins.
- `SdkError` STAYS in `lib.rs` (shared by note+tree; moving it to either would be arbitrary).
  The `pub use ext_data::… / pub use prover::…` re-export lines (~31-32) also stay in `lib.rs`.
- The existing `#[cfg(test)]` module at the bottom of lib.rs (~526+): move it into the module
  whose items it tests if it tests one module's items, else leave in `lib.rs` — decide by reading
  it; either is behavior-identical; state the choice in your report.
- New `lib.rs` shape: crate doc-comment (verbatim) + the two re-export lines + `SdkError` +
  `mod note; mod tree; mod ix;` + `pub use ix::*; pub use note::*; pub use tree::*;` + each
  module's `use` headers resolved compile-driven.

- [ ] **Step 2: gate + commit**

Run: `cargo test --workspace 2>&1 | tail -5 && cargo fmt --check && cargo clippy --all-targets -- -D warnings 2>&1 | tail -2`
(no program change → no anchor build strictly needed; running it anyway is harmless.)
Expected: all green; `crates/sdk/tests/*` and `programs/pool-program/tests/*` compile with ZERO
edits — that compile is the public-path proof.

```bash
git add -A && git commit -m "refactor(sdk): split lib.rs into note/tree/ix modules (flat re-exports; public paths unchanged)"
```

---

### Task 4: Piece 5 — withdraw → membership rename cascade (riskiest; last)

**Files:** `circuits/{circom,scripts,test,package.json}`, `crates/prover`, `crates/vk-gen`,
`crates/sdk` (+tests), `programs/pool-program` (`verifier.rs`, `vk.rs`, `instructions/commit_intent.rs`
+ lib.rs delegating signature, tests), `circuits/README.md`, prover/sdk doc-comment path refs.

**Interfaces:** renames `WithdrawProof→MembershipProof`, `verify_withdraw→verify_membership`,
`WITHDRAW_VK→MEMBERSHIP_VK`, `prove_withdraw→prove_membership`, `WithdrawInputs→MembershipInputs`,
`WithdrawArtifacts→MembershipArtifacts`, artifacts `withdraw.*`/`withdraw_js`→`membership.*`/
`membership_js`. The soak (next work item) builds against these final names.

- [ ] **Step 1: determinism pre-check (spec hard-stop protocol, item 2)**

With the UNMODIFIED circuit: run `bash circuits/scripts/setup.sh` twice; after each run
`shasum circuits/build/verification_key.json` (and `cargo run -p vk-gen` → `shasum` the generated
vk.rs or its stdout). Identical hashes across runs → determinism confirmed; record both hashes.
If they differ, STOP, report NEEDS_CONTEXT with the hashes — the guard strategy changes (spec item 3).

- [ ] **Step 2: the partition inventory (STOP-on-ambiguous)**

Run: `grep -rn -i "withdraw" --include="*.rs" --include="*.circom" --include="*.js" --include="*.sh" --include="*.json" --include="*.toml" --include="*.md" --include="*.yml" . | grep -v "target/\|node_modules\|circuits/build/\|Cargo.lock\|docs/superpowers\|.superpowers"`
(note: `.yml` included and `docs/research` NOT excluded — fork spec-gate fold, 2026-07-19).
Write EVERY hit to `.superpowers/sdd/withdraw-partition.md` as a table: file:line | token |
RENAME or KEEP | one-word reason. Four hits are PRE-CATEGORIZED by the fork's spec gate and must
appear in the table exactly so:
- `.github/workflows/ci.yml` (~:155) step name "…+ withdraw membership/nullifier" → **RENAME**
  (one word → "membership/nullifier"; CI invokes `npm test` with no filenames, so nothing else
  changes).
- `docs/research/crowd-depth-and-timing-mechanisms.md` — five `withdraw.circom` cites (incl.
  line-cites) → **KEEP** (dated historical research record; spec's no-docs-prose rule).
- `docs/research/behavioral-rounds-followup-proposal.md:356` — a Tornado Cash URL ending in
  `withdraw.circom` → **KEEP — EXTERNAL LINK, MUST NOT CHANGE** (the exact trap a careless sweep
  corrupts).
- Any other `docs/research/` hit → KEEP as historical record unless it's a live wrong-path claim
  about OUR tree post-rename; if you find one of those, STOP and surface it (don't decide alone). Category rules (spec): RENAME = the 6 symbols + artifact
filenames/paths + wrong-after-move path prose (circuits/README.md legacy note + files table;
prover/sdk doc comments citing `circuits/circom/withdraw.circom`). KEEP = `WithdrawAction`,
`MAX_K_WITHDRAW`, `ActionKind::Withdraw`, `build_execute_round_ix`, **`withdrawer`** (native
stake-authority field — renaming breaks compile), the `tx_envelope.rs` withdraw-action test fn
name, `let withdraw_proof`-style locals, "withdraw pool/arm" action prose, `PoolError` variants,
`circuits/test/withdraw_vectors.json` (+ every reference to it), and known non-cascade files
(`merkle_parity.test.js`'s vectors reference, `merkle_proof_main.circom` if its hit is a comment).
**Any hit that fits neither category cleanly: STOP and report it — do not guess.** Zero
uncategorized rows before proceeding.

- [ ] **Step 3: execute the renames (per the partition table, symbol-by-symbol — no global sed)**

1. `git mv circuits/circom/withdraw.circom circuits/circom/membership.circom`;
   `git mv circuits/test/withdraw.test.js circuits/test/membership.test.js`.
2. Edit `circuits/scripts/setup.sh` + `circuits/package.json` scripts + the moved test's internal
   circuit/artifact references: `withdraw`→`membership` for circuit/artifact tokens only
   (NOT the vectors filename).
3. Rust symbol renames per the table (IDE-precision: rename each symbol's definition then chase
   compile errors — the compiler is the completeness check for code tokens): the 6 symbols across
   `verifier.rs`, `vk.rs` (const name line), `vk-gen/src/main.rs` (template string ~103 + unit
   test ~228), `prover/src/lib.rs`, `sdk` (incl. the `pub use prover::…WithdrawInputs…` line →
   `MembershipInputs`), `commit_intent.rs`/`lib.rs` (the `proof: crate::verifier::MembershipProof`
   parameter type — the parameter TYPE renames; the `#[program]` fn name `commit_intent` does not),
   and every test file's artifact-path strings (`withdraw_js/withdraw.wasm`, `withdraw.r1cs`,
   `withdraw.zkey` → `membership_js/membership.wasm`, `membership.r1cs`, `membership.zkey`).
4. Path-prose: `circuits/README.md` (discharge the "still named withdraw… rename tracked" note —
   now describe `membership.circom` plainly — and the files table), prover/sdk doc comments citing
   the .circom path (`crates/prover/src/lib.rs:21`, `crates/sdk` — per the partition table), and
   the one-word CI step name in `.github/workflows/ci.yml` ("withdraw membership/nullifier" →
   "membership/nullifier").

- [ ] **Step 4: regenerate + the VK hard-stop**

`rm -rf circuits/build && bash circuits/scripts/setup.sh` (produces `membership.*` artifacts) then
`cargo run -p vk-gen` (regenerates `vk.rs` with `MEMBERSHIP_VK`).
**Primary guard:** `git diff programs/pool-program/src/vk.rs` must show exactly ONE changed line —
the const name. Any hex-byte line change = **STOP AND REPORT** (do not commit, do not rationalize).
Also run vk-gen's `--check` mode if wired — must pass against the renamed template.

- [ ] **Step 5: full gate + commit**

Run: `anchor build && cargo test --workspace 2>&1 | tail -6 && (cd circuits && npm test) 2>&1 | tail -5 && cargo fmt --check && cargo clippy --all-targets -- -D warnings 2>&1 | tail -2`
Expected: everything green — parity tests, `prover::prove_verify`, pool-program `verifier`, sdk
`e2e` real proofs, JS circuit tests — with unchanged counts. Attach the partition table +
determinism hashes + the one-line vk.rs diff to your report.

```bash
git add -A && git commit -m "refactor: rename membership circuit withdraw->membership (symbols + artifacts; VK byte-identical; withdraw ACTION surface untouched)"
```
