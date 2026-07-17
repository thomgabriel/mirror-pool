---
title: Code-craft & repo-hygiene checklist → mirror-pool
date: 2026-07-17
status: research (informational checklist — Part 2 is an actionable punch-list; re-verify each file:line against HEAD before applying)
scope: Cited best-practices checklist (comment discipline, Rust API Guidelines, Solana/Anchor structure, Cargo workspace hygiene) plus a point-in-time, read-only punch-list audit of this repo
method: Direct-fetch citations where stated; Part 2 audit via ls/find/wc/grep/git ls-files/Read only — no cargo build/test/clippy/fmt run, deliberately, since another process was compiling programs/ and crates/ concurrently and a second cargo invocation would contend for target/
caveat: >-
  Part 2 is a snapshot from branch feat/timeout-cancel (worktree clean at capture,
  nothing edited/staged/committed during the audit). By the time this doc was written,
  feat/timeout-cancel had already merged to main (53f9c08) — see the timing note at the
  top of Part 2 and the Limitations section for exactly what was and wasn't re-verified.
---

# Code-craft & repo-hygiene checklist → mirror-pool

A synthesis of the CODE-CRAFT research strand: cited engineering best practices, mapped
onto concrete mirror-pool decisions and residuals, followed by a point-in-time punch-list
for this repo. Nothing here recommends new abstraction, config, or frameworks — per
CLAUDE.md's YAGNI section, every item below is either "already done" (credited in Part 3)
or a minimal, cited fix (Part 2), with anything more speculative explicitly marked as
future work rather than a recommendation.

---

## Part 1 — Best-practices checklist (cited)

### A. Comment discipline — why, not what

| Rule | Citation | mirror-pool tie |
|---|---|---|
| Comments should capture "information that was in the mind of the designer but couldn't be represented in the code" — rationale, not mechanics. | Ousterhout, J. K., *A Philosophy of Software Design*, 2nd ed., Yaknyam Press, 2021, ISBN 978-1732102217, Ch. 13. [bibliographic facts confirmed; exact chapter prose is secondary-sourced only — the primary Stanford PDF exceeded the original research pass's fetch size limit] | This is CLAUDE.md's own "Comments — signal only" section, near-verbatim. Part 3 lists the repo's own examples of it working. |
| "Delayed comments are bad comments" — write the comment as you write the code, not after. | Same work, Ch. 15 "Write the Comments First." [same secondary-sourcing caveat as above] | Pairs with CLAUDE.md's "spec → plan → TDD" workflow — a rationale comment is a design artifact produced *during* the work, not paperwork added after. |
| Errors/panics/safety belong in dedicated doc sections; `unsafe` fns need a `# Safety` section listing caller-upheld invariants. | Rust API Guidelines, "Documentation" (C-FAILURE), https://rust-lang.github.io/api-guidelines/documentation.html — fetched directly, quoted verbatim. | mirror-pool's `PoolError` (24 variants in `lib.rs`) is the program's entire typed-error surface (CLAUDE.md: "on-chain paths return typed program errors"). No `unsafe` in the program; C-FAILURE's relevance today is to any future doc comments on instruction handlers, not a current gap. |
| Crate-level docs should be thorough with examples (C-CRATE-DOC); prose should hyperlink related items (C-LINK). | Same page, fetched directly. | `crates/sdk` is the client-facing proving surface (CLAUDE.md: "proving is client-side") — the natural place for this to matter most. Doc-comment *coverage* wasn't exhaustively audited in this pass; not claimed as a finding either way. |
| Prefer line comments (`//`) over block (`/* */`); first doc-comment line is a one-sentence, third-person-present summary ("Returns…"). | Rust RFC 505, "API Comment Conventions," https://rust-lang.github.io/rfcs/0505-api-comment-conventions.html — fetched directly, quoted verbatim. | Directly governs the doc comment with the stale cross-reference in punch-list #1 (`crates/sdk/src/lib.rs:426`) — a one-line summary is exactly what drifted out of sync with the code it describes. |
| Doc-comment template: summary → explanation → example → edge cases; a "Panics" section is recommended whenever reachable. | *The rustdoc Book*, "How to Write Documentation," https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html — fetched directly. | Not flagged as missing anywhere in this pass — listed for completeness, applicable if/when new public doc comments get written. |

### B. Rust code craft for correctness-critical code

| Rule | Citation | mirror-pool tie |
|---|---|---|
| `panic!`/`unwrap`/`expect` belong in prototypes, tests, or cases where you "have more information than the compiler" — documented inline. Code taking untrusted input should return `Result`. | *The Rust Programming Language*, Ch. 9.3 "To panic! or Not to panic!," https://doc.rust-lang.org/book/ch09-03-to-panic-or-not-to-panic.html — fetched directly, quoted verbatim. | This **is** CLAUDE.md's "fail closed" rule. The audit found `programs/pool-program/src` fully compliant — zero `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!` outside `#[cfg(test)]` on the attacker-facing surface — and `crates/sdk`'s `Note::new()` (`.expect`, invariant documented inline) vs. `Note::from_parts()` (untrusted input → `Result`) as a textbook-correct application, credited in Part 3. |
| Error types follow verb-object-error naming (`ParseAddrError`, not `AddrParseError`). | Rust RFC 236, "Error Conventions," https://rust-lang.github.io/rfcs/0236-error-conventions.html | Applies to `PoolError`'s variants (e.g. `CancelTooEarly`). Naming wasn't spot-checked against RFC 236 in this pass — listed as unaudited, not as a finding. |
| Type-level items `UpperCamelCase`, value-level `snake_case`; acronyms are word-cased (`Uuid`, not `UUID`). | Rust API Guidelines, "Naming" (C-CASE), https://rust-lang.github.io/api-guidelines/naming.html | `PooledAction`/`WithdrawAction`/`StakeAction`/`RoundState` read as compliant on inspection; not formally re-checked here. |
| YAGNI: "Always implement things when you actually need them, never when you just foresee that you [will] need them." | Jeffries, Anderson, Hendrickson, *Extreme Programming Installed*, Addison-Wesley, 2000 — via https://en.wikipedia.org/wiki/You_aren%27t_gonna_need_it. [the book is the primary source; the Wikipedia aggregator is what was actually fetched] | This is CLAUDE.md's own "No overengineering (YAGNI)" section. The audit's best example: `invariants.rs:53-58`'s `TIMEOUT_SLOTS` comment states its own epistemic status honestly ("a workload-contingent judgment call... kept a const here to avoid unused config surface") — YAGNI reasoning applied *and* narrated, on the exact feature this branch shipped. |
| Every dependency is supply-chain surface: real incidents include a Sept. 2023 typosquat wave (9 crates exfiltrating host metadata via malicious `build.rs`) and the May 2022 "CrateDepression" `rustdecimal` typosquat (Go/Mythic payload into GitLab CI). | Harvey, A., "crates.io: Malicious Crates Postmortem," *Inside Rust Blog*, Sept. 1 2023, https://blog.rust-lang.org/inside-rust/2023/09/01/crates-io-malware-postmortem/ — fetched directly. CrateDepression: SentinelOne Labs, https://www.sentinelone.com/labs/cratedepression-rust-supply-chain-attack-infects-cloud-ci-pipelines-with-go-malware/ [secondary vendor write-up, corroborated by one independent advisory (Cycode), not a primary incident disclosure]. | mirror-pool already operationalizes "justify before adding a dependency" (CLAUDE.md) via `deny.toml`'s per-RUSTSEC-ID justified ignores (not a blanket suppression) — credited in Part 3. No new dependency-hygiene action is recommended; the audit found nothing superfluous (§2.5 of the source audit). |
| Tooling for "minimize/justify dependencies": RustSec Advisory DB (what `cargo-audit`/`cargo-deny` query) and Mozilla's `cargo-vet` (per-dependency audit certification). | RustSec, https://rustsec.org/ ; Mozilla, `cargo-vet`, https://mozilla.github.io/cargo-vet/ | `deny.toml` is the RustSec-consuming half of this already in place. `cargo-vet` is not adopted and not recommended here — no evidence of a gap it would close; noted as cited future work only, not a recommendation. |

### C. Solana/Anchor program structure

| Pattern (real repos, not blog paraphrase) | Source | mirror-pool tie |
|---|---|---|
| **Squads Protocol v4** — `instructions/` holds one file per instruction (handler + its `Accounts` struct together), `mod.rs` re-exports. | https://github.com/Squads-Protocol/v4 | The precedented next step **if/when** `lib.rs` (696 lines, largest pure-production file, see punch-list #5) needs splitting — a vertical, same-layer split, not the generic-bucket "by layer" split CLAUDE.md warns against. Not recommended now (YAGNI); watch-item only. |
| **SPL Token-2022** — flatter native (non-Anchor) layout, split by *concern* (`processor.rs` centralizes dispatch), not per-instruction. | https://github.com/solana-program/token-2022 | Not directly applicable — mirror-pool is Anchor, not native. Noted for contrast: the per-instruction split is an Anchor-ecosystem convention, not a universal Solana one. |
| **Light Protocol** — workspace-of-workspaces: on-chain program crates (`programs/`) and host-side pure/SDK crates (`program-libs/`, `sdk-libs/`) are separate top-level groups. | https://github.com/Lightprotocol/light-protocol | This is mirror-pool's existing `programs/` + `crates/` split, already in place — credited in Part 3, not a change to make. |
| **Anchor's own scaffold** (`anchor init`) — `lib.rs` is `declare_id!` + module re-exports only, not the logic itself. | https://www.anchor-lang.com/docs/quickstart/local ; https://www.anchor-lang.com/docs/basics/program-structure [the program-structure page documents `#[program]`/`#[derive(Accounts)]` macro mechanics, not the folder convention itself — the folder convention is attested by the quickstart page and by Squads v4, not this page] | mirror-pool's `lib.rs` currently holds the full `#[program]` dispatch table rather than re-exporting instruction modules. Per the audit, this is a legitimate choice at the program's current size — each handler already delegates its logic out to `action.rs`/`invariants.rs`/`merkle.rs` — not a violation of the scaffold convention. |
| Secondary corroboration: same `entrypoint.rs`/`instructions/`/`processor/`/`state/`/`error.rs`/`utils/` shape. | RareSkills, "Organizing a Solana Program," https://rareskills.io/post/organizing-solana-programs (updated Feb 26 2026) [developer blog, not a spec — included only because it independently converges with the three primary-source repos above] | Not separately actionable; corroborates the Squads v4 / Light Protocol shape. |

### D. Rust workspace / repo-root organization

| Rule | Citation | mirror-pool tie |
|---|---|---|
| A workspace root with no `[package]` section is a "virtual manifest" — for when there's no single "primary" package; `members` supports globs. | *The Cargo Book*, "Workspaces," https://doc.rust-lang.org/cargo/reference/workspaces.html — fetched directly. | mirror-pool's root `Cargo.toml` already does exactly this (`members = ["programs/*", "crates/*"]`) — credited in Part 3. |
| `[workspace.dependencies]` centralizes duplicated version strings across members, inherited via `dep.workspace = true`. Stock Cargo feature, not a custom layer. | Same page. | **Not currently used**, despite 7 version strings duplicated across ≥2 member `Cargo.toml`s (`solana-sdk`, `litesvm`, `bincode`, `serde_json`, `ark-bn254`, `num-bigint`, `groth16-solana`) — punch-list #2, real issue/low severity. Minimum recommended fix: adopt the stock feature, nothing more. |
| `[workspace.lints]` can share one lint table across every member via `[lints] workspace = true`. | Same page. | **Not recommended now** — no evidence in this audit of lint-table divergence across members. Cited as future work only, to reach for *if* lint config actually starts drifting, not before. |

---

## Part 2 — Punch-list for this repo (point-in-time, read-only)

**Timing note (as of 2026-07-17, the date this doc was written):** the underlying audit
was captured on branch `feat/timeout-cancel` (worktree clean, nothing edited during the
audit). Since then, `feat/timeout-cancel` has already merged to `main` (`53f9c08 Merge
Plan 6a: timeout-gated cancel`), with two further commits (`b84f3b0`, `f747a5c`) landing
on top of that merge. So "apply after the branch merges" is now literally true, but the
merge itself post-dates the snapshot below — **re-diff every file:line against current
`HEAD` before acting on any of this; treat line numbers as approximate, not exact.** See
Limitations for exactly what was and wasn't spot-re-checked while writing this doc.

No file under `programs/` or `crates/` was modified to produce this list or this
document — everything below is carried forward from the source audit as a checklist,
not applied.

| # | Location | Finding | Tag | mirror-pool-specific note |
|---|---|---|---|---|
| 1 | `crates/sdk/src/lib.rs:426` | Doc comment on `stake_account_pda` cites `programs/pool-program/src/round.rs::execute_round`; `execute_round` actually lives in `lib.rs:259` (`round.rs` only holds `RoundState`/`Round`/`ActionKind`/`Intent`). | **Real issue**, trivial severity | `stake_account_pda` is the address derivation for mirror-pool's *second* `PooledAction` adapter (genuine native-stake delegation) — a contributor chasing "where does stake dispatch actually happen" via this comment lands in the wrong file. Spot-re-checked while writing this doc (2026-07-17): **still present verbatim**, same line number. |
| 2 | root `Cargo.toml` | No `[workspace.dependencies]` despite 7 version strings duplicated across ≥2 members (`solana-sdk`, `litesvm`, `bincode`, `serde_json`, `ark-bn254`, `num-bigint`, `groth16-solana`). | **Real issue**, low severity | The `litesvm` duplication is the one worth prioritizing: CLAUDE.md mandates LiteSVM as the primary instruction-test tool specifically to avoid `solana-test-validator` flakiness — two crates silently resolving different `litesvm` versions in the lockfile would mean "the same test strategy" isn't actually exercising one consistent in-VM runtime. |
| 3 | repo root | No `README.md` — present in all three comparison repos (Squads v4, Token-2022, Light Protocol). | Nitpick (pre-submission item) | Not a CLAUDE.md violation (nothing mandates one) and plausibly deliberate mid-build. Worth having once, right before the Superteam Brazil bounty submission — a reviewer's first click — not mid-development. |
| 4 | `crates/vk-gen/src/main.rs:30-32,41-42` vs. `:29,37,39,71` | Bare `.unwrap()` mixed with `.expect("msg")` for equivalent JSON-parsing failures in the same functions. | Nitpick | `vk-gen` is build-time codegen from a developer-controlled `verification_key.json` (feeds the machine-generated `vk.rs`), not attacker input — within the Rust Book's own tooling exception (Part 1.B, row 1). The inconsistency just undercuts the message-per-panic discipline the rest of the repo holds uniformly. |
| 5 | `programs/pool-program/src/lib.rs` | 696 lines, largest pure-production file in the repo, zero inline tests, no internal file seams (5 handlers + their `Accounts` contexts + 1 event + a 24-variant error enum). | Nitpick / watch-item | Not a current CLAUDE.md violation — each handler already delegates logic to `action.rs`/`invariants.rs`/`merkle.rs`; this is a vertical slice, not a "by layer" bucket. If it keeps growing, the precedented move is Squads v4's one-file-per-instruction (Part 1.C). Flagged as a watch-item only, per this task's own instruction not to recommend restructuring preemptively. |
| 6 | `crates/sdk/src/lib.rs` | 725 lines total; ~525 lines of production code before its own `#[cfg(test)]` block — second-largest file in the repo. | Nitpick / watch-item, lower priority | Same framing as #5, lower priority since this is client tooling (`Note`, `MerkleTree`, instruction builders), not the on-chain custody surface. |
| 7 | `crates/sdk/src/lib.rs:230-236` | `MerkleTree::insert` defers in-field validation to a documented panic in `root()`/`authentication_path()` rather than validating eagerly via `Result`. | Nitpick | Host/client-side code (proving is client-side per CLAUDE.md's privacy invariants), not on-chain attacker surface — the fail-closed rule (Part 1.B, row 1) is scoped there. The deferred-panic behavior is already documented, which is what Rust API Guidelines C-FAILURE (Part 1.A) actually requires; eager validation would be stylistically nicer but this isn't a standards violation. |

**Not found** (per the source audit — not independently re-verified for this synthesis):
banner/divider comments, `TODO`/`FIXME`/`XXX`/`HACK` markers, commented-out code,
unjustified `#[allow(...)]`, accidentally-tracked build artifacts or ZK binaries, or any
`unwrap`/`expect`/`panic!` on attacker-controlled input in the on-chain program.

---

## Part 3 — What we already do well

CLAUDE.md already mandates two of Part 1's cited rules almost verbatim — "Comment **why**,
never **what**" (§ Comments — signal only) and "Small, focused files... split it by
responsibility (not by layer)" (§ Code style). The audit found real adherence, not just a
policy on paper:

- **Zero banner comments, zero `TODO`/`FIXME`/`XXX`/`HACK` markers, zero commented-out
  code** across `programs/pool-program/src` and `crates/*/src` — checked by grep, not
  spot-reading. Nearly every comment present explains rationale, a cross-module contract,
  or a rejected alternative (Part 1.A):
  - `lib.rs:253-258` — why `execute_round` needs a named `'info` lifetime, citing the
    specific `AccountInfo<'info>` invariance rule that forces it.
  - `lib.rs:283-293` — why `execute_round` has no explicit `round.state`/`round_id`
    re-check, by walking through which Anchor account constraints make it unreachable —
    CLAUDE.md's "no dead code" rule enacted by *explaining* an apparently-missing guard.
  - `action.rs:110-120` — why `StakeAction::execute` must normalize a pre-funded stake
    PDA to an exact balance in both directions, tracing the griefing vector (public PDA
    seed chain → pre-fundable address) *and* the privacy consequence (a non-uniform
    delegation amount would be distinguishable on-chain) — this is the k-anonymity-over-
    actions thesis showing up directly in a comment.
  - `state.rs:5-14` — justifies `zero_copy` + `repr(C)` with the concrete compiler error
    it avoids (`E0793`) and the ~7-8 KB stack frame it prevents — the reasoning behind
    CLAUDE.md's "Box large accounts, never copy onto the 4 KB SBF stack" rule, stated inline.
  - `invariants.rs:53-58` — `TIMEOUT_SLOTS`'s comment states its own epistemic status
    honestly ("a workload-contingent judgment call, not a derived number... kept a const
    here to avoid unused config surface") — YAGNI (Part 1.B) applied *and* narrated, on
    this branch's own feature.
  - `crates/ext-data/src/lib.rs:30-36` — why the modular reduction is a bounded loop, not
    a single conditional subtract, with the concrete bound (~5.3r) and the DoS
    implication — `ext_data_hash` is what binds recipient/relayer into the ZK proof, so
    this comment is documenting a fund-drain-prevention invariant, not a style choice.
  - `crates/sdk/src/lib.rs:58-79` — `Note::new()` uses `.expect(...)` with the invariant
    justified inline; the sibling `Note::from_parts()` (untrusted input) returns `Result`
    instead — the Rust Book's Ch. 9.3 pattern (Part 1.B) applied and *distinguished by
    trust boundary*, correctly.
  - Every crate's `Cargo.toml` carries a why-comment per dependency (version-pin reason,
    feature-flag reason, or "required directly because X"); `deny.toml`'s advisory
    `ignore` list gives a one-line reachability justification per RUSTSEC ID rather than
    a blanket suppression (Part 1.B's supply-chain citations, operationalized).

- **Fail-closed, fully, on the attacker-facing surface**: zero `unwrap()`/`expect()`/
  `panic!()`/`todo!()`/`unimplemented!()` in `programs/pool-program/src` outside
  `#[cfg(test)]`. Every such call elsewhere in the repo lives in host-side/build-time
  tooling over developer-controlled input — the Rust Book's own documented exception
  (Part 1.B, row 1).

- **All 4 `#[allow(...)]` in the repo are justified**: 3× `#[allow(clippy::
  too_many_arguments)]` on wide-but-necessary instruction-builder signatures, 1×
  `#[allow(deprecated)]` (`crates/sdk/src/lib.rs:448`) with an inline comment naming
  exactly which deprecated item and why it's still required (`stake::config::ID` still
  mandatory in `DelegateStake`'s CPI) — CLAUDE.md's "no `#[allow(...)]` to silence real
  warnings" rule, literally met.

- **CI enacts a rule as a regression test, not just a policy**: `.github/workflows/
  ci.yml`'s custom grep-based guard step exists specifically to prevent reintroducing the
  whole-`Pool`-copy stack bug — a live, automated check for exactly the "Box, mutate in
  place" custody rule above. CI also pins third-party GitHub Actions to commit SHAs (not
  floating tags) and sets `permissions: contents: read` at the top level with a stated
  least-privilege rationale.

- **Repo-root and workspace hygiene match Part 1.D and 1.C's citations without being
  told to**: root `Cargo.toml` is a correct virtual manifest; nothing build/scratch
  (`.anchor/`, `circuits/build/`, `circuits/node_modules/`, `target/`) is actually
  tracked; the `programs/` + `crates/` split mirrors Light Protocol's real top-level
  workspace shape — a precedent from a comparable Solana privacy protocol, not an
  idiosyncratic choice.

---

## Limitations

- **Part 2 is not tool-verified.** No `cargo build`/`test`/`clippy`/`fmt` was run for the
  underlying audit — deliberately, to avoid contending with a concurrent compile of
  `programs/` and `crates/`. Formatting/lint cleanliness there is visual-inspection only.
- **The audit is already stale, provably.** By the time this synthesis doc was written,
  `feat/timeout-cancel` (the branch Part 2 was captured on) had already merged to `main`
  (`53f9c08`), with two more commits (`b84f3b0`, `f747a5c`) landing after that merge —
  three commits past the snapshot before this checklist even existed. Only two items were
  manually spot-re-checked while writing this doc: punch-list #1's stale cross-reference
  (confirmed still present, same line, `crates/sdk/src/lib.rs:426`) and the two line
  counts in #5/#6 (`lib.rs` 696, `sdk/src/lib.rs` 725 — both still match). Nothing else in
  Part 2 — the dependency-duplication grep, the "zero TODO/banner/dead-code" claims, the
  `.unwrap()`/`.expect()` inconsistency in `vk-gen`, item #7's `MerkleTree::insert`
  behavior — was re-run for this doc; all of it is carried forward from the CODE-CRAFT
  strand as given, per this task's synthesis (not re-audit) scope.
- **Two Part 1 citations rely on secondary sourcing, flagged inline at point of use**: the
  two Ousterhout rows (chapter/section titles corroborated via independent secondary
  summaries, not a direct fetch of the primary chapter text) and the YAGNI row (fetched
  via a Wikipedia aggregator, not the primary 2000 XP Installed text). The CrateDepression
  citation is a secondary security-vendor write-up corroborated by one independent
  advisory, not a primary incident disclosure. The RareSkills citation is a developer
  blog, included only because it independently converges with primary-source repos.
  None of these are claimed as more certain than that.
- **Part 2 is now actionable.** Plan 6a has merged (`53f9c08`) and the tree is clean, so
  this is a normal cleanup pass — not a mid-branch hazard. The one discipline that still
  applies: re-verify each `file:line` against `HEAD` before editing, since the snapshot
  line numbers predate the merge and are approximate, not exact.
