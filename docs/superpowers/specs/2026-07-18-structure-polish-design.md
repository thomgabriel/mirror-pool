# Structure-polish — behavior-identical hygiene pass

**Date:** 2026-07-18 · **Status:** approved design, pending fork spec-review
**Grounding:** `docs/superpowers/plans/2026-07-18-structure-polish-brief.md`,
`docs/research/code-craft-and-repo-hygiene.md` (Part 1.C Squads-v4 shape; Part 2 punch-list)
**Branch:** `feat/structure-polish` off `main` (`e9f9c68`, post-F1)

## 1. Frame — and the honest scoping

File splits and one rename cascade for **reviewability on a code-reviewed bounty**. The hygiene
audit itself files the splits as YAGNI watch-items, *not* violations — so the bar is: pure moves
and renames, no new abstractions, no drive-by changes. This pass exists to make the next reviewer's
read cheaper, nothing else.

## 2. The overriding guard — BEHAVIOR-IDENTICAL

- **Zero logic change.** Every hunk is a move or a rename. A changed condition, reordered check, or
  added/removed logic line is a defect by definition.
- **Full verification at every commit:** `cargo test --workspace` green with **unchanged test
  assertions** (a changed assertion means behavior moved — red flag), `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`. Five pieces = five commits; never combined.
- **SBF rebuild BEFORE the test run** on every piece touching `programs/pool-program/` (2, 3, 5):
  the LiteSVM tests load a prebuilt `target/deploy/pool_program.so` that `cargo test` does NOT
  rebuild — running the suite without `anchor build` first exercises the OLD binary and produces a
  false green exactly where it matters (piece 3's dispatch, piece 5's embedded VK). The gate is:
  `anchor build && cargo test --workspace` in that order (internal review F1, 2026-07-18).
- Public surfaces frozen: error-code ABI (6000–6024), every `sdk::*` path, `declare_id!`, the VK
  bytes, all test files (except piece-5 symbol renames inside them and piece-1 comment fixes).

## 3. The five pieces (sequenced low-risk → high-risk; one commit each)

### Piece 1 — punch-list warm-up (docs-in-code only, + vk-gen expect discipline)
- Punch-list **#1 is already fixed at HEAD** (verified 2026-07-18: `stake_account_pda`'s comment
  correctly cites `programs/pool-program/src/lib.rs`; no `round.rs` reference remains). Record
  that; no edit needed for it.
- **Sweep** the workspace for stale cross-references in doc comments: wrong-file/`file:line` refs
  invalidated by F1's growth, and stale *plan-relative* references (known instances: 
  `programs/pool-program/src/invariants.rs` "Task 2 adds a compile-time assert… in action.rs" — the
  assert exists; rephrase to point at it, not at a plan task; `tests/round_support.rs` "parity
  proven in Task 1" — cite the parity test, not the plan task). Fix each to reference code, not
  plan history. Comments only — zero code tokens change.
- Punch-list **#4**: `crates/vk-gen/src/main.rs` — replace the bare `.unwrap()`s with
  `.expect("<specific msg>")` matching the file's existing message style. Build-time codegen on
  developer-controlled input; message-discipline only.
- **Out of scope (recorded):** punch-list #2 `[workspace.dependencies]` hoist — manifest/lockfile
  churn violates the behavior-identical bar; stays a cited watch-item. #3 README — already exists.

### Piece 2 — `PoolError` → `errors.rs`
- Move the `#[error_code] pub enum PoolError` from `lib.rs` to a new
  `programs/pool-program/src/errors.rs` **verbatim** — the variant order IS the error-code ABI
  (6000–6024); byte-for-byte identical enum body.
- `lib.rs`: `pub mod errors;` + `pub use errors::PoolError;` so every existing `crate::PoolError`
  / `PoolError::X` path (including `deposit.rs`-era hardcoded-code tests and all `require!` sites)
  resolves unchanged.
- Guard — **the existing tests are NOT sufficient for this piece** (fork + internal review,
  concurring, 2026-07-18): nearly all error assertions are name-based log matches
  (`l.contains("RoundFull")`) or `Custom(_)` wildcards, which pass even if a reorder shifts every
  numeric code (the `#[msg]`/name travels WITH the variant); only 6001/6002 are pinned numerically
  anywhere (`deposit.rs:25-26`). Two guards, both required:
  1. *Structural (move-verification):* byte-diff the extracted `enum PoolError { … }` body between
     the old `lib.rs` and the new `errors.rs` — zero delta. Total and falsifiable for THIS move.
  2. *Permanent (regression guard):* piece 2 **adds one host test** in `errors.rs` pinning the
     FULL ABI numerically — `assert_eq!(PoolError::<Variant> as u32, <index>)` for all 25
     variants, `MerkleInit`=0 … `KFloorTooHigh`=24 (Anchor code = 6000 + discriminant) — so any
     FUTURE reorder also fails a test. The pass's sole sanctioned test addition (§5).
- Existing name/code assertions still pass unchanged, as before.
- The `deposit.rs:22-23` comment citing "see `programs/pool-program/src/lib.rs`" for the error
  list may be repointed to `errors.rs` in this piece (sanctioned one-comment carve-out — §2's
  freeze rule otherwise forbids test-file edits; internal review F6).

### Piece 3 — `lib.rs` → `instructions/` (Squads-v4 one-file-per-instruction)
- `programs/pool-program/src/instructions/{initialize_pool,deposit,commit_intent,cancel_intent,execute_round}.rs`
  — each file holds that instruction's **handler fn + its `#[derive(Accounts)]` context struct**,
  moved verbatim. `instructions/mod.rs` declares + `pub use`s all five.
- `lib.rs` becomes: `declare_id!` (unchanged — the program id must not move), module wiring, and
  the `#[program]` mod whose fns **delegate**: `pub fn initialize_pool(ctx, …) -> Result<()> {
  instructions::initialize_pool::handler(ctx, …) }` — signatures identical (Anchor's IDL and
  discriminators derive from these fn names; they must not change).
- `DepositEvent` moves into `instructions/deposit.rs` (its only emitter), re-exported so its path
  stays importable. No new `events.rs` (one event ≠ a module).
- The two load-bearing `execute_round` comment blocks move **verbatim**: the named-`'info`
  invariance rationale and the why-no-`round.state`-recheck constraint walk-through.
- **The load-bearing naming rule:** the `#[program]` fn names AND every `#[derive(Accounts)]` /
  `#[account]` struct name stay **byte-identical** — instruction discriminators and the IDL derive
  from these names. Move bodies; never rename. (Visibility/scope mistakes are compile-time-caught;
  name drift is the silent one.)
- Two mechanics the plan must carry verbatim (internal review): `lib.rs` `use`s the moved context
  structs into the `#[program]` mod's scope (the macro needs the types resolvable), and the
  `execute_round` delegating wrapper reproduces the full 4-lifetime signature
  `Context<'_, '_, 'info, 'info, ExecuteRound<'info>>` exactly.
- Guard: all per-instruction LiteSVM test binaries pass unchanged (they build instruction data via
  `disc("name")` — name → sha256 discriminator — and hit the real dispatch through the .so, so any
  discriminator break fails them end-to-end); `anchor build` succeeds.

### Piece 4 — SDK split: `note.rs` / `tree.rs` / `ix.rs`
- `crates/sdk/src/lib.rs` (725 lines) splits: `note.rs` (`Note` + note serialization),
  `tree.rs` (`MerkleTree`, paths), `ix.rs` (all `build_*_ix` builders, PDAs, discriminator,
  ext-data/proving glue — whatever remains after note/tree). Exact allocation decided at plan time
  from the real file; the requirement is the **flat re-export**: `lib.rs` = `mod` decls +
  `pub use note::*; pub use tree::*; pub use ix::*;` (plus any existing re-exports like prover
  types) so **every** current public path (`sdk::Note`, `sdk::MerkleTree`,
  `sdk::build_execute_round_ix`, `sdk::compute_ext_data_hash`, `sdk::WithdrawInputs`, …) resolves
  identically.
- Guard: `crates/sdk/tests/{sdk,e2e,tx_envelope}.rs` and `programs/pool-program/tests/*` compile
  and pass with **zero edits**.

### Piece 5 — circom rename cascade: withdraw → membership (riskiest; last; own task)
The membership circuit is misnamed "withdraw" (it proves note membership for every action kind).

**Rename by enumerated symbol — never a blind textual `s/withdraw/membership/`:**

| Old | New |
|---|---|
| `circuits/circom/withdraw.circom` | `circuits/circom/membership.circom` |
| build artifacts `withdraw.r1cs` / `withdraw.zkey` / `withdraw_js/` / `withdraw.wasm` / `withdraw.sym` (gitignored; produced by setup.sh) | `membership.*` / `membership_js/` |
| `circuits/test/withdraw.test.js` (+ its internal circuit refs) | `circuits/test/membership.test.js` |
| `WithdrawProof` (pool-program `verifier.rs`) | `MembershipProof` |
| `verify_withdraw` | `verify_membership` |
| `WITHDRAW_VK` (`vk.rs`, vk-gen output) | `MEMBERSHIP_VK` |
| `prove_withdraw` (`crates/prover`) | `prove_membership` |
| `WithdrawInputs` | `MembershipInputs` |
| `WithdrawArtifacts` | `MembershipArtifacts` |

Cascade covers: `circuits/` (circom source, setup.sh, package.json scripts, JS tests),
`crates/prover`, `crates/vk-gen` (**including** the codegen template at `main.rs:103` and its unit
test asserting `pub const WITHDRAW_VK` — else `vk-gen --check` drift-fails for a non-crypto
reason), `crates/sdk` (+ its tests), `programs/pool-program` (`verifier.rs`, `vk.rs`, handler call
sites, `tests/{verifier,round_support}.rs` and the other test binaries' artifact paths), **and the
path-reference prose that goes stale with the file move** (internal review F7):
`circuits/README.md` (the legacy-name note — which this pass finally discharges — and the files
table) plus the Rust doc comments citing `circuits/circom/withdraw.circom` by path
(`crates/prover/src/lib.rs`, `crates/sdk`). Leaving those would re-create the exact wrong-file
defect class piece 1 removes.

**Shared fixture stays (internal review F3):** `circuits/test/withdraw_vectors.json` is consumed
by BOTH the renamed circuit test AND `circuits/test/merkle_parity.test.js` (out of the cascade)
plus the Rust fixture loaders. It is **deliberately NOT renamed** in this pass — the residual name
is recorded here as intentional so neither implementer nor reviewer treats it as a miss. (Rename
deferred; would touch parity files outside this cascade.)

**DO-NOT-TOUCH list (the Withdraw *action* — one of two pooled actions — not the circuit):**
`WithdrawAction` (`action.rs`), `MAX_K_WITHDRAW`, `ActionKind::Withdraw`, `build_execute_round_ix`,
`PoolError` variants/messages, "withdraw pool"/"withdraw arm" prose in action-context comments, and
every `#[program]` fn name (ABI). Additionally (internal review F5 — real survivors a careless
pass would corrupt): **`withdrawer`** — the native-stake *withdraw authority* field
(`action.rs:70,186,189` sets `withdrawer: self.recipient` on a Solana stake-program struct;
renaming breaks compile) and its test references; the withdraw-action test fn
`tx_envelope.rs::withdraw_execute_round_at_max_k_fits_64_account_locks`; and local variables like
`let withdraw_proof = …` in tests — these compile fine post-rename and are explicitly OUT of
scope (leaving them is behavior-identical; "tidying" them is a drive-by). The rename set is the
enumerated symbols + artifact paths + F7's path-prose ONLY; every other `withdraw*` token
survives. When a sentence mixes both meanings, only the circuit/proof tokens change.

**CRYPTO HARD-STOP — primary guard is the committed file, regeneration is corroboration
(fork finding + internal review F4, 2026-07-18):**
1. *Primary, always-available, falsifiable:* `git diff` of `programs/pool-program/src/vk.rs`
   against the **committed pre-rename file** must show **exactly one changed line — the const
   name** (`WITHDRAW_VK` → `MEMBERSHIP_VK`); every hex byte line identical. This needs no
   toolchain and cannot false-positive.
2. *Corroboration — verify determinism first:* BEFORE the rename, run `circuits/scripts/setup.sh`
   + `crates/vk-gen` twice on the unmodified circuit and byte-compare the outputs. Setup is
   expected deterministic (sha256-pinned ptau, fixed beacon + iteration count — verified in
   source); if the double-run confirms it, re-run after the rename and require the regenerated VK
   bytes identical too. A byte delta here in a *mismatched toolchain* (wrong circom/snarkjs
   version, missing ptau) is an environment artifact, NOT a rename leak — resolve the environment
   before concluding anything; the committed-file diff (1) remains the ground truth.
3. *If determinism unexpectedly fails:* fall back to the honest functional guard — the
   regenerated VK must verify proofs end-to-end — and record why byte-identity was unavailable.
**Any true VK byte delta in the committed file = STOP and report.** Either way, the full proof
chain re-verifies before the piece-5 commit lands: circom parity tests, `prover::prove_verify`,
pool-program `verifier` test, SDK `e2e` real-proof rounds — all against a freshly `anchor build`-t
`.so` (§2).

**Rename-completeness discipline — partition, don't pattern-match:** enumerate EVERY
`withdraw`/`Withdraw` hit in the workspace (code, tests, circuits, scripts, package.json,
fixtures) and categorize each as rename-XOR-keep with **zero left uncategorized**. A hit that fits
neither "clearly circuit/proof" nor "clearly withdraw-action" is the stop condition — surface it
to the orchestrator, don't guess. The plan carries the full partitioned inventory.

**Why last and why now:** highest regression surface; and the soak (next work item) calls
`prove_*`/proof types — landing this first means the soak is written once against final names.

## 4. Task grouping for the plan

Task 1 = pieces 1 + 2 (two commits). Task 2 = piece 3. Task 3 = piece 4. Task 4 = piece 5.
Each piece's commit lands only with the full green gate of §2.

## 5. Non-goals

No `[workspace.dependencies]` hoist (#2). No new abstractions, traits, or helper layers. No test
additions or assertion changes — with exactly ONE sanctioned exception: piece 2's numeric
error-ABI pin test (mandated by the fork's guard-gap finding; the name-based assertions cannot
catch a reorder). No `Pool`/`Round`/`Intent` layout or logic edits. No docs-prose pass (already
done in `88c4260`); code-comment fixes limited to piece 1's stale-reference sweep.

## 6. Process

`feat/structure-polish` → fork spec review → plan (`writing-plans`) → fork plan review →
subagent-driven build (4 tasks) → whole-branch review (top-tier model; bar = **no behavior
delta**: pure moves/renames, unchanged assertions, ABI order, VK bytes, public paths) → fork
reviews merged branch → local merge. No push without the user's explicit yes.
