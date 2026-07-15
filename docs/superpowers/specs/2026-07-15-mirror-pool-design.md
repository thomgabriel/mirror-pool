# mirror-pool — Architecture Design Spec

> **Status:** draft for review · **Date:** 2026-07-15
> **Prior art / research basis:** [`docs/research/prior-art.md`](../../research/prior-art.md)
> **Bounty:** Superteam Brazil — mirror-pool (crowd-sourced behavioral anonymity set; Rust-only)

---

## 1. Purpose & thesis

mirror-pool is a **crowd-sourced *behavioral* anonymity set** for Solana —
"Tornado Cash for synchronized on-chain actions/withdrawals, **not** for funds."
Observers can see that an action happened; they cannot determine **which**
participant initiated it.

The research (`prior-art.md`) confirmed this is a genuine white space: every
production Solana privacy protocol (Cloak, Privacy Cash, Umbra, Light, Arcium)
pools **value/notes** for fund/amount privacy — **none pools synchronized
behavioral actions**. mirror-pool reuses their proven cryptographic spine and
adds the two things none of them have: **actions as the unit of anonymity** and
a **protocol-enforced minimum anonymity-set size**.

### Locked design decisions

| Decision | Choice | Rationale |
|---|---|---|
| Scope | Full platform, layered + phased | User directive; kept buildable via module boundaries + build order |
| Core mechanism | ZK shielded pool + relayer | Dominant production pattern (Cloak/Privacy Cash/Umbra); Rust-native |
| Action model | Generic `PooledAction` trait | Serves "trivially customizable to integrate new protocols" |
| Round/execution | Custodial vault + generic executor + **`k`-floor** trigger | `k`-floor bakes the research's #1 lesson (never fire a thin round) into consensus |
| Custody | **Hardened custodial** | Custody is *required* for the uniform-actor property; hardened to shrink blast radius |
| ZK | Custom Groth16 circuits, **reusing audited references** | Best practice for Solana per-action privacy (native `alt_bn128`); zkVM is wrong shape |
| Incentives | Bonding + fee rewards, **pluggable, default** | Solves crowd-depth + Sybil together; no token/regulatory baggage; open to deepen |
| Compliance | **Opt-in viewing-key disclosure, no backdoor** | Survivors all bolt on compliance; keep it self-sovereign (research lesson) |

---

## 2. Architecture overview

Three layers of Rust, one extensibility seam (`PooledAction`), one hard safety
invariant (`k`-floor).

```
CLIENT (Rust SDK, user's machine)
  sdk (engine-first): note mgmt/scanning · CLIENT-SIDE proving · intent building · viewing-key disclosure
  circuits: Groth16 (membership · action-validity · value-conservation) + exported verifier keys
  facades: wallet · coordinator-client
        │ encrypted intent + proof                  │ scans chain for its notes
        ▼                                           ▼
OFF-CHAIN COORDINATION (Rust services)
  coordinator (relayer + round engine): mempool · round formation · k-FLOOR + timing · batch + fee-payer submit · retries
  indexer (optional): rebuilds tree/note state from chain (perf cache; SDK can read chain directly)
        │ submits round transactions
        ▼
ON-CHAIN (Solana programs, Rust — Anchor or Pinocchio)
  pool-program: per-mint custodial VAULTS · Merkle tree · nullifier set · 100-root ring · round state machine + k-floor · Groth16 verify · upgrade auth → Squads multisig
  action-adapters (pluggable): Withdraw/Transfer (built-in) · Stake · Swap(Jupiter) · LP · Vote  — behind PooledAction
```

**Trust boundaries.** The coordinator is a **liveness-only** trust point: it can
delay or censor and it observes *timing*, but it never learns "who → what"
(proving is client-side; intents are encrypted). Custody lives in the
`pool-program`, hardened by multisig upgrade authority + time-locks + caps.

**Phased build order** (makes "full platform" buildable, not a paper):

1. `pool-program` core + circuits + minimal SDK → working shielded pool (deposit, commit, prove, withdraw)
2. round engine + `k`-floor + coordinator → synchronized rounds
3. `PooledAction` adapters (stake, swap) → **behavioral pooling** (the novel core)
4. incentive module (bonding) + viewing-key disclosure → crowd depth + compliance
5. hardening (multisig, time-locks, caps) + indexer + audit + trusted-setup ceremony

---

## 3. Components

Each unit: **what it does · interface · dependencies.**

### 3.1 On-chain

**`pool-program`** — core state machine + custodian.
- **Does:** per-mint custodial vaults; Merkle commitment tree; nullifier set;
  100-root history ring; round state machine + `k`-floor enforcement; Groth16
  verification via `alt_bn128`; dispatch to action adapters.
- **Interface (instructions):** `initialize_pool`, `deposit`, `commit_intent`,
  `execute_round`, `withdraw`, `emergency_withdraw`.
- **State (PDAs):** `["pool",mint]`, `["vault",mint]`, `["tree",mint]`,
  `["nullifier",pool,hash]`, `["round",pool,round_id]`, `["member",pool,commitment]`.
- **Depends on:** `groth16-solana`, `action-adapters` (CPI), Squads multisig.

**`action-adapters` + `PooledAction`** — the extensibility seam.
```rust
pub trait PooledAction {
    type Params: BorshDeserialize;
    fn kind() -> ActionKind;
    fn validate(p: &Self::Params, ctx: &RoundCtx) -> Result<()>;   // well-formed, value-conserving, allowed
    fn execute(p: &Self::Params, vault: &VaultAuthority, accs: &[AccountInfo]) -> Result<ActionEffect>;
}
```
- **Built-ins:** `Withdraw`, `Transfer`. **Adapters:** `Stake`, `Swap` (Jupiter CPI), `LP`, `Vote`.
- **Depends on:** target protocol programs (CPI). Adding a protocol = one new adapter.

### 3.2 Off-chain

**`coordinator`** (relayer + round engine).
- **Does:** intent mempool; round formation; `k`-floor + timing policy; batch
  build; **fee-payer** submission; root-staleness retries; ALT creation.
- **Interface:** `POST /intents`, `GET /rounds/:id`, `GET /status/:req`.
- **Depends on:** Solana RPC, pool-program IDL. Liveness-only; replicable; holds
  no keys to user funds; cannot deanonymize.

**`indexer`** (optional perf cache).
- **Does:** reconstruct tree + note events from chain for fast scanning.
- **Interface:** `GET /commitments`, `GET /notes?since=`.
- **Depends on:** RPC. Optional — SDK can read chain directly (chain-native).

### 3.3 Client

**`sdk`** (engine-first, à la Cloak).
- **Does:** note lifecycle; **client-side Groth16 proving**; intent build +
  encryption; round participation; viewing-key disclosure; chain scanning.
- **Interface:** engine core + thin `wallet` / `coordinator-client` facades.
  Core types: `Note`, `Intent`, `Membership`, `DisclosureProof`.
- **Depends on:** `circuits` (proving keys), coordinator client, RPC/indexer.

**`circuits`**.
- **Does:** Groth16 circuits (membership, action-validity, nullifier derivation,
  value conservation) + verifier keys consumed by `pool-program`.
- **Toolchain:** **circom** authoring (reusing audited Tornado/Cloak/Light-style
  references) → **`ark-circom`** for client-side proving in Rust →
  **`groth16-solana`** on-chain verify. "Rust end-to-end" except the circuit DSL.
- **Depends on:** trusted-setup ceremony output (§6).

### 3.4 Cross-cutting

**`incentive-module`** (pluggable; default = bonding).
- **Does:** bond-to-join; cover-reward accounting (fee split to members who stay
  across rounds); slashing on defect/de-sync. Behind a trait so it can be
  deepened or tokenized later.
- **Interface:** lifecycle hooks `on_join`, `on_round_participate`, `on_defect`.
- **Depends on:** `pool-program` round lifecycle. **OPEN:** exact economic
  parameters deferred (user chose to explore later).

---

## 4. Data flow (one round)

```
① JOIN     bond X → pool-program records ["member",pool,C_m]  (anti-Sybil + reward eligibility)
② DEPOSIT  value → vault; append note C = H(amount, mint, secret, nullifier_seed) to tree; store secret locally
③ COMMIT   client LOCALLY: Groth16 proof {own note in root R, nullifier N derived, value conserved};
           encrypt intent (action+params) → submit {proof, N, encrypted_intent} to coordinator (never the secret)
④ FORM     coordinator mempools intents; at epoch tick: if |valid| ≥ k AND timing policy ok → batch, else roll forward
⑤ EXECUTE  pool-program, atomically for the batch: re-verify every proof vs a root in the ring; check+mark nullifiers;
   (chain)  ENFORCE k-floor on-chain; dispatch each intent to its PooledAction adapter via CPI with the VAULT as signer
           ("the pool staked to V" — uniform actor); credit outputs as NEW note commitments
⑥ SETTLE   client scans chain (or indexer), decrypts its output note; cover-reward accrues to stayers
⑦ EXIT     withdraw = a PooledAction: prove note ownership → pool sends to a FRESH destination
⑧ DISCLOSE (opt-in) client uses viewing key to prove its OWN history to a chosen third party; no global auditor
```

**Guarantees:** (a) *unlinkability* — coordinator sees `{proof, nullifier,
ciphertext, timing}`, never the mapping; (b) *uniform actor* — vault signs, so
on-chain it's "the pool did N actions"; (c) *`k`-anonymity by construction* —
sub-`k` batches rejected on-chain.

---

## 5. Threat model & failure handling

### Adversaries → defenses

| Adversary | Attack | Defense |
|---|---|---|
| Clustering / AI attribution | Link wallets to one entity | Uniform actor (no common-input); fresh unlinkable output notes |
| Timing correlation (FIFO 34.7%) | Match deposit→action→exit by time | `k`-floor + simultaneous batching; decouple deposit-time from action-time; jitter; per-member rate limits |
| Amount fingerprinting | Distinctive amounts | Discretized denomination buckets; non-standard amounts rejected/split |
| Sybil / set poisoning | Join N times to fake the crowd | Bond cost per membership (poisoning costs capital). *Residual:* not fully solved; optional per-round diversity heuristics |
| Malicious coordinator | Censor/reorder/force thin round | `k`-floor **on-chain**; client-side proving; permissionless coordinators; user self-submit / `emergency_withdraw` |
| Custody exploit | Drain the vault | Multisig upgrade auth; time-locks; **per-round + per-account caps**; emergency withdrawal-only; audited invariants |
| ZK soundness / trusted setup | Forge proofs → mint/drain | Reuse audited circuits; **multi-party trusted-setup ceremony**; strict verifier-key mgmt |
| Network metadata | IP logging via coordinator/RPC | No identity required; Tor/proxy recommended (operational guidance) |

**Trusted setup (note for non-ZK readers):** Groth16 needs a per-circuit setup
ceremony producing proving/verifying keys; if its secret ("toxic waste")
survives, its holder can forge proofs. Mitigation = a **public multi-party
ceremony** (Tornado/Zcash model): safe as long as one participant is honest. A
first-class launch task, not an afterthought.

### Failure handling

- **Sub-`k` round** → roll all intents forward; never fire.
- **Failed intent in batch** → **atomic revert of the whole round**; failing
  intent **quarantined** from the immediate retry (prevents repeated stalls and
  removes a forced-failure de-anonymization lever). *(v1 decision; per-intent
  isolation reconsidered only if liveness demands it.)*
- **Stale Merkle root** → rebuild against a fresh root in the 100-root ring, resubmit.
- **Coordinator down / censoring** → after timeout, user **self-submits** or
  calls `emergency_withdraw` (coordinator-independent withdrawal path).
- **Slashing triggers** → commit-then-abstain / de-sync / attempted double-spend
  → bond slashed into the cover-reward pool.

---

## 6. Testing strategy

Prove two things: that it *works*, and that it actually *hides*.

1. **Circuit tests** — correctness (valid witness → proof) + soundness (invalid
   witness → unprovable); reuse reference circuits' vectors; assert constraint counts.
2. **Program invariant tests** (`proptest`) — value conservation (`Σin = Σout`);
   nullifier uniqueness; **`k`-floor** rejects sub-`k` batches; Merkle append +
   ring correctness; vault authority.
3. **Integration tests** (`solana-program-test` / LiteSVM) — full lifecycle
   join→deposit→commit→round→settle→withdraw; multi-member rounds; real CPI adapters.
4. **Adversarial / negative tests** — forged proof, replayed nullifier, sub-`k`
   round, coordinator forcing thin/reordered round, `emergency_withdraw` with
   coordinator offline, caps enforcement — all must fail-closed.
5. **Anonymity tests (differentiator)** — simulate many rounds, run the real
   deanonymization heuristics (FIFO timing, common-input, amount fingerprinting)
   against the resulting graph, and **assert attribution probability ≤ 1/k**.
   Measures privacy empirically instead of assuming it.
6. **Fuzzing** — instruction data, proof bytes, intent payloads.
7. **Scale tests** — thousands of intents through round formation; tree growth
   and proving/scan performance ("thousands running for weeks" bar).

---

## 7. Open questions (deferred, not blocking)

- **Incentive economics** — exact bond size, cover-reward formula, slashing
  params. User chose to explore later; default bonding module ships first.
- **`k` value & denomination buckets** — the concrete anonymity-set floor and
  per-action amount buckets (tune against the anonymity tests in §6).
- **Circuit reference selection** — which audited circuit family to adapt
  (Tornado vs. Cloak-style 2-in/2-out vs. Light) — finalize at Phase 1.
- **Anchor vs. Pinocchio** for the on-chain program — Anchor (ergonomics/ecosystem)
  vs. Pinocchio (compute-unit efficiency); decide at Phase 1.
- **Coordinator decentralization** — v1 is a single replicable service; a
  permissionless multi-coordinator / relayer market is future work.

---

## 8. Non-goals (v1)

- No native token or on-chain governance (incentives are a pluggable module).
- No global auditor / compliance backdoor (opt-in user disclosure only).
- No cross-chain / bridging.
- Not a funds mixer positioning — behavioral pooling is the differentiator;
  withdrawals are just one `PooledAction`.
