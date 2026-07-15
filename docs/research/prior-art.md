# Prior Art: Solana Privacy Protocols → mirror-pool

> Research synthesis informing the design of **mirror-pool** — a crowd-sourced
> *behavioral* anonymity set (Tornado Cash for synchronized on-chain
> actions/withdrawals, **not** for funds).
>
> Method: multi-source web research with adversarial 3-vote verification of each
> claim (22 confirmed / 3 refuted across 26 sources, 6 research angles),
> supplemented by a first-hand read of Cloak's documentation. Claims below are
> tagged with confidence; unverified or refuted items are called out explicitly.
>
> Last updated: 2026-07-15.

---

## TL;DR

- **The behavioral-pooling thesis is a genuine white space.** Every production
  protocol verified — Cloak, Light Protocol, Arcium, Privacy Cash, Confidential
  Balances — pools **value/notes** for fund/amount/counterparty privacy. **None
  pools synchronized behavioral actions.** Anonymity strength is *universally* a
  function of pool depth/activity that the protocol itself cannot manufacture.
- **There is a mature, reusable spine.** Best documented by **Cloak**: a
  Groth16 shielded-UTXO pool with a Merkle commitment tree, a bounded root-history
  ring, nullifier PDAs, a relayer that submits on the user's behalf, and an
  engine-first Rust SDK. mirror-pool can lift this spine directly.
- **The novel part mirror-pool must invent** is not the membership proof (solved)
  but: (1) set membership over *actions* rather than denominated notes,
  (2) defeating cross-crowd **timing correlation** (the dominant real-world
  attack), and (3) the coordinator's **incentive + anti-Sybil** model.

---

## 1. Protocol comparison

| Protocol | Mechanism (verified) | What it pools | Anonymity set | Relevance to mirror-pool |
|---|---|---|---|---|
| **Cloak** | Groth16 shielded-UTXO pool; in-browser proving (~3s), on-chain verify (~50ms). Live mainnet program `zh1eLd6rSphLejbFfJEneUwzHRfMKxgzrgkfwA6qRkW` (RPC-confirmed executable). | Funds/notes — amounts, addresses, history | Merkle tree of note commitments (height 32) | **Primary blueprint** — reuse on-chain layout, relayer, Rust SDK shape |
| **Light Protocol** | ZK Compression = Groth16 *validity proofs* (constant 128 bytes), 4-program Anchor Rust monorepo. **Provides NO privacy** — compressed state is public calldata. (Light *the org* separately ships private-transfer tooling.) | Nothing — it is state-compression, not a mixer | None | A cheap-state primitive to **build on**, not an anonymity mechanism to copy |
| **Arcium** | Encrypted **MPC** network (MXE execution environments, Arx nodes computing on secret shares; "Cerberus" blends MPC + semi-homomorphic encryption). **Originated as Elusiv.** Mainnet-alpha 2026-02-02. | Private *computation*, not action timing | N/A (compute network) | The MPC branch — relevant only if action-pooling ever needs private *joint* computation instead of per-user proofs |
| **Umbra** | Shielded pool on Solana; "Stealth Pool Notes" as **Indexed Merkle Tree** commitments + EncryptedTokenAccounts + `burn_*` instructions. ZK = **Groth16 (snarkjs) in a Web Worker**; confidential arithmetic via **Arcium MPC** (off-chain, nodes never decrypt). Mainnet program `UMBRAD2ishebJTcgCLkTkNUx1v3GyoAgpTRPeWoLykh`. Launched 2026-02-02 as the first app on Arcium. | Funds (shielded transfers, encrypted swaps) | Merkle tree of note commitments — but **gated rollout** (100 users/week, $500 cap at launch) → small early set | **Closest architectural cousin** to mirror-pool (shielded pool + relayer), but TS-SDK + leans on Arcium MPC. mirror-pool differentiates as Rust-native + behavioral |
| **Privacy Cash** | Tornado-style ZK mixer for SOL/SPL (launched 2025-08, open-source, reportedly "14 audits"): deposit commitment → on-chain Merkle tree → ZK withdraw to a fresh, unrelated address, breaking the deposit→withdrawal link. Most-used Solana privacy protocol by volume ($150–200M+ processed). | Funds | Merkle tree of deposit commitments | The canonical Tornado-on-Solana reference the bounty itself cites |
| **ORE (shielded pool)** | Not its own tech — **ORE uses Privacy Cash's shielded pool** via partnership ("official shielded pool"). Team **seeded the pool with 100+ ORE to bootstrap the anonymity set**. | Funds | Shared Privacy Cash pool | Concrete example of the **bootstrapping problem** mirror-pool faces (protocol must seed its own initial set) |
| **Confidential Balances** (Token-2022) | Twisted ElGamal encryption + authenticated encryption; proof types = **range + equality + validity proofs** verified by the **ZK ElGamal Proof Program**. Hides amounts/balances; addresses/mint/owners stay public. Optional **auditor ElGamal pubkey** for compliance. | Amounts only (not transaction existence) | N/A (per-account confidentiality) | **Not live on mainnet** as of latest docs — devnet-only, scheduled mainnet June 2026; "under audit." A native *compliant confidential-amount* primitive, not an anonymity set. See §7. |

---

## 2. The reusable production spine (from Cloak)

Directly liftable for mirror-pool's on-chain program and coordinator. All specs
below are confirmed against Cloak's primary docs (`docs.cloak.ag`).

### 2.1 On-chain anonymity-set layout

- **Incremental append-only Merkle tree**, `MERKLE_TREE_HEIGHT = 32`, storing
  commitments (the anonymity set).
- **Bounded root-history ring**, `ROOT_HISTORY_SIZE = 100` — the program accepts
  proofs built against any of the last 100 accepted roots, so a proof against a
  slightly-stale root still validates (critical for liveness under concurrency).
- **Nullifier PDAs**, seed `["nullifier", pool_pubkey, nullifier_hash]`, created
  for non-zero input nullifiers to enforce one-time use without revealing which
  commitment was spent.
- **In-circuit invariants**: value conservation
  `sum(inputs) + publicAmount = sum(outputs)`, root membership, nullifier
  uniqueness, transfer invariants.
- **Public inputs = 264 bytes**: `root[32] + publicAmount[8] + extDataHash[32] +
  mint[32] + nullifiers[64] + commitments[64] + chainNoteHash[32]` (nullifiers
  and commitments are 2×32 each, matching the 2-in/2-out model).
- **Proof**: 256-byte Groth16 over BN254 (2×G1 + 1×G2), verified via Solana's
  `alt_bn128` syscalls (`groth16-solana`-style verifier).

**mirror-pool adaptation:** the "note" becomes a *right to participate in an
action-round*; the nullifier prevents double-claiming a round slot rather than
double-spending a coin.

### 2.2 Relayer-as-link-breaker

- A coordinator submits the transaction so the acting wallet **"pays no gas and
  never appears as the sender"** (verbatim, cloak.ag). Default relay
  `https://api.cloak.ag`, configurable via `relay_url`.
- Handles stale-root retry loop (`RootNotFound` / `BlockhashExpired` /
  `StaleProofState` with blockhash + transport backoff) and ephemeral Address
  Lookup Table (ALT) auto-creation for oversized v0 transactions.
- **Scope nuance (verifier-caught):** the no-gas/no-sender property holds for
  *relayed* withdrawals/transfers. **Deposits use a direct signer path**, so the
  wallet still appears at entry. The relayer's own language/stack is **not**
  publicly confirmed (a claim that it is a Rust `services/relay` with no TEE/MPC
  was refuted 0–3) — do not assume.

### 2.3 Engine-first Rust SDK split

- A **stateless `transact()` engine core** (`cloak-sdk` Rust crate, engine-first,
  "no stateful wrapper"), with the TypeScript `CloakSDK` class as a thin
  wallet-aware façade over the same engine.
- `TransactResult` exposes commitments/indices/post-tx tree so callers build
  their own wallet, scanner, or compliance report on top.
- **mirror-pool adaptation:** engine core + separate wallet facade + coordinator
  facade. This is the recommended coordinator architecture.

### 2.4 Optional admission / compliance gate

- Cloak prepends a **Risk-oracle Ed25519 sigverify instruction at tx index 0**,
  and wraps note payloads in a **Chain-note v2 envelope (HKDF-SHA256 +
  AES-256-GCM)**. Circuit artifacts are versioned (circuits 0.1.0).
- Reusable if mirror-pool needs an admission/compliance oracle or encrypted
  per-user action payloads.

### 2.5 Design lesson: standardized shapes

Cloak's fixed **2-in/2-out, 9 public signals** and its explicit warning that
"if very few similar deposits exist, timing and amount correlation gets easier"
carry the same lesson as fixed denominations in Tornado: **uniform transaction
shapes keep the set indistinguishable.** mirror-pool needs *standardized action
templates* (same protocol, discretized amount buckets) or the crowd dissolves.

---

## 3. Threat model: the number that matters

From a 2025 arXiv cross-chain analysis of Tornado Cash deanonymization:

- Address reuse + transactional linkage alone deanonymized **5.1–12.6%** of
  withdrawals.
- Adding a **FIFO temporal-matching heuristic pushed this to 34.7%**.

**Implication for mirror-pool:** timing correlation is the *dominant* attack on
any pool. A *behavioral* pool that fires actions on a schedule is **more**
timing-exposed than a funds mixer, not less. Defeating temporal linkage
(commit-reveal windows, threshold/batched release, decoy actions, jittered
execution) is the core cryptographic problem — membership proofs are already a
solved, reusable pattern.

---

## 4. What mirror-pool must invent (no existing template)

1. **Set membership over *actions*, not denominated notes** — define "the same
   action" such that N of them are mutually indistinguishable on-chain.
2. **Defeating cross-crowd timing correlation** — the 34.7% attack above, applied
   to a crowd of synchronized intents.
3. **Coordinator incentive + anti-Sybil model** — keep the crowd deep when a
   participant does not currently need cover (their presence *is* others' cover),
   and prevent an adversary from joining 999× to hollow out the set
   (anonymity-set poisoning).

---

## 5. Confidence & coverage gaps

- ~~**Confidential Balances status is UNCERTAIN.**~~ **RESOLVED in §7.3** — not
  live on mainnet; devnet-only, scheduled for mainnet June 2026, under audit.
- ~~**Umbra and Elusiv/PrivacyCash/ORE produced no confirmed claims.**~~
  **RESOLVED in §7.1–7.2** via first-hand follow-up research.
- **Arcium coverage is high-level only** — mechanism verified, but on-chain
  program design, incentive model, and Rust stack were not.
- **Source quality:** nearly all confirmed claims are self-reported vendor docs —
  authoritative for a protocol's *own* architecture, but not independent audits
  of deployed bytecode. The one independent check was Cloak's mainnet program ID
  (confirmed executable via RPC).
- **Time-sensitivity:** fast-moving space — Arcium was mainnet-alpha (Feb 2026),
  Light Protocol was recently acquired by Helius, Cloak is a live 2026 protocol.

### Refuted claims (do not rely on)

1. Token-2022 Confidential Transfers are live on mainnet with protocol-level ZK
   hiding amounts — **refuted 1–2** (mainnet status/mechanism uncertain; see
   epoch-805 disablement above).
2. Cloak's relay is specifically a Rust `services/relay` service with no TEE/MPC —
   **refuted 0–3** (relay internals not publicly confirmed).
3. "Light Protocol is not a privacy protocol at all" — **refuted 0–3** (the *ZK
   Compression primitive* is not private, but Light's wider stack includes
   separate privacy tooling).

---

## 6. Open questions for follow-up research

- ~~Is Token-2022 Confidential Transfers live on mainnet?~~ Resolved (§7.3): no,
  devnet-only, mainnet target June 2026. *Re-check the actual mainnet feature-gate
  status when the spec depends on it — June 2026 is now.*
- ~~Umbra / Elusiv / PrivacyCash / ORE mechanisms & stacks?~~ Resolved (§7.1–7.2).
  Remaining sub-question: Umbra's *realized* concurrent set size after the gated
  rollout expands.
- What is Cloak's *realized concurrent* anonymity-set size and denomination
  distribution in practice? (Only a cumulative lifetime counter — "24,187
  shielded transactions" — is disclosed, not the effective indistinguishable-set
  size, which is the metric that actually determines privacy strength.)
- **[Design, carried into the architecture spec]** How is a behavioral/action
  anonymity set cryptographically constructed and coordinated — set membership
  over actions, timing-correlation defenses (commit-reveal, threshold/batched
  release, decoys), the anti-Sybil model, and the cold-start/bootstrapping
  strategy (cf. ORE seeding, Umbra gating)?

---

## 7. Gap-closing addendum (2026-07-15)

Follow-up first-hand research on the three items §5 flagged as thin/uncertain.

### 7.1 Umbra — the closest architectural cousin

- **Mechanism:** shielded pool where deposits become "Stealth Pool Notes"
  (Indexed Merkle Tree commitments) held in `EncryptedTokenAccounts`; spends are
  `burn_*` instructions. **Groth16 (via snarkjs) generated client-side in a Web
  Worker**; confidential arithmetic is delegated **off-chain to Arcium's MPC**
  (MXE — nodes compute without decrypting). Mainnet program
  `UMBRAD2ishebJTcgCLkTkNUx1v3GyoAgpTRPeWoLykh`.
- **Stack:** SDK is **TypeScript** (`@umbra-privacy/sdk`; ZK prover, indexer
  client, relayer client as subpath modules) — **not** a Rust SDK like Cloak.
  Relayer submits burns (`/v1/claims`) with no on-chain sender linkage.
- **Real-world anonymity-set datapoint:** launched 2026-02-02 as the *first* app
  on Arcium Mainnet Alpha, but with a **deliberately gated rollout — ~100 users/
  week and a $500 deposit cap** at launch. Even a well-funded launch starts with
  a *tiny* set. This is direct evidence that crowd-depth bootstrapping is the
  binding constraint, not the cryptography.
- **Takeaway for mirror-pool:** architecturally adjacent (shielded pool +
  relayer link-breaker), so mirror-pool's differentiation is being **Rust-native**
  and **behavioral** (pooling actions, not funds), rather than another
  fund-shielding pool that outsources compute to Arcium.

### 7.2 Elusiv → Arcium, and the Privacy Cash / ORE lineage

- **Elusiv (Privacy 1.0):** application-layer ZK-SNARK **shared privacy pool**
  with viewing keys for selective disclosure; began Nov 2022 ($3.5M seed).
  **Sunset March 2024** (withdrawal-only through Jan 2025) under post-Tornado-Cash
  regulatory pressure. The team rebuilt from scratch as **Arcium** (Privacy 2.0 =
  MPC confidential compute; +$5.5M, ~$9M total).
- **Regulatory lesson (important for mirror-pool):** the pure-mixer model was
  regulatorily untenable and was abandoned. **Every survivor bolts on
  compliance** — Cloak's viewing keys + risk oracle, Privacy Cash's "compliance
  architecture," Confidential Transfers' auditor key. mirror-pool should design
  **optional selective disclosure from day one**, not as an afterthought.
- **Privacy Cash:** live Tornado-style ZK mixer (commitment → Merkle tree → ZK
  withdraw to fresh address). Launched Aug 2025; $150–200M+ processed; "14
  audits"; supports SOL/USDC/USDT/ZEC/ORE. Framed as "Tornado Cash with a
  compliance architecture."
- **ORE:** does **not** build its own privacy — it **uses Privacy Cash's shielded
  pool** and the team **seeded the pool with 100+ ORE to bootstrap the anonymity
  set**. A concrete instance of the cold-start problem mirror-pool must solve.

### 7.3 Confidential Transfers status — RESOLVED

The §5 uncertainty is resolved against Solana's own docs (via the Solana MCP
RAG over `solana-program.com` and `solana.com/docs`):

- **NOT live on mainnet** as of the latest docs. Token-2022 status page: "all
  clusters have the latest program deployed **without confidential transfer
  functionality**." The Confidential Transfer Issuer Guide: confidential
  transfers "are available on **devnet today** and are **scheduled to be enabled
  on mainnet in June 2026**." Token-2022 is "still under audit and not meant for
  full production use." → the earlier "live on mainnet" claim was **premature**;
  June 2026 is right around now, so treat status as *transitional* and verify on
  the specific cluster.
- **Mechanism (confirmed):** Twisted ElGamal encryption (keypair generated
  locally, pubkey stored on the token account) + authenticated encryption for the
  decryptable balance. Three proof types verified by the **ZK ElGamal Proof
  Program**: **range proofs** (amount is a positive 64-bit int; remaining balance
  ≥ 0), **equality proofs** (two ciphertexts encrypt the same value), and
  **validity proofs**. It hides **amounts/balances only** — addresses, mint, and
  owners stay public — so it is *amount confidentiality*, **not an anonymity
  set**. An optional **auditor ElGamal pubkey** lets a designated party decrypt
  every amount (compliance).
- **Rust stack:** `solana-zk-sdk`, `spl-token-2022`,
  `spl-token-confidential-transfer-proof-generation` / `-extraction`,
  `zk_elgamal_proof_program`. The 2025 ZK ElGamal vulnerability (patched April
  2025; program kept disabled pending audit) is consistent with the
  under-audit / devnet-only status.
- **Takeaway for mirror-pool:** usable later as a *native compliant confidential-
  amount layer* to compose with, but it is not an anonymity mechanism and is not
  yet mainnet-ready — do not make it a hard dependency.

---

## Sources

Primary (protocol-authoritative):
- Cloak — https://www.cloak.ag/ ; https://docs.cloak.ag/platform/overview ;
  `/protocol/architecture` ; `/protocol/shield-pool` ; `/platform/components` ;
  `/sdk/rust/introduction` ; `/guide/security` ; `/llms.txt`
- Solana privacy landscape — https://solana.com/privacy
- Solana ZK ElGamal post-mortem — https://solana.com/news/post-mortem-june-25-2025
- Light Protocol — https://github.com/Lightprotocol/light-protocol ;
  https://github.com/Lightprotocol/groth16-solana ; https://www.zkcompression.com/
- Arcium — https://docs.arcium.com/introduction/basic-concepts
- Confidential Balances — https://www.solana-program.com/docs/confidential-balances

Secondary / analysis:
- Solana Confidential Transfers breakdown — https://chainstack.com/solana-confidential-transfers/
- Privacy on Solana (Elusiv & Light) — https://www.helius.dev/blog/privacy-on-solana-with-elusiv-and-light
- ORE / Privacy Cash shielded pool — https://ourcryptotalk.com/news/ore-privacycash-shielded-pool-solana
- Tornado Cash clustering (cross-chain, 2025) — https://arxiv.org/html/2510.09433v1
- Tornado deposit-pattern analysis — https://cryptotracelabs.com/blog/what-are-tornado-cash-deposit-patterns-and-how-are-they-analyzed-2/
- Solana privacy ecosystem overviews — https://www.kucoin.com/news/flash/solana-privacy-ecosystem-expands-with-zk-mpc-and-tee-based-solutions ; https://www.bitrue.com/blog/solana-privacy-ecosystem-tools-projects-innovations

Gap-closing follow-up (2026-07-15):
- Umbra — https://docs.umbraprivacy.com/docs/core-concepts/anonymity-layer ; https://sdk.umbraprivacy.com/introduction
- Arcium ← Elusiv lineage & Umbra launch — https://www.theblock.co/post/387564/arcium-launches-privacy-preserving-mainnet-alpha-on-solana-as-umbra-debuts-shielded-finance-layer ; https://www.arcium.com/articles/the-rebirth-of-privacy-on-solana ; https://www.gate.com/learn/articles/arcium-s-past-and-present-solana-steps-into-privacy-2-0/7770
- Privacy Cash / ORE shielded pool — https://techiexpert.com/ore-partners-with-privacycash-to-launch-official-shielded-pool-private-transfers-now-live-on-solana-mainnet/
- Confidential Transfers status (authoritative) — https://www.solana-program.com/docs/token-2022/status ; https://solana.com/docs/tokens/extensions/confidential-transfer/issuer-guide (via Solana MCP docs RAG)
