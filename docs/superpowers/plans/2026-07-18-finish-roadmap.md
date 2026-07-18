# mirror-pool — finish roadmap (bounty submission)

> **This is a coordination index, not an implementation plan.** Each work item below gets its own
> `brainstorm → spec → plan → TDD` cycle, and each spec / plan / merged branch is gated by an
> **independent review** (the fork — same gate that cleared Pool.fee, timeout-cancel, effective-k).
> Purpose: keep the implementation lane and the review lane on one picture of *what "finished" means*,
> *what's in scope*, and *the honesty guard on each item*.

**Date:** 2026-07-18 · **Supersedes** any "we're mid-phase-6 with a mountain left" framing.

---

## What "finished" means here

A bounty submission wins on two things, and every item serves one:

1. **The core claim is airtight and honest** — behavioral k-anonymity via the uniform actor, measured
   honestly (effective-k), with residuals disclosed not hidden.
2. **A judge can independently verify it** — a live run they can reproduce, and an empirical
   demonstration that it *actually hides*.

The differentiator (phases 1–3 of the design spec) is **built**. What remains is verification,
two hardening fixes, two scoped additions, and drawing an honest future-work boundary.

---

## Scope decisions (2026-07-18)

**IN — pull into the bounty:**

| Item | Why it's in |
|---|---|
| Round-engine hardening (MAX_K cap) | Closes a latent fund-stranding bug. (Canonical ordering was dropped at the F1 spec gate — not an anonymity gap; see spec §5 + `14e6d49`) |
| SOAK + proof doc | The live, reproducible run — the verification tier LiteSVM can't reach |
| §6.5 adversarial simulation | The empirical "it actually hides" proof the spec promised — and an honest contrast to a rigged harness |
| Opt-in viewing-key disclosure | Serves the prior-art lesson (every survivor bolts on selective disclosure); a real distinguisher |
| Fuzzing pass | Cheap robustness signal that the fail-closed paths hold under garbage input |
| 6c `round_executable_slot` | The last mechanism-research item (anti-trickle timing) |

**STAY DEFERRED — and here the *research*, not just the old spec, says so:**

- **Bonding / incentive module** — our mechanism research concluded it is "a price, not a proof"
  (a well-capitalized adversary is unaffected; it does not deepen distinct-human `k`). Building it would
  push us toward claiming *economic Sybil resistance* — the exact overclaim we have deliberately avoided.
  The reserved `["member",pool]` PDA + the honest "priced, not solved" disclosure is the stronger posture.
- **Swap adapter** — the execution-envelope research shows a Jupiter CPI blows the 64-account lock at
  very low `k`. Architecturally blocked in the single-tx model, not a free choice.
- **Production hardening** (Squads multisig, time-locks/caps, external audit, multi-party trusted-setup
  ceremony) — mainnet-launch scope, not bounty scope; a solo MPC ceremony isn't meaningfully buildable.
  Honest disclosure (dev-only setup) is the right bounty posture.
- **Thin coordinator / participant CLI** — considered, not selected; the soak binary covers "a judge can
  run it." (If revisited: keep it liveness-only — this is exactly where the competitor leaks initiators.)
- **Indexer · scale tests · coordinator decentralization** — low bounty value; SDK reads chain directly,
  and MAX_K bounds a round to the low tens so "thousands per round" is moot.

**Already shipped under a different name:** `emergency_withdraw` → `cancel_intent` (the
coordinator-independent, timeout-gated reclaim). Not a gap.

---

## Sequencing reconciliation (2026-07-18) — checked against the repo, not assumed

Both lanes independently agree on the leverage lens (verifiability > more mechanism > CI optics >
hygiene; the soak is the top prize). Reconciling the deltas, with the concrete claims verified via
`git`/`gh`:

- **CI is GREEN, not red.** The bincode / RUSTSEC-2025-0141 ignore (`71a0630`) is already on `origin`
  (ancestor of `origin/main` = `7ae70cb`), and the latest CI run on `origin/main` is `success`. There is
  **no "push to fix a red CI" action**. What's unpushed is **8 docs-only commits** (this research + README
  polish + this roadmap) — zero code, zero CI impact.
- **"Phase 0" is a *decision*, not a fix — push the docs?** Pushing makes the honest, Smith-anchored
  README and the research corpus visible to a browsing judge (real *presentation* value, no code risk).
  It is the user's explicit-yes gate; recommendation is a clear yes, but never auto-done.
- **The soak does NOT wait on F1 at all** (updated 2026-07-18): canonical ordering — the only
  interface coupling — was dropped at the F1 spec gate (not an anonymity gap; spec §5, `14e6d49`).
  **MAX_K** changes no interface and doesn't affect a small-`k` demo — the soak and F1 are fully
  parallel tracks.
- **Soak runtime = `solana-test-validator`, honestly scoped** (the building lane's call, and the more
  honest one): universally reproducible; the stake round delegates to a *self-created* vote account — a
  genuine stake-program round, framed as exactly that in the proof doc, **not** "a real mainnet validator."
- **Implementation rides feature branches, not `main`** (as every prior plan did) — the two lanes share
  one repo; branch → whole-branch review → merge keeps histories clean.

## Sequence (implementation lane runs these in order; ★ = parallelizable anytime)

1. **F1 — Round-engine hardening.** The `MAX_K` cap (`task_b3a08dd7`) only — canonical batch
   ordering was dropped at the spec gate (spec §5). No longer blocks the soak; F2a may run in
   parallel.
2. **F2a — SOAK + proof doc.** The other Claude is already brainstorming this. Capstone live proof.
3. **F2b — §6.5 adversarial simulation.** The empirical privacy proof. Host-side, no surfpool dependency.
4. **F3 — Opt-in viewing-key disclosure.** Independent feature (SDK-side); could also run ★-parallel.
5. **6c — `round_executable_slot`.** On-chain (touches `Round` layout) → whole-branch review earns the
   strongest model.
6. ★ **Fuzzing pass** — independent; slot in whenever.
7. **Docs — the future-work boundary + submission polish** (see below).

*Swap the F2a/F2b order if you prefer the empirical proof before the live one — they're independent
deliverables. F1 stays first.*

---

## Per-item notes + the honesty guard on each

**F1 · Round-engine hardening** — `commit_intent` has no upper bound on `intent_count`, so a round can
grow permanently unexecutable → funds exit only via the linkable cancel path. Add
`require!(intent_count <= MAX_K)` after the increment at commit + `require!(k_floor <= MAX_K)` at
init, **action-kind-aware**, `MAX_K` **pinned by measurement** (compiled-tx key-count guard for the
lock dimension + LiteSVM CU sweep — don't hard-code the ~17/19 estimate). Canonical ordering was
**dropped at the spec gate** (not an anonymity gap — the intent→recipient linkage is already public
per the never-closed Intent PDA; spec §5, research corrected in `14e6d49`). See
`docs/research/solana-execution-limits.md`.

**F2a · SOAK** — runtime `solana-test-validator` (universally reproducible, no surfpool dependency): the
withdraw uniform-actor round + live effective-k, plus a stake round that delegates to a *self-created*
vote account — a genuine stake-program round, framed as exactly that, **not** "a real mainnet validator."
**Guard:** assert the headline — *zero participant signatures* — by reading the **actual landed-tx signer
set**, not by trusting code. Assert only independently-checkable on-chain facts (value conservation,
byte-uniform payouts, canonical order). The proof doc is a real run log with lookup-able tx signatures —
**not** a self-generated narrative. Don't claim fork-delegation touches mainnet.

**F2b · §6.5 adversarial sim** — the differentiator. **Guard (this is the whole point): it must be
adversarial against *ourselves*, not a demo of the happy path.** Run the heuristics that still bite
(cross-round timing, funder-clustering — amount and common-input are already foreclosed by fixed-denom +
Pool.fee + the uniform actor) and report what actually happens across three regimes: distinct funders
(effective-k ≈ k, the mechanism works), whale self-fill (effective-k collapses — the disclosed residual),
and repeated participation (the Danezis multi-round decay). Assert on the *measured* effective-k and the
Danezis bound, never a hand-waved "≤ 1/k." A harness that shows the residuals is more credible than one
that hides them. Host-side, builds on `crates/effective-k`.

**F3 · Opt-in disclosure** — needs a real brainstorm (creative feature). **Guard:** opt-in and
*user-controlled only* — the user proves *their own* history to a party *they* choose; no global auditor,
no compel path, no backdoor (design spec §8 non-goal). This is the honest version of "compliance."

**6c · `round_executable_slot`** — timing slice (Threshold-AND-Timed mix) raising the (n−1) isolation
attack's cost/latency; **guard:** it does *not* make the attack uncertain (that needs a true pool mix) —
keep the claim scoped. New pure fn (the `cancel_unlock_slot` mould) + `Round` +16B + a SlotHashes account.

**Fuzzing** — `cargo-fuzz` on instruction data / proof bytes / intent payloads; assert fail-closed, no
panics on attacker-influenced input.

**Docs — the future-work boundary** — add a clear "future work (v1 → production)" section (README +
spec) listing the STAY-DEFERRED items *as deliberate scope with reasons*, so a literate judge reads them
as decisions, not gaps. Also pending: retire-vs-keep `behavioral-rounds-followup-proposal.md` (user call).

---

## Lanes & gate

- **Implementation lane** (the building session): owns F1–F3, 6c, fuzzing — each `spec → plan → TDD`,
  subagent-driven, `cargo fmt` + `clippy -D warnings` + `cargo test` green before "done."
- **Review / research lane** (the fork): reviews every spec, plan, and merged branch; holds the
  honesty ceiling; research is **complete** (no further passes). 
- **Not pushed without the user's explicit "yes push."**
