# CLAUDE.md — mirror-pool engineering conventions

mirror-pool is a **Rust-only Solana privacy protocol** — a crowd-sourced *behavioral*
anonymity set (unlinkable synchronized on-chain actions). It custodies user funds and
makes privacy guarantees, so correctness and clarity beat cleverness everywhere.

**Read before coding:** design spec → `docs/superpowers/specs/2026-07-15-mirror-pool-design.md`;
prior-art → `docs/research/prior-art.md`; CI/test strategy → `docs/research/cicd-and-testing.md`.
Active plans live in `docs/superpowers/plans/`.

## Workflow

- **Spec → plan → TDD.** Non-trivial work gets a design in `specs/` and a plan in
  `plans/` before code. Write the failing test first; implement the minimum to pass.
- **Commit small and often**, one logical change per commit. **Conventional commits**
  (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`) — they feed release automation.
- Don't broaden scope mid-task. If you spot unrelated cleanup, note it; don't do it here.

## Code style

- **Rust 2021, Anchor 0.31.x.** `cargo fmt` + `cargo clippy --all-targets -- -D warnings`
  must be clean before every commit. `overflow-checks` stays on.
- **Small, focused files.** One clear responsibility per module. If a file is getting
  hard to hold in your head, split it by responsibility (not by layer).
- **Match the surrounding code** — naming, structure, idioms. Consistency > personal taste.
- **No dead code, no commented-out code, no `#[allow(...)]` to silence real warnings.**
  Delete it; git remembers.

## Comments — signal only

- Comment **why**, never **what**. The code already says what it does.
  - Bad: `// increment the counter` above `counter += 1;`
  - Good: `// nullifier PDA existing == spent; init fails atomically on a double-spend`
- No narration, no section-divider banners, no restating the function name in a doc comment.
- A public API gets a doc comment **only when** the name+signature aren't self-explanatory
  (invariants, units, panics, non-obvious constraints). Otherwise leave it clean.
- Prefer a well-named function or variable over a comment explaining a murky one.

## No overengineering (YAGNI)

- Build for the current plan, not an imagined future. No config, generics, traits, or
  indirection until a second concrete caller needs them.
- **The one sanctioned extension seam is the `PooledAction` trait** (adding a protocol =
  one adapter). Don't invent other abstraction layers "to be flexible."
- Before adding a dependency, a trait, or a layer: justify why the simpler version fails.
  Fewer moving parts is a feature in a custody protocol.

## Correctness & custody safety (this code holds funds)

- **Fail closed.** No silent `catch`/`unwrap_or` that swallows an error into a fallback.
  On-chain paths return typed program errors; no `unwrap()`/`expect()`/`panic!` on
  attacker-influenced input.
- Validate inputs at the boundary (field-element range, amounts, account ownership/seeds).
- Use checked/`require!`-guarded arithmetic; never assume an amount or index is in range.
- Put invariant logic (Merkle/ring/nullifier/k-floor/value-conservation) in **pure
  `pub fn`s** with host unit tests — it's the security-critical code *and* the only code
  `cargo-llvm-cov` can truthfully measure (SBF in-VM lines aren't counted; see CI doc §3).
- Large accounts are `Box`ed and mutated **in place** — never copy a multi-KB struct onto
  the 4 KB SBF stack.

## Privacy invariants (don't undo the whole point)

- **Never log, emit, or return secrets** — note secrets, nullifier preimages, or the
  member→action mapping. Events carry only already-public data.
- **Proving is client-side.** The coordinator/relayer must never receive anything that
  lets it deanonymize a participant.
- The **`k`-floor is enforced on-chain**, not just in the coordinator. Keep action shapes
  standardized (bucketed amounts) — arbitrary values fingerprint users.

## Testing

- **TDD, always.** `proptest` for invariants; **LiteSVM** (Rust) for instruction tests —
  avoid `solana-test-validator` in the inner loop (flaky). No TypeScript.
- Tests must assert real behavior, not tautologies. Adversarial/negative cases are
  mandatory for anything security-relevant (forged proof, replay, sub-`k`, caps).
- A change isn't done until `cargo test -p <crate>` is green and you've said so with the
  output — no "should pass".
