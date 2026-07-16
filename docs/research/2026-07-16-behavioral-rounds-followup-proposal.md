---
title: "Behavioral rounds — follow-up systems proposal (from k-candidates to k-anonymity)"
date: 2026-07-16
status: THINKING MATERIAL — a proposal for consideration; does NOT edit Plan 4, the spec, or shipped code
companion_to: 2026-07-16-behavioral-privacy-industry-practices.md
method: 6 deepening research fronts (Sybil-pricing, anonymity harness, funding-topology, coordinator trust, disclosure, cross-round intersection), each fact-checked; cross-related with the code audit + research-1; adversarially critiqued for YAGNI / feasibility / bounty-fit
caveat: >-
  Produced by a parallel research pass while Plan 4 was in flight. Every item is an option, not a plan change.
  ⚠️ READ THE ADVERSARIAL CRITIC REVIEW (near the end) BEFORE ACTING — it materially reprioritizes this proposal
  (its sharpest point: build the second pooled ACTION, not a sixth privacy layer on the withdraw exit) and flags
  real feasibility holes (on-chain distinct-funder counting is unenforceable; a "chunked atomic" executor is a
  contradiction; the ~19-account ceiling is withdraw-only). Research agents could not open the repo under the
  sandbox lock, so line-number claims should be reconfirmed against the tree before acting.
---

# mirror-pool — Follow-up Systems Proposal: from *k-candidates* to *k-anonymity*

> **Status: THINKING MATERIAL — a proposal for consideration.** It does **not** edit
> Plan 4, the design spec, or shipped code. Every item is an option/recommendation.
>
> **⚠️ RECONCILED against `main` @ `2882505` (2026-07-16) by a code-grounded verification pass.**
> This doc was written *before* Plan 4 merged, blind to the repo (a macOS filesystem block), so its
> code-grounding was a pre-merge snapshot. Corrections that supersede the body below:
> - **Plan 4 is MERGED, not "in flight." Task 6 is DONE — the standalone `withdraw` instruction is
>   REMOVED** (gone from `lib.rs`; its test deleted; `build_withdraw_ix` gone from the SDK). Any line
>   below reading "withdraw still present / Task 6 not done / k=1 exit awaiting removal" is stale.
> - The remaining single-note (sub-k) exit is **`cancel_intent`** — **permanent by design and explicitly
>   disclosed** (plan Self-Review + the `cancel_intent` doc comment + commit `1a78cd4`). It is a
>   documented safety-valve tradeoff to *mitigate* (e.g. timeout-gating), **not** a Task-6-style loose end.
> - **Inline `lib.rs:NNN` line numbers are the pre-merge snapshot and have shifted** (the file is 549
>   lines now; e.g. `lib.rs:567` no longer exists). Current anchors: k-floor gate = `execute_round`'s
>   `meets_k_floor(count, k_floor)` (~`lib.rs:244`); `cancel_intent` (~`lib.rs:170`); `ExecuteRound`
>   accounts (~`lib.rs:464`); `deposit`'s `amount == denomination` is unchanged at `lib.rs:75`.
> - **The anonymity-test mandate is spec §6 _item 5_, not "§6.5"** (the spec has no §6.5).
> - **Everything substantive verified CORRECT** and thus load-bearing: the k-floor gates on raw
>   `intent_count` (not distinct funders) — whale-self-fill collapse is real; `cancel_intent` refunds the
>   full denomination with **zero** bond/forfeit; the `6 fixed + 3-per-intent` account shape ⇒ a **~19
>   intents/tx** ceiling; **k=2 = 26,247 CU** (re-measured live); no coordinator/harness crate exists;
>   single-denomination pool; and every spec citation. The core thesis ("k-floor buys k *candidates*, not
>   k-anonymity; withdraw-MVP is a mixer until effective-k is real") **holds against the merged code** —
>   only the "transitional / awaiting-Task-6" framing is wrong; the gap is the system's current, intentional state.

---

## 1. The through-line: what audit + research-1 + this follow-up say *together*

Three studies, one story:

- **(A) The audit of the shipped spine + Plan 4** shows a working single-denomination
  Groth16/Poseidon shielded pool (deposit → prove → withdraw), and an in-flight
  behavioral layer that batches intents under an **on-chain k-floor**. But the audit's
  between-the-lines finding is concrete in the code: `execute_round` counts
  `round.intent_count` against `k_floor` (`programs/pool-program/src/lib.rs:305`), and an
  *intent* is nothing more than a spent note plus recorded payout keys. Nothing on-chain
  ties an intent to a distinct person or funding source.
- **(B) research-1** ([`docs/research/prior-art.md`](docs/research/prior-art.md)) established
  that behavioral pooling is genuine white space, that **timing correlation is the dominant
  attack** (Tornado FIFO temporal matching reached 34.7% of withdrawals), and that mirror-pool
  must *invent* three things: set-membership over actions, timing-correlation defeat, and the
  coordinator incentive/anti-Sybil model. Its load-bearing sharpening: **the k-floor buys k
  candidates, not k-anonymity.**
- **(D) this follow-up** supplies the exact, primary-sourced constructions for each gap.

**The insight that orders everything:** the shipped MVP is a *funds mixer in a behavioral
jacket* until three layers land on top of Plan 4's k-floor — a **harness** that measures
effective-k, **Sybil-pricing** that makes the k real, and **funding/coordinator hygiene**
that closes the two residual attribution surfaces (pre-round funding topology; off-chain
coordinator metadata) the uniform-actor batch provably cannot fix. The whale who self-fills
k-1 of k intents ([`lib.rs:305`](programs/pool-program/src/lib.rs)) is not a hypothetical —
it is the default the current gate permits. **Withdraw-as-a-PooledAction becomes on-mission
only when those three layers exist**; adding more PooledAction adapters before then just
multiplies an un-private base.

---

## 2. Where the code actually is (so this is grounded, not a paper)

| Fact (verified in tree) | Location | Consequence |
|---|---|---|
| k-floor gates on `intent_count`, not distinct funders | `lib.rs:305`, `invariants::meets_k_floor` | k = candidates; whale self-fill collapses effective-k to 1 |
| `cancel_intent` refunds the **full** denomination, no bond/forfeit | `lib.rs:231-273` | Sybil-pricing costs ~0 today (rent + gas only) |
| Single-tx batch: 6 fixed accounts + program + 3/intent vs Solana's 64 account-lock cap | `ExecuteRound`, `lib.rs:567` | **k ≲ 18-19 ceiling**; no chunking. CU is *not* binding (k=2 = 26,247 CU) |
| No `coordinator` crate, no `harness` crate | `crates/` = sdk, prover, ext-data, vk-gen, parity-fixtures | Harness + coordinator metadata work are **greenfield, pure host-side Rust** |
| Opt-in viewing-key disclosure promised | spec §2, §4 (⑧), §8 | Unbuilt; the cheapest construction needs **no new circuit** |
| Single-denomination pool | `Pool.denomination`, `deposit` (`lib.rs:75`) | Amount-bucketing partly satisfied already — a real head start |
| Task 6 DONE — standalone `withdraw` REMOVED (verified: absent from `lib.rs`) | `execute_round`/`cancel_intent` (see header) | The k=1 linkable *withdraw* exit is closed; `cancel_intent` remains as a **disclosed, permanent** single-note safety valve to mitigate (e.g. timeout-gate), not remove |

The harness (spec §6 item 5) and coordinator (spec §3.2) are the two spec components with **zero
lines shipped** — and they are precisely the two that carry the mission and carry no custody
risk. That is the opportunity.

---

## 3. Prioritized initiatives

Ranked by (bounty impact × safety × cost × dependency). Ruthlessly YAGNI: the **only**
sanctioned extension seam is `PooledAction`; everything below layers onto existing
instructions as `require!` guards + bond PDAs + pure invariant `fn`s, or lives in new
host-side crates — **no new abstraction layers.**

### #1 — Effective-k anonymity harness (host-side crate) · effort M · **independent of Plan 4**
**What.** A new `crates/anonymity-harness` that models a round as an intent→execution graph,
runs a simulated adversary, and **fails the build** when worst-case re-identification exceeds
1/k. **Primary gate = min-entropy / Bayes vulnerability** `V = max_i p_i <= 1/k_floor`
(equivalently residual min-entropy `>= log2(k)`), NOT Shannon entropy — a distribution can
have high Shannon entropy yet be guessed in one try, which is exactly the whale-self-fill
collapse ([Smith QIF](https://www.lix.polytechnique.fr/~catuscia/papers/QIF/gleakage.pdf),
canonical). Drive it with **`proptest` + counterexample shrinking**, persisting the failing
seed to `proptest-regressions/` so a probabilistic privacy claim becomes a deterministic CI
failure ([proptest docs](https://proptest-rs.github.io/proptest/proptest/failure-persistence.html),
production). The simulated adversary ports **empirically-validated heuristics**: Tutela's
address/gas/multi-denom/funding linkage (42.8k of 97.3k Tornado deposits compromised, ~37%
set reduction — [arXiv 2201.06811](https://arxiv.org/abs/2201.06811), mainnet-measured) and
Railgun's knapsack amount-fingerprint (3.42-bit median loss —
[arXiv 2606.25926](https://arxiv.org/abs/2606.25926)); the harness must **assert** the
uniform vault-signed batch nulls the gas/FIFO-at-execution channels, then verify the surviving
channels operate only on pre-round funding + intent-submission observables. Add a **cross-round**
check: the possinymity intersection `P_N` over all rounds an identity acted, and Danezis's
computable rounds-to-deanonymize bound for a repeat participant
([Buddies, arXiv 1305.5236](https://arxiv.org/pdf/1305.5236);
[Danezis SDA](https://www.freehaven.net/doc/e2e-traffic/e2e-traffic.pdf), research). Emit
Shannon `H`, `2^H`, and Diaz `d` as **telemetry only** — never the gate (an average can mask
one near-deanonymized victim).

**Why #1.** It is the mission's proof-of-work and the yardstick for #2–#6; it is the exact
"anonymity test (differentiator)" the spec already mandates (§6 item 5); it is pure `pub fn` +
proptest — the one place `cargo-llvm-cov` can *truthfully* measure the security-critical code
(CLAUDE.md); and it has **no custody surface**. Honest maturity: the metric and the attacker
heuristics are canonical/mainnet-validated; **packaging them as a proptest anonymity CI gate
is novel — that novelty is mirror-pool's contribution.**
**Bounty axis.** *Impact* (proves the pool defeats clustering) + *Quality* (host-tested,
llvm-cov-truthful, fail-closed CI).

### #2 — Funding-topology + timing hygiene in the SDK · effort M · **independent of Plan 4 core**
**What.** Client-side funding pipeline that closes **residual surface #1 (pre-round funding
topology)** and attacks the **dominant timing attack**. Concretely: (a) per intent, a
**single-use funding keypair** whose first inbound SOL comes from an aged, non-hub intermediary
— on Solana the attribution key is literally "the first incoming SOL transfer"
([Helius funded-by](https://www.helius.dev/docs/wallet-api/funded-by), production; it ships a
"find wallet clusters" example), so the fresh funder inherits the intermediary's attribution,
not the user's; (b) **bucketed funding amounts** (fund to a fixed reserve, never `deposit+fee`)
— a value unique on-chain re-links the two ends even past a ZK break (Zcash: 249.9999→250.0001
ZEC, 28.5% of coins linked by value —
[benthamsgaze](https://www.benthamsgaze.org/2018/05/09/the-pools-run-dry-analyzing-anonymity-in-zcash/),
production evidence); (c) **Poisson-decorrelated commit timing** so round-join order is not a
sorted function of funding time — FIFO temporal matching is the highest-yield heuristic, and a
1-day window alone collapsed a ~400-member Tornado set to ~12
([Béres, arXiv 2005.14051](https://arxiv.org/pdf/2005.14051)); the memoryless exponential delay
is the [Loopix](https://arxiv.org/abs/1703.00536) construction (research); (d) hard rules:
never fund from a CEX-withdrawal address, quarantine dust, no peel-chain balance-building.

**Why #2.** Timing is *the* number research-1 flagged, and this is the layer that actually
moves it — at **zero custody risk and no on-chain change**. It is also the acceptance target
the harness (#1) measures against. Note the recursion limit honestly: a whale who launders each
funder through independent paths still passes, which is why #3 (pricing) is needed alongside.
**Bounty axis.** *Impact* (defeats the dominant attack; closes the surface the uniform actor
cannot) + *Quality* (SDK correctness).

### #3 — Sybil-priced intent bond + distinct-funding counting · effort L · **touches Plan 4**
**What.** Make the k-floor count *cost something to fake*. (a) A **forfeitable per-intent bond**
`B` escrowed in the intent PDA, slashed on abort, plus a non-refundable fee `f`; count only
**bonded** intents toward k. An attacker owning fraction x of a round then posts ~x·k bonds
(cost linear in round size) while an honest user posts one — Xim's result that "a Sybil
attacker's costs grow linearly... while honest participants' costs remain small, fixed, and
constant" ([Xim / Sybil-Resistant Mixing, WPES'14](https://people.cs.umass.edu/~gbiss/mixing.pdf),
research). Size `f` so `(k-1)·f >= V`, the whale's front-run/copy-trade payoff. This layers onto
`commit_intent`/`cancel_intent`/`execute_round` as guards + a bond PDA + host-tested pure
`fn`s — **no new seam.** (b) A **cheap on-chain distinct-funder approximation**: hash-commit
each intent's funded-by root and enforce round-local uniqueness — a whale self-filling from one
source (Trusta's #1 Sybil pattern, "addresses funded by the same source" —
[Trusta](https://github.com/TrustaLabs/Airdrop-Sybil-Identification), production-adjacent) then
provably cannot fill k slots; full cluster-resistance is delegated to the harness (#1), since a
whale can defeat the cheap check with per-mask fresh funders.

**Why #3.** This is the frame's deliverable #1 and the direct antidote to the collapse visible
at `lib.rs:305`. Ranked below #1/#2 because it carries a **custody surface** (escrow + slash
must be trustless and fail-closed) and needs the harness to prove it works. If/when the deferred
incentive module is built, its reward MUST be **concave in intents-per-source** or it becomes
the cheapest Sybil subsidy (the necessary-and-sufficient Sybil-proof condition —
[Cost of Sybils, arXiv 2301.12813](https://arxiv.org/abs/2301.12813), research).
**Bounty axis.** *Impact* (makes k real) + *Quality* (fail-closed bond math, host-tested).

### #4 — Coordinator-metadata hygiene: harden the escape hatch + commit-reveal + Tor transport · effort M · **touches Plan 4**
**What.** Close **residual surface #2 (off-chain coordinator metadata)** with the cheapest,
most-self-contained levers, in order: (a) **Harden the self-submit escape hatch** already seeded
by `cancel_intent` into a permissionless timeout-gated `force_close_round` — the dominant rollup
pattern (self-sequence is ~73% of surveyed designs —
[arXiv 2503.23986](https://arxiv.org/html/2503.23986v1), production; Tornado's universal
self-relay), bounding the coordinator's power to sit on a k-satisfied round; (b) **commit-reveal
intent submission** so the coordinator counts commitments toward liveness but cannot read/reorder
content pre-close (CoW-style batch commit-reveal, production shape; formal order-fairness via
[Themis, eprint 2021/1465](https://eprint.iacr.org/2021/1465) is a later, heavier hardening —
research); (c) **transport blinding** via `arti`, the Tor Project's pure-Rust client that reached
its production bar at v1.0.0 ([arti](https://blog.torproject.org/arti_100_released/), production),
embeddable directly in the Rust SDK to strip the (submitter IP, timestamp) linkage.

**Why #4.** These only *constrain or blind* the existing liveness-only coordinator — no committee,
no new trust root, no custody change — so they are the right first move and honor YAGNI. Heavier
options (threshold-encrypted submission, MPC coordinator) are deferred (see §5).
**Bounty axis.** *Impact* (closes surface #2; censorship resistance) + *Quality* (trust-minimized,
permissionless, no custody added).

### #5 — Selective disclosure via per-intent payment disclosure · effort S · **independent of Plan 4**
**What.** Deliver the spec's "opt-in viewing-key disclosure, no backdoor" (§2, §8) with the
cheapest construction that fits mirror-pool's per-round intent model and needs **no new circuit**:
a **Zcash-style payment disclosure** — the owner reveals `(preimage, Merkle path, nullifier)` for
one intent, **bound to a verifier-chosen nonce** so it cannot be replayed, and the counterparty
re-derives the commitment/nullifier off-chain against the on-chain tree/nullifier state the pool
already publishes ([Zcash ZIP 311](https://zips.z.cash/zip-0311), proposed). Default state is
silence (mirrors OVK=⊥ and CLAUDE.md's never-emit-secrets invariant); no persistent key, no global
auditor. Gate disclosure on the round's *realized* k so a small-k round is not narrowed further.

**Why #5.** Ships a promised platform capability at S cost and zero backdoor risk; the persistent
viewing-key and association-set variants (§5) are strictly heavier and deferred.
**Bounty axis.** *Volume* (a delivered platform feature) + *Quality* (no-backdoor, fail-closed
default).

### #6 — Chunked round executor within the 64-account envelope · effort M · **touches Plan 4** · *gated by #1*
**What.** Today `execute_round` processes all intents in one tx (`lib.rs:567`); with 6 fixed
accounts + program + 3 per intent against Solana's 64 account-lock cap, that ceilings k at
**~18-19** (Address Lookup Tables raise the 1232-byte message-key limit but **not** the 64
account-lock cap). A chunked executor processes sub-ranges of `remaining_accounts` across multiple
txs with a per-round `executed_count` cursor, preserving atomicity/fail-closed semantics. This is
frame deliverable #4.

**Why #6 (and why last).** It is a real, computed constraint — not vaporware — but **YAGNI-gated**:
build it only if the harness (#1) sets a launch k that exceeds the single-tx ceiling. If the
minimum viable k is, say, 8-16, single-tx execution is fine and chunking is premature. CU is not
the binding limit (k=2 = 26,247 CU ⇒ ~19 intents ≈ 250k CU, far under 1.4M), so the account cap is
the sole ceiling.
**Bounty axis.** *Volume* (raises crowd-scale ceiling) + *Quality* (atomic across chunks). Indirect
*Impact* by enabling larger k.

---

## 4. Suggested sequence *after* Plan 4 merges

Plan 4 finishes Task 6 (remove standalone `withdraw`, SDK round builders, e2e). Then:

1. **Land the harness (#1) first.** It reads the merged core, establishes the CI gate, and
   computes the launch k from the SDA bound. Nothing later is measurable without it.
2. **SDK funding + timing hygiene (#2).** Re-run the harness to confirm the FIFO/gas channels go
   null and the funding surface shrinks.
3. **Sybil-priced bond + distinct-funder counting (#3).** Harness confirms self-fill now shows
   effective-k → 1 and the priced bond restores it; tune `f` against modeled `V`.
4. **Coordinator hygiene (#4).** Self-submit hardening → commit-reveal → arti transport.
5. **Payment-disclosure (#5)** — parallelizable with #4; low-risk, delivers the spec promise.
6. **Chunked executor (#6)** — only if the harness's chosen k exceeds ~19.
7. **Revisit the deferred register (§5)** only when a concrete second caller or threat appears.

---

## 5. Deferred / "watch — do NOT build now" register (YAGNI + anti-vaporware guardrail)

- **Concave-reward incentive module** — governs the *shape* of the deferred bonding/fee module;
  build only alongside #3, and only concave-in-intents-per-source, or it subsidizes Sybils
  ([2301.12813](https://arxiv.org/abs/2301.12813), research). Deferred per spec §7.
- **Association-set second Merkle root / Proof-of-Innocence** — a clean fit for the Poseidon/BN254
  stack ([0xbow Privacy Pools](https://github.com/0xbow-io/privacy-pools-core/), mainnet-alpha),
  but adds an ASP curation authority that can recreate the coordinator-metadata chokepoint; adopt
  only the distinct-source machinery, never the AML narrative, and only if #3's cheap check proves
  insufficient.
- **Threshold-encrypted submission / MPC coordinator** — Shutter is mainnet-**beta** with ~3-min
  inclusion latency; **Penumbra flow-encryption is unshipped** and **Arcium is mainnet-alpha** —
  **do not build against these.** They trade a coordinator-liveness problem for a committee-liveness
  problem and fail the YAGNI "second concrete caller" test. Prototype only after #1/#4 ship and only
  if coordinator liveness proves the dominant residual leak.
- **World ID personhood nullifier** — the only primitive that converts k-candidates to k-*persons*
  ([World ID](https://world.org/blog/engineering/iris-recognition-inference-system), production),
  but a centralized biometric issuer conflicts with the permissionless ethos; consider only as an
  optional high-assurance tier, never mandatory.
- **IVC-folded multi-hop source-of-funds** ([arXiv 2606.10172](https://arxiv.org/abs/2606.10172),
  research, weeks-old preprint) — needs a Nova/folding toolchain entirely separate from the
  Groth16/circom pipeline; watch, don't build.
- **Token-2022 Confidential Transfers** — disabled on mainnet since Jun 2025 (prior-art §7.3);
  not a dependency.

---

## 6. Open decisions (for the team)

- **Launch k + denomination buckets.** Compute k from the Danezis SDA rounds-to-deanonymize bound
  and the harness (spec §7 open). Ratio floor (`V<=1/k`) vs an **absolute** distinct-funding-root
  floor for low-liquidity rounds (sanctions-era Tornado inflow dropped >90%, shrinking sets).
- **Persistent cross-round identity or strictly per-intent?** A foundational fork: persistent
  identity enables full viewing keys but *reopens* the cross-round intersection attack; per-intent
  (payment-disclosure, #5) avoids it. Resolve before #5's richer variants.
- **Bond parameters `f`, `B`** as a fraction of the denomination so `(k-1)·f >= V` for realistic
  Solana front-run/copy-trade `V`. No deployed mixer publishes this — needs modeling.
- **Distinct-funding enforcement locus:** cheap on-chain hash-commit (whale beats it with fresh
  funders) vs harness-only (coordinator-recomputable, noisy — the multi-input heuristic is the most
  widely used but only ~0.36 precision at full-cluster level) vs in-circuit association buckets
  (needs a curation authority). Where does the recursion limit force the line?
- **Coordinator posture:** single liveness-only + self-submit + arti (cheap), or invest in
  commit-reveal/threshold? Gated by the **round-collection window latency budget** — a number the
  spec doesn't yet fix, and the thing that decides whether Shutter/Nym-class options are even viable.
- **Nullifier-derivation audit** (Penumbra nk-split lesson): confirm — as a pure `pub fn` with
  proptest — that no vault-signer/coordinator assembling the batch can precompute a user's *future*
  nullifier from public round data. Cheap, high-value, independent of everything above.
- **Cover traffic:** can the vault fund receiver-bound cover for *value-exiting* withdrawals without
  the dummy destination becoming a fingerprint? Easier for reversible future PooledActions
  (stake/vote) than for withdrawals.

---

## 7. Bounty-judging map (Impact / Quality / Volume)

- **Impact** (does it defeat clustering/copy-trading/front-running?): #1 proves it, #2 defeats the
  dominant timing attack, #3 defeats whale self-fill, #4 closes the coordinator surface.
- **Quality** (production-grade, fail-closed custody, test rigor): #1 (host-tested, llvm-cov-truthful
  CI gate), #3 (fail-closed escrow), #5 (no backdoor), #6 (atomic chunking) — all match CLAUDE.md's
  pure-fn-plus-proptest doctrine.
- **Volume** (breadth of the working platform): #5 and #6 expand the surface; adding new
  `PooledAction` adapters (Stake/Swap/Vote) is the sanctioned Volume lever — but it should wait until
  #1–#3 make the withdraw base actually private, or it multiplies an un-private base.

*The honest bottom line: withdraw-MVP is a fund mixer until #1, #2, and #3 exist together. That is
the shortest path from "shipped" to "on-mission."*

---

## Adversarial critic review

> Kept as a distinct section so you can weigh the proposal against its strongest challenge rather than having the dissent silently folded in.

**Verdict:** `aligned-with-caveats`

### Sharpest single recommendation

"Land ONE synchronized non-exit PooledAction through the sanctioned seam — cheapest first (a pooled vote or stake, because a Jupiter-swap CPI's account load collides with the 64-account lock cap; graduate to a pooled swap once the envelope allows) — paired with a MINIMAL self-fill effective-k harness that measures THAT action's k (drop the cross-round SDA/Buddies and multi-denomination knapsack machinery entirely). Why this is the one thing: the bounty mission is behavioral obscurity of ACTIONS — defeating copy-trading and front-running — and those are attacks on trades, not withdrawals. A withdraw-only pool, however hardened by six privacy layers, cannot demonstrate the mission's headline defense and reads as exactly the funds mixer that both the mission and spec §8 disclaim. Crucially, funding hygiene (#2) and the harness are action-agnostic, so they do NOT require withdraw to be perfected first — the proposal's sequencing (perfect the exit, then add actions) is not actually forced by any dependency, and its own through-line ('withdraw MVP is a mixer until effective-k is real') is the argument against it. Demonstrating one pooled trade plus its measured effective-k is the shortest path from 'shipped cryptographic spine' to 'on-mission behavioral obscurity,' and it is the sanctioned Volume lever besides. Build the second action, not the sixth layer on the exit."

### Bounty-alignment reality check

"CREDIT FIRST: this is a real audit, not a paper. Every load-bearing code claim verified in-tree (k-floor on round.intent_count at lib.rs:305; cancel_intent refunds full denomination with no bond at 255-265; standalone withdraw at 112; ExecuteRound at 567; crates = sdk/prover/ext-data/vk-gen/parity-fixtures, no coordinator/harness; single-denomination Pool). The cryptographic spine (Groth16 verify_withdraw) is shipped, so ZK-verify feasibility is proven. The through-line ('a mixer in a behavioral jacket until effective-k is real') is correct and matches spec §8 ('Not a funds mixer positioning'). Harness-first is genuinely spec-sanctioned — but it is §6 item 5 'Anonymity tests (differentiator)', not a literal '§6.5'; the spec even names the exact heuristics (FIFO/common-input/amount), so the harness core is well-grounded and its heuristic-porting is partly spec-directed. The deferred register is exemplary anti-vaporware discipline. IMPACT REALISM (the load-bearing gap): the mission as stated targets clustering + copy-trading + front-running. Copy-trading and front-running are attacks on active TRADES, not exits — you cannot copy-trade or front-run a withdrawal. None of #1-#6 demonstrate defeating them, because there is no pooled trade to obscure; the pooled swap/vote that would is parked in the deferred register. So the proposal perfects plausible-deniability + anti-clustering (1 of 3 named threats) while leaving 2 undemonstrated, and the harness measures WITHDRAW-unlinkability only. VOLUME REALISM: the sanctioned Volume lever is PooledAction adapters; the proposal defers ALL of them behind #1-#3, keeping the platform withdraw-only through six initiatives. The spec's own phased order (§2) calls stake/swap 'the novel core' at phase 3, BEFORE incentives at phase 4 — the proposal inverts this by pulling bonds (#3) forward and pushing adapters back. To a judge, a hardened exit + a QIF test harness reads as 'a mixer with a test suite.' QUALITY REALISM: pure-fn + proptest matches CLAUDE.md and cargo-llvm-cov truthfulness (good), but 'certifies worst-case <= 1/k' overstates proptest, and #3's bond math prices the wrong attacker (see feasibility_risks). MINOR: #2 cites the Helius funded-by API as if a client build-dependency, but that is adversary threat-surface evidence — the client already knows its own funding path — and a proprietary hosted indexing API sits awkwardly with the Rust-only/self-contained/MIT ethos."

### YAGNI flags

- #1 harness scope creep — multi-denomination fingerprinting: the Railgun knapsack amount-fingerprint adversary (3.42-bit) models attacks on VARYING amounts, but the pool is single-denomination today (Pool.denomination; deposit requires amount==denomination, lib.rs:75). With one bucket there is nothing to fingerprint. Building a knapsack decomposer for amounts that do not vary is premature until multi-bucket denominations ship.
- #1 harness scope creep — cross-round machinery: the Danezis SDA rounds-to-deanonymize bound + Buddies possinymity intersection model a REPEAT-PARTICIPANT / persistent-identity attack. But persistent-vs-per-intent identity is an unresolved open decision (the proposal's own open_decisions #2). Building — and coupling launch-k to — the cross-round SDA bound before that fork is resolved is YAGNI. The spec's mandated heuristics (§6 item 5) are FIFO/common-input/amount on a single round, not a cross-round SDA model.
- #1 effort mislabeled 'M': a QIF min-entropy harness that ports Tutela + Railgun + Buddies + Danezis-SDA and wires proptest shrinking + regression persistence is L/XL, not M. The genuinely M-sized, on-mission core is ONLY the self-fill collapse over the distinct-funder partition with a V=max_i p_i <= 1/k gate — that alone captures research-1's load-bearing finding.
- #4c embedding arti (a full Tor client) in the SDK adds a heavy dependency tree + circuit-bootstrap latency for a benefit the fee-payer/relayer already largely provides — the coordinator submits the settlement tx, so the user's IP is not on the on-chain transaction. The spec scopes network metadata as 'operational guidance (Tor/proxy recommended)' (§5), NOT a build item. Violates CLAUDE.md 'fewer moving parts / justify why the simpler version fails.'
- #3 pulls the incentive/bonding mechanism FORWARD from the spec's deferred phase-4 incentive module (§7 'explore later'; §3.4 'economic parameters deferred'; §2 phased order puts bonding at phase 4, AFTER PooledAction adapters at phase 3) — and at a different locus (per-INTENT bond vs the spec's per-MEMBER join bond in data-flow ①). This is scope the spec explicitly deferred, re-designed on the fly.
- Aggregate surface: six initiatives + a deferred register stacked on an in-flight Plan 4. Each is individually justified, but the only sanctioned seam is PooledAction, and the collective build adds a bond PDA, commit-reveal, a harness crate, a coordinator-metadata layer, an arti transport, and a chunk cursor — well beyond the MVP the bounty must demonstrate to prove the thesis.

### Feasibility risks

- On-chain distinct-funder counting (#3b) is NOT enforceable on this stack. A self-attested 'funded-by root' is attacker-supplied — a whale simply posts k fabricated distinct roots. Genuine funding provenance would need a ZK proof over historical chain state, which the Groth16/circom membership pipeline (crates/prover, verify_withdraw) does not provide. The proposal concedes the whale beats the cheap check with fresh funders and delegates the real defense to the harness — but the harness only MEASURES, it does not ENFORCE on-chain. So the frame's headline deliverable 'distinct-funding-source counting that makes k real' degrades to forgeable self-attestation + off-chain measurement. This is the sharpest technical gap and it undercuts #3's entire premise.
- #6 chunked executor cannot be atomic across Solana transactions — atomicity is per-transaction only. 'Chunked across multiple txs preserving atomicity/fail-closed' is a contradiction; it is resumability, not atomicity. It also breaks the spec's stated model: §4 ⑤ 'pool-program, atomically for the batch', §5 'atomic revert of the whole round' + failing-intent quarantine — all of which assume single-tx execution. A partially-executed round additionally leaks observable sub-batch timing structure that weakens the uniform-actor property. (Correctly YAGNI-gated, but the atomicity claim is wrong and the privacy side-effect is understated.)
- The k~19 account-envelope ceiling is computed for WITHDRAW (3 accounts/intent). The spec's actual behavioral core — pooled swap via Jupiter CPI (§3.1, §3.2 'the novel core') or stake — consumes many more accounts per intent, so a single-tx BEHAVIORAL round hits the 64-lock cap at low single-digit k, far below 19. The proposal never surfaces this because it models only the 3-account withdraw. (Credit: the ALT-lifts-message-keys-not-the-lock-cap point is accurate and sophisticated.)
- proptest cannot certify 'worst-case re-identification <= 1/k'. proptest is randomized sampling with shrinking, not exhaustive/worst-case proof. The honest claim the gate supports is 'no counterexample found under the modeled adversary on sampled inputs' — materially weaker than a worst-case certificate. Presenting a probabilistic test as a worst-case guarantee cuts against the fail-closed/honesty ethos CLAUDE.md demands of custody code.
- #3's forfeitable bond B prices the wrong vector. A whale self-filling k-1 slots to deanonymize a victim INTENDS to execute the round, so a forfeit-on-abort bond never fires. Only the non-refundable fee f prices that attacker, and (k-1)*f >= V forces a punitive f whenever the copy-trade/front-run payoff V is large — which also taxes honest users. The proposal conflates abort-grief and self-fill-deanon under one 'Sybil-priced' label.

### Vaporware flags

- #5 selective disclosure is modeled on Zcash ZIP-311, which the proposal itself labels 'proposed'. ZIP-311 is a Draft/Reserved Zcash ZIP that was never finalized or shipped in Zcash. The reveal-(preimage, path, nullifier)-nonce-bound construction is simple enough to stand on its own, but it should be presented as a first-principles design, not as a proven/deployed standard.
- #1's Railgun knapsack heuristic (the specific '3.42-bit median loss' figure) is sourced to a June-2026 arXiv preprint (2606.25926) that is weeks old and unverifiable in this environment. A load-bearing harness heuristic resting on a fresh, unreplicated preprint is citation-fragile — note the proposal itself quarantines the sibling IVC preprint (2606.10172) as watch-only, yet treats the Railgun one as build-input. Label it unverified rather than settled.
- CREDIT (not a flag): the explicit DEFER register correctly quarantines the real vaporware — Penumbra flow-encryption (unshipped), Arcium (mainnet-alpha), Token-2022 confidential transfers (disabled on mainnet since Jun 2025), Shutter (mainnet-beta), 0xbow Privacy Pools (mainnet-alpha), World ID, and the IVC/Nova preprint. This discipline is the proposal's strongest anti-vaporware feature and must be preserved. (arti is genuinely shipped/production at v1.0.0, so it is a YAGNI/dependency-weight concern, not vaporware.)

---

## Sources

1. <https://people.cs.umass.edu/~gbiss/mixing.pdf>
2. <https://eprint.iacr.org/2019/1111>
3. <https://arxiv.org/abs/2301.12813>
4. <https://arxiv.org/pdf/2301.12813>
5. <https://arxiv.org/abs/2504.20296>
6. <https://arxiv.org/pdf/2504.20296>
7. <https://papers.ssrn.com/sol3/papers.cfm?abstract_id=4563364>
8. <https://www.sciencedirect.com/science/article/pii/S2096720923000519>
9. <https://github.com/0xbow-io/privacy-pools-core>
10. <https://github.com/ameensol/privacy-pools>
11. <https://eprint.iacr.org/2023/273>
12. <https://www.gate.com/learn/articles/exploring-privacy-pools-a-new-on-chain-privacy-paradigm-backed-by-vitalik-buterin/8652>
13. <https://medium.com/@chainway_xyz/introducing-proof-of-innocence-built-on-tornado-cash-7336d185cda6>
14. <https://github.com/chainwayxyz/proof-of-innocence>
15. <https://rareskills.io/post/how-does-tornado-cash-work>
16. <https://github.com/tornadocash/tornado-core/blob/master/circuits/withdraw.circom>
17. <https://world.org/blog/engineering/iris-recognition-inference-system>
18. <https://world.org/blog/world/proof-of-personhood-what-it-is-why-its-needed>
19. <https://arxiv.org/pdf/2405.04463>
20. <https://docs.passport.human.tech/building-with-passport/stamps/major-concepts/scoring-thresholds>
21. <https://support.passport.human.tech/passport-knowledge-base/stamps/how-is-gitcoin-passports-score-calculated>
22. <https://arxiv.org/pdf/2607.07414v1>
23. <https://arxiv.org/html/2510.09433v1>
24. <https://arxiv.org/pdf/2203.09360>
25. <https://github.com/pareto-xyz/tutela-app/blob/main/README.md>
26. <https://arxiv.org/abs/2201.06811>
27. <https://arxiv.org/abs/2606.25926>
28. <https://cs.au.dk/~askarov/lbs-course/2025/reading/qif.pdf>
29. <https://www.lix.polytechnique.fr/~catuscia/papers/QIF/gleakage.pdf>
30. <https://bib.mixnetworks.org/pdf/serjantov2002towards.pdf>
31. <https://link.springer.com/chapter/10.1007/3-540-36467-6_5>
32. <https://nym.com/nym-whitepaper.pdf>
33. <https://arxiv.org/pdf/2107.12172>
34. <https://dl.acm.org/doi/10.1145/3672608.3707896>
35. <https://arxiv.org/pdf/2005.14051>
36. <https://arxiv.org/html/2510.09443v2>
37. <https://proptest-rs.github.io/proptest/>
38. <https://cacm.acm.org/research/a-fistful-of-bitcoins/>
39. <https://smeiklej.com/files/usenix22.pdf>
40. <https://arxiv.org/abs/2510.09433>
41. <https://www.chainalysis.com/blog/solana-chainalysis/>
42. <https://www.helius.dev/docs/wallet-api/funded-by>
43. <https://github.com/TrustaLabs/Airdrop-Sybil-Identification>
44. <https://arxiv.org/html/2505.09313v1>
45. <https://beosin.com/resources/a-closer-look-at-the-anti-sybil-mechanism-under-the-arbitrum>
46. <https://arxiv.org/html/2403.19530v2>
47. <https://repositum.tuwien.at/bitstream/20.500.12708/198225/1/Niedermayer%20Thomas%20-%202024%20-%20Detecting%20Bot%20Wallets%20on%20the%20Ethereum%20Blockchain.pdf>
48. <https://airdropfarming.org/blog/behavioural-fingerprints-chain-analysis-uses-to-cluster-wallets>
49. <https://arxiv.org/abs/1703.00536>
50. <https://arxiv.org/pdf/1805.03180>
51. <https://www.benthamsgaze.org/2018/05/09/the-pools-run-dry-analyzing-anonymity-in-zcash/>
52. <https://tornado-cash.medium.com/tornado-cash-introduces-arbitrary-amounts-shielded-transfers-8df92d93c37c>
53. <https://github.com/tornadocash/tornado-nova>
54. <https://arxiv.org/pdf/2606.25926>
55. <https://arxiv.org/pdf/2201.06811>
56. <https://pineanalytics.substack.com/p/solana-account-dusting-and-address>
57. <https://chain.link/article/crypto-dusting-attack>
58. <https://tornado-cash-2.gitbook.io/docs/general/staking>
59. <https://tornado-cash-2.gitbook.io/docs/general/how-to-become-a-relayer>
60. <https://docs.torn.cash/generals/staking>
61. <https://docs.railgun.org/developer-guide/wallet/broadcasters>
62. <https://docs.railgun.org/community-faqs/readme/costs-and-fees>
63. <https://docs.shutter.network/docs/protocol/api/how_it_works>
64. <https://eprint.iacr.org/2022/898>
65. <https://www.gnosis.io/blog/shutterized-gnosis-chain-is-live>
66. <https://blog.shutter.network/shutterized-gnosis-chain-is-now-live/>
67. <https://protocol.penumbra.zone/main/crypto/flow-encryption/threshold-encryption.html>
68. <https://protocol.penumbra.zone/main/concepts/batching_flows.html>
69. <https://protocol.penumbra.zone/main/crypto/flow-encryption/dkg.html>
70. <https://nym.com/docs/network/cryptography/sphinx>
71. <https://blog.torproject.org/arti_100_released/>
72. <https://blog.torproject.org/announcing-arti/>
73. <https://vac.dev/rlog/rln-relay/>
74. <https://arxiv.org/pdf/2207.00117>
75. <https://github.com/libp2p/rust-libp2p>
76. <https://www.helius.dev/blog/block-assembly-marketplace-bam>
77. <https://bam.dev/blog/introducing-bam/>
78. <https://www.helius.dev/blog/constellation>
79. <https://solanacompass.com/learn/Solfate/elusiv-enabling-private-token-swaps-on-solana-w-nico-co-founder-solfate-podcast-46>
80. <https://www.quicknode.com/guides/solana-development/3rd-party-integrations/privacy-with-elusiv>
81. <https://docs.arcium.com/multi-party-execution-environments-mxes/mxe-encryption>
82. <https://www.arcium.com/research/cerberus>
83. <https://blockeden.xyz/blog/2026/02/12/arcium-mainnet-alpha-encrypted-supercomputer-solana/>
84. <https://www.arcium.com/articles/eli5-honest-majority-vs-dishonest-majority>
85. <https://arxiv.org/html/2503.23986v1>
86. <https://www.gate.com/learn/articles/how-do-censorship-resistant-transactions-work-in-ethereum-rollups/3211>
87. <https://www.gate.com/learn/articles/practical-limitations-on-forced-inclusion-mechanisms-for-censorship-resistance/4246>
88. <https://eprint.iacr.org/2021/1465>
89. <https://eprint.iacr.org/2020/269>
90. <https://eprint.iacr.org/2025/2115>
91. <https://bips.dev/156/>
92. <https://protocol.penumbra.zone/main/addresses_keys/viewing_keys.html>
93. <https://protocol.penumbra.zone/main/crypto/fmd/construction.html>
94. <https://protocol.penumbra.zone/main/crypto/fmd.html>
95. <https://protocol.penumbra.zone/main/crypto/fmd/system_mapping.html>
96. <https://protocol.penumbra.zone/main/crypto/fmd/sender-receiver.html>
97. <https://zips.z.cash/zip-0311>
98. <https://zips.z.cash/zip-0310>
99. <https://zips.z.cash/zip-0032>
100. <https://zips.z.cash/zip-0316>
101. <https://zips.z.cash/protocol/protocol.pdf>
102. <https://github.com/zcash/zcash/blob/master/doc/payment-disclosure.md>
103. <https://z.cash/support/zcashd-deprecation/>
104. <https://github.com/0xbow-io/privacy-pools-core/>
105. <https://docs.privacypools.com/dev-guide>
106. <https://0xbow.io/blog/getting-started-with-privacy-pools>
107. <https://privacypools.com/whitepaper.pdf>
108. <https://www.globenewswire.com/news-release/2025/11/18/3190435/0/en/0xbow-Closes-3-5M-Round-for-Compliant-Crypto-Privacy-Technology-Following-Ethereum-Foundation-Integration.html>
109. <https://www.theblock.co/post/348959/0xbow-privacy-pools-new-cypherpunk-tool-inspired-research-ethereum-founder-vitalik-buterin>
110. <https://www.theblock.co/post/379395/0xbow-raises-3-5-million-seed-round-ethereum-foundation-backed-privacy-pools>
111. <https://docs.railgun.org/wiki/assurance/private-proofs-of-innocence>
112. <https://help.railway.xyz/private-proofs-of-innocence>
113. <https://github.com/Railgun-Community/private-proof-of-innocence>
114. <https://docs.railgun.org/wiki/learn/wallets-and-keys>
115. <https://docs.railgun.org/developer-guide/wallet/private-wallets/view-only-wallets>
116. <https://docs.namada.net/users/shielded-accounts/shielding>
117. <https://docs.namada.net/users/shielded-accounts>
118. <https://namada.net/blog/shielding-the-multichain-how-namadas-data-protection-works>
119. <https://github.com/namada-net/masp>
120. <https://namada.net/blog/namada-mainnet-launch-is-complete>
121. <https://arxiv.org/pdf/2606.10172>
122. <https://arxiv.org/abs/2606.10172>
123. <https://dl.acm.org/doi/10.1145/3460120.3484545>
124. <https://eprint.iacr.org/2021/1180>
125. <https://github.com/EspressoSystems/cape>
126. <https://cape.espressosys.com/cape-technical-documentation/introduction>
127. <https://www.coindesk.com/business/2022/06/16/espresso-systems-launches-testnet-of-cape-privacy-solution>
128. <https://www.freehaven.net/doc/e2e-traffic/e2e-traffic.pdf — Mathewson & Dingledine, 'Practical Traffic Analysis: Extending and Resisting Statistical Disclosure' (PET 2004): SDA estimator, Danezis rounds-to-deanonymize bound, pool-mix/mixnet extensions, padding and partial-observation defenses with simulation results.>
129. <https://link.springer.com/chapter/10.1007/978-0-387-35691-4_40 — Danezis, 'Statistical Disclosure Attacks' (IFIP SEC 2003): the original long-term intersection attack in statistical form.>
130. <https://arxiv.org/pdf/1305.5236 — Wolinsky, Syta & Ford, 'Hang With Your Buddies to Resist Intersection Attacks' (CCS 2013): possinymity/indinymity metrics, p^k/(p^k+1) decay, buddy-set lock-step churn and possinymity-threshold policies (Dissent prototype).>
131. <https://bib.mixnetworks.org/pdf/serjantov2002towards.pdf — Serjantov & Danezis, 'Towards an Information Theoretic Metric for Anonymity' (PET 2002): effective anonymity set size S = -Σ p log2 p.>
132. <https://en.wikipedia.org/wiki/Degree_of_anonymity — normalized degree of anonymity d = H(X)/H_M, H_M = log2(N), attributing Serjantov-Danezis and Diaz-Seys-Claessens-Preneel (PET 2002).>
133. <https://www.freehaven.net/anonbib/cache/DBLP:conf/esorics/MalleshW07.pdf — Mallesh & Wright, 'Countering Statistical Disclosure with Receiver-bound Cover Traffic' (ESORICS 2007): system-generated receiver-bound cover works; user/background cover insufficient.>
134. <https://arxiv.org/pdf/2005.14051 — Béres, Seres, Benczúr & Quintyne-Collins, 'Blockchain is Watching You: Profiling and Deanonymizing Ethereum Users' (2021): Tornado Cash linking heuristics, 400->12 timing collapse, 17.1% deanonymization, balance-fingerprint entropy/survival.>
135. <https://dl.acm.org/doi/10.1145/3672608.3707896 — 'Attacking Anonymity Set in Tornado Cash via Wallet Fingerprints' (ACM SAC 2025): wallet-fingerprint quasi-identifiers not tied to in-app behavior (abstract/metadata verified; full text paywalled).>
136. <https://arxiv.org/pdf/2606.25926 — 'A Tattered Cloak of Invisibility: Measuring Anonymity Loss in Railgun on Ethereum' (2026): on-chain shielded-pool anonymity-loss measurement over time (located via search; PDF body not machine-extracted — treat specific figures as unverified pending direct read).>
137. <https://link.springer.com/chapter/10.1007/978-3-540-75551-7_3 — Danezis, Diaz & Troncoso, 'Two-Sided Statistical Disclosure Attack' (PET 2007): extends SDA to link senders with reply-recipients, relevant to receiver-side intersection on action-targets.>
