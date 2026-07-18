# mirror-pool

**Tornado Cash for behavior, not funds** — a crowd-sourced *behavioral* anonymity set for Solana.

A mixer hides *how much* moved and *from whom*. mirror-pool hides **who initiated an on-chain
action that everyone can see happened.** N participants pool one identical action (a withdrawal,
a native-stake delegation, …) into a synchronized round; the round executes on-chain in full
view, signed by a **single uniform actor**, so an observer sees N identical actions occur but
cannot attribute any one of them to the wallet that initiated it. This is **k-anonymity over
*actions*** — deliberately *not* over denominations, and *not* a way to hide balances or move
value privately.

---

## How it works

A participant deposits into a Groth16 shielded pool, then acts through a two-phase round:

1. **`deposit`** — post a commitment `H(secret, …)`; it becomes a leaf in an on-chain
   Poseidon Merkle tree. No link between the deposit and any later action is created on-chain.
2. **`commit_intent`** — prove *in zero knowledge* that you own a note in the tree (this **burns
   the note's nullifier**, preventing re-use), and record an *intent* whose payout recipient and
   relayer are bound into the proof (via `extDataHash`, so a relayer can't redirect your funds).
   **No payout happens here.**
3. **`execute_round`** — once at least `k_floor` intents have accumulated, **one vault-signed
   transaction** executes *all* `k` identical actions in a single batch, dispatched through the
   `PooledAction` trait. Because the **vault** (a program PDA) signs every action, **no
   participant signature appears on any executed action** — that uniform actor is what makes the
   initiators unlinkable. The `k`-floor is enforced *on-chain*: a round below the floor never fires —
  though it is a liveness *count*, not the realized-anonymity guarantee (that is the entropy-based
  **effective-k**, below).

The one sanctioned extension seam is the **`PooledAction` trait** — adding a new action type is
one adapter. Two are shipped: `Withdraw` and native-stake `Stake`.

---

## Architecture

Three layers, one canonical implementation of every hash shared across all of them (so an
off-chain-computed value can never silently disagree with what the chain checks):

| Layer | Where | What |
|---|---|---|
| **On-chain program** | [`programs/pool-program`](programs/README.md) | The custody + round-engine: `initialize_pool` / `deposit` / `commit_intent` / `execute_round` / `cancel_intent`, the on-chain `k`-floor, the Poseidon accumulator, the Groth16 verifier, and the `PooledAction` adapters. |
| **Circuits** | [`circuits/`](circuits/README.md) | The circom membership circuit (note ownership + nullifier + extDataHash), shared by *every* action, and its Groth16 setup. |
| **Host crates** | [`crates/`](crates/README.md) | The client SDK, the Rust ZK prover, the shared `extDataHash`, build-time tooling, and the effective-k analysis instrument. |

Design & decisions live in [`docs/superpowers/specs/`](docs/superpowers/specs); the research that
grounds them is in [`docs/research/`](docs/research).

---

## Design rationale

Each non-obvious choice traces to a specific result in the literature:

- **Measure anonymity by the adversary's *posterior*, not the crowd size.** Serjantov & Danezis
  (*Towards an Information-Theoretic Metric for Anonymity*, PET 2002) showed an anonymity set's
  *size* overstates protection whenever the adversary's probability isn't uniform across it — the
  honest measure is the entropy of that posterior. So while the on-chain `k`-floor is a count, the
  anonymity mirror-pool *reports* is entropy-based.
- **Use *min-entropy*, because the honest threat is a single-guess attacker.** The measure is Geoffrey
  Smith's (*On the Foundations of Quantitative Information Flow*, FoSSaCS 2009): `effective-k = 1/V(X)
  = 2^{H∞}`, where vulnerability `V(X) = maxᵢ pᵢ` *is* the optimal single-guess success — the noise-free,
  single-guess framework this design lives in. Dodis, Reyzin & Smith (2007) frame the same
  predictability; Tóth, Hornák & Vajda (PET 2004) built two distributions of *identical Shannon entropy*
  where one leaks 5% and the other 50% — so Shannon and nominal `k` can look healthy while real
  anonymity sits at the floor. That is why `crates/effective-k` reports **min-entropy effective-k**
  (`k_∞ = 1 / maxᵢ pᵢ = k/m`), not a count. (`k/m` and the guessing advantage `(m−1)/k` are our labeled
  arithmetic instantiations of Smith's definition, not literature-named terms.)
- **A group of size `k` isn't protected when one member dominates it.** The k-anonymity →
  l-diversity → t-closeness line (Sweeney 2002; Machanavajjhala et al. 2007; Li et al. 2007) is
  exactly the finding that a `k`-sized group fails under a *homogeneity* attack — one value holding
  most of the mass. mirror-pool's "whale self-fill" residual is that attack re-cast: one funder
  owning `m` of the `k` notes, which min-entropy effective-k catches precisely (`k_∞ = k/m`).

- **Batch on one timestamp behind a uniform actor, because timing and metadata break mixers — not
  the crypto.** Empirical studies of deployed Tornado-style pools (Wu et al., *Tutela*, 2022; Wang
  et al., WWW 2023) show they leak *well below* their advertised set via timing, address-reuse, and
  gas/fingerprint heuristics. mirror-pool's answer is one vault-signed batch on a single timestamp —
  one signer, one fee, one gas payer — so those heuristics have nothing per-initiator to read.

- **The obvious anti-Sybil fixes are traps — and we say why.** A mechanism deep-dive rejected
  Rate-Limiting Nullifiers (rate-limits *one* identity; orthogonal to distinct-identity self-fill),
  anonymity mining (leaks by construction), operator-funded decoys (the operator *is* the whale),
  and cover traffic (recreates the whale). What we build instead is a nominal-cost `fee`; deeper
  Sybil resistance is priced, not overclaimed.

The full analysis behind these — plus the "build a pool-wide `fee`, defer bonding" verdicts — is in
[`docs/research/`](docs/research).

---

## Limitations

Where the guarantee stops:

- **The `k`-floor is a *liveness* gate, not a measure of realized anonymity.** One funder who
  self-fills `m` of the `k` notes ("whale self-fill") collapses the effective anonymity toward 1.
  `crates/effective-k` *measures* this residual (`k_∞ = k/m`); it does not remove it.
- **The per-round anonymity-set size is capped by the Solana transaction envelope, now enforced
  on-chain**: `MAX_K` = 17 (withdraw) / 10 (stake), pinned by measurement (measured ceilings
  18/11, shipped one below for cranker headroom). `execute_round` settles the whole round in one
  vault-signed transaction, so a round can never grow past what that transaction can settle. The
  two kinds are bound by *different* dimensions: withdraw by the 64-account-lock limit
  (compute-clean to k=21); stake by the 32 KB SBF heap (a bump allocator that never frees — hits
  out-of-memory at k=12, well below the lock or compute ceilings, and not liftable by the cranker
  via `request_heap_frame`, which was measured to change nothing). A larger `k` would need chunked
  execution (not built) or an on-chain custom allocator (stake only; future work) — see
  `docs/research/solana-execution-limits.md`.
- **A residual mechanism gap is documented, not hidden.** The stake path's create-vs-normalize branch
  leaves a per-intent inner-instruction / vault-debit *shape* difference when a stake PDA is pre-funded
  (a chain-observable distinguisher) — analyzed in `docs/research/` rather than narrated away.
- **Crowd depth / Sybil resistance is the binding constraint** — and it is *priced, not solved*.
  A mandatory `fee` raises the *nominal* cost of self-fill; it does not deepen *distinct-human* `k`.
- **`cancel_intent` is a single-note, non-batch exit** (a liveness safety-valve for a round that
  never fills). It is *not* `k`-anonymous, and it is now **timeout-gated** so it can't be used as
  an on-demand bypass — but the residual sub-`k` linkage is disclosed, not hidden.
- **Repeated participation degrades anonymity** — *intersection / statistical-disclosure attacks*
  (Danezis 2003; Mathewson & Dingledine 2004) let an observer shrink a repeat participant's
  anonymity set across rounds on public chain data. Stake delegation is the more clusterable action
  (richer observable surface).
- **The trusted setup shipped here is dev-only** (deterministic beacon, public toxic waste). A
  production multi-party ceremony is required before mainnet.
- **The coordinator is a *liveness-only* trust** — it can censor or stall, but proving is
  client-side, so it can never deanonymize a participant or redirect funds.

---

## Status

Local-only (`main` is ahead of `origin`, not yet pushed). Shipped and merged:

| Phase | What |
|---|---|
| Plans 1–3 | Pool foundations (Poseidon, height-20 Merkle tree, root ring, nullifiers) → circuits → wired ZK + SDK |
| Plan 4 | Behavioral round engine (on-chain `k`-floor, `commit_intent`/`execute_round`, `PooledAction`) |
| Plan 5 | Pooled native-stake (the 2nd `PooledAction`, vault-unilateral delegation) |
| Plan 6a | Timeout-gated `cancel_intent` |
| Pool.fee | One mandatory pool-wide fee — closes a withdraw-pool amount fingerprint + nominal anti-Sybil tax |
| Plan 6b | `crates/effective-k` — the min-entropy effective-k measurement core |
| Research | Frontier-delta validation — Smith 2009 (QIF) anchored as effective-k's definitional source; the Solana execution-limits (`MAX_K`) envelope; two open mechanism gaps disclosed honestly |

Every phase was built via spec → plan → TDD, with an independent review gate on the spec, the
plan, and the merged branch.

---

## Build & test

Rust-only; toolchain is pinned (`rust-toolchain.toml` → 1.92.0, Anchor 0.31.1, Agave/Solana 3.0.1,
platform-tools v1.54). Tests are Rust-native **LiteSVM** (no `solana-test-validator` in the inner loop).

```bash
# Host unit + integration tests (the fast loop)
cargo test --workspace

# Build the on-chain program for SBF, then run its in-VM (LiteSVM) tests
cargo build-sbf --manifest-path programs/pool-program/Cargo.toml
cargo test -p pool-program

# The ZK circuits (needs circom 2.1.6 + snarkjs — see circuits/README.md)
cd circuits && bash scripts/setup.sh
```

Lint/supply-chain gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo deny check` (config in `deny.toml`). CI mirrors these — see `.github/workflows/ci.yml`.

---

## Repository layout

```
programs/pool-program/   on-chain Anchor program        → programs/README.md
crates/                  host-side crates (SDK, prover, tooling, analysis) → crates/README.md
circuits/                circom membership circuit + Groth16 setup → circuits/README.md
docs/research/           the research that grounds the design
docs/superpowers/        specs (designs) and plans (implementation)
deny.toml                cargo-deny supply-chain policy
rust-toolchain.toml      pinned toolchain (reproducible builds)
```

## License

MIT — see [LICENSE](LICENSE).
