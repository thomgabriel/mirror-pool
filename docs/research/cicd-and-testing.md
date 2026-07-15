# CI/CD & Testing Strategy — mirror-pool

> **Status:** strategy / recommendation · **Date:** 2026-07-15
> **Scope:** the whole monorepo — `pool-program` + `action-adapters` (Anchor 0.31.x),
> `circuits` (circom + Groth16), `coordinator` (Rust service), `sdk` (Rust, engine-first).
> **Design basis:** [`../superpowers/specs/2026-07-15-mirror-pool-design.md`](../superpowers/specs/2026-07-15-mirror-pool-design.md) §6 (Testing strategy),
> prior art in [`prior-art.md`](./prior-art.md).

---

## 1. Intro — why this shape of CI

mirror-pool is a privacy protocol: a bug does not just crash a service, it either
loses custody of user funds or *silently breaks the anonymity guarantee*. The
bounty judges on "production-grade engineering, test rigor, and being tested e2e
after every change." That bar plus the four-layer split (on-chain program, ZK
circuits, off-chain coordinator, client SDK) drives three principles for the
pipeline:

1. **Every layer gets the strongest test type it can afford**, and the *seam*
   between layers (circuit → verifier key → program → SDK proof) is exercised by
   a single **full-stack e2e** on every PR. A green "unit tests pass" that never
   fed a real Groth16 proof into the deployed verifier is worthless here.
2. **Coverage is measured where it is meaningful (host-compiled Rust) and
   supplemented by a behavioral checklist where line-coverage is a lie (SBF
   in-VM execution).** See the SBF caveat in §3.
3. **Supply-chain and reproducibility are first-class**, because this is an
   MIT open-source custody protocol whose deployed bytecode must be provably
   the audited source (`solana-verify`) and whose ZK verifier key must be the
   ceremony output, not a dev artifact.

The pipeline is **phased** (§7) to match the design's build order — you do not
stand up circuit-e2e infrastructure before the circuits exist.

Toolchain baseline used throughout this doc (pin these; bump deliberately):

| Tool | Version | Notes |
|---|---|---|
| Anchor | `0.31.1` | matches spec's "Anchor 0.31.x"; default of `setup-anchor@v3.4` |
| Solana / Agave CLI | `2.2.x` (Agave) | `alt_bn128` + `poseidon` syscalls are live 1.18+; use a current 2.2.x |
| Rust | pinned via `rust-toolchain.toml` | one source of truth for host builds; `build-sbf` bundles its own |
| Node (circuits/JS tooling) | `20.x` | only for circom/snarkjs test harness |

---

## 2. Layered test strategy

Each component gets: a fast host-side layer (unit/property), an in-VM behavioral
layer (LiteSVM / localnet), and adversarial/negative cases. The table maps
**component → test type → tool → what it asserts**.

| Component | Test type | Tool | What it asserts |
|---|---|---|---|
| `pool-program` (pure logic: Merkle append, root ring, nullifier set, k-floor, value-conservation math) | Unit | `cargo test` / `cargo nextest` on host target | Individual functions correct; **this is the code cargo-llvm-cov can actually measure** |
| `pool-program` invariants | Property | `proptest` | `Σin + publicAmount = Σout`; nullifier uniqueness; k-floor rejects `\|batch\| < k`; Merkle append + 100-root ring correctness; vault-authority invariants (spec §6.2) |
| `pool-program` + `action-adapters` | Integration (in-VM) | **LiteSVM** (`litesvm` + `anchor-litesvm`); `solana-program-test` for full-runtime cases | Full lifecycle join→deposit→commit→execute_round→settle→withdraw; multi-member rounds; real CPI into Stake/Swap adapters |
| `pool-program` | Adversarial / negative | LiteSVM + hand-crafted inputs | Forged proof rejected; replayed nullifier rejected; sub-k round rejected on-chain; coordinator-forced thin/reordered round fails closed; `emergency_withdraw` works with coordinator offline; per-round/per-account caps enforced |
| `pool-program` | Fuzzing | **Trident** (Ackee, honggfuzz-based, Solana-aware) for instruction sequences; `cargo-fuzz`/`honggfuzz` for byte parsers (proof bytes, intent payloads) | No panic/overflow/invariant break under randomized instruction sequences and malformed proof/instruction bytes |
| `pool-program` | Reproducible build | `solana-verify build` (Docker) | Deployed bytecode == source at a given commit (see §6 release workflow) |
| `circuits` | Correctness | `circom_tester` (+ mocha/chai) or **Circomkit**; `snarkjs` | Valid witness satisfies constraints; outputs match reference vectors; **assert constraint counts** (regression guard) |
| `circuits` | Soundness (negative) | `circom_tester` expecting `assert`/unsatisfied | Invalid witnesses do **not** satisfy — unit tests alone prove correctness, *not* soundness (see §3 note), so negative cases are mandatory |
| `circuits` ↔ Rust proving | Proving round-trip | `ark-circom` (worldcoin/gakonst fork) in Rust | Rust prover produces a proof from `.wasm`/`.zkey` that `snarkjs` and `groth16-solana` both accept — the client proving path is real |
| `circuits` ↔ `pool-program` | On-chain verify | `groth16-solana` (Lightprotocol) via `alt_bn128` syscalls | Exported verifier key verifies a real proof on-chain in `<200k` CU; VK in repo matches circuit |
| `sdk` | Unit + property | `cargo nextest`, `proptest` | Note lifecycle, nullifier derivation, intent encryption, chain-scan/decrypt round-trips |
| `sdk` | Integration | LiteSVM-backed | SDK builds an intent + proof that a locally-deployed `pool-program` accepts |
| `coordinator` | Unit + property | `cargo nextest`, `proptest` | Round formation, k-floor + timing policy, stale-root retry, batch assembly, ALT logic |
| `coordinator` | API / integration | `axum`/`reqwest` test harness (or `wiremock` for RPC) | `POST /intents`, `GET /rounds/:id`, `GET /status/:req` contracts; mempool → batch behavior |
| **whole stack** | **e2e** | localnet/LiteSVM + real circuit artifacts + SDK prover | See §4 — the load-bearing test |
| **whole stack** | Anonymity (differentiator) | scenario sim + deanon heuristics (FIFO/common-input/amount) | Attribution probability `≤ 1/k` over many simulated rounds (spec §6.5) — run nightly/`workflow_dispatch`, not per-PR (slow) |

**Notes**
- **Prefer `cargo nextest`** as the runner (`taiki-e/install-action@v2` → `tool: nextest`): process-per-test, sharding for CI, automatic flaky-retry, JUnit XML for PR annotations.
- **Avoid `solana-test-validator` in the inner loop.** It is the main source of flaky localnet tests (port races, slot timing, airdrop latency). Use **LiteSVM in-process** for the bulk and reserve a single `solana-test-validator`/`anchor test` localnet job for the true e2e where full runtime semantics matter.

---

## 3. Coverage strategy — and the SBF caveat

### Tooling choice
Use **`cargo-llvm-cov`** (`taiki-e/cargo-llvm-cov`, LLVM source-based
`-C instrument-coverage`) over `cargo-tarpaulin`: better source mapping, broader
platform support, first-class `--lcov`/Codecov output, and it is the de-facto
2025–2026 default. Install prebuilt via `taiki-e/install-action@cargo-llvm-cov`.

### The SBF caveat (read this before setting a % gate)
`cargo llvm-cov` builds and runs tests on the **host** target with LLVM coverage
instrumentation. But LiteSVM and `solana-program-test` load your program as a
compiled **SBF ELF** (`cargo build-sbf`) and execute it inside the **SBF VM**.
The SBF build does not emit, and the SBF VM does not run, the LLVM coverage
runtime. **Therefore lines executed *inside the program under test* during an
in-VM integration test are not counted by cargo-llvm-cov.** llvm-cov faithfully
measures only host-compiled code: the SDK, the coordinator, the Rust proving
path, and any `pool-program` logic you invoke **directly as host library
functions** (i.e., pure `pub fn`s factored out of the `#[program]` entrypoint).

**Design implication (do this):** push every invariant — Merkle/ring math,
nullifier checks, k-floor, value-conservation, cap arithmetic — into plain
`pub fn` library code with `#[cfg(test)]` host tests. That code is both the
security-critical part *and* the part llvm-cov can measure. The thin
`#[program]` handlers then just wire accounts to those functions.

**For real SBF line-coverage** (optional, later): **LimeChain `sbpf-coverage`**
maps SBPF execution traces to source via DWARF debug info — the only tool that
reports coverage of code actually running in the VM. Treat it as a nightly/
report-only signal, not a PR gate (it is heavier and less mature than llvm-cov).

### Per-layer targets & gating

| Layer | Coverage tool | What the % means | Target | Gate? |
|---|---|---|---|---|
| `pool-program` **library logic** (host tests) | cargo-llvm-cov | real line/branch coverage of invariant code | **90%+** lines on the invariant modules | Yes (patch gate) |
| `pool-program` **in-VM behavior** | LiteSVM tests + explicit test-case checklist | scenarios covered, not lines | every row in §2 adversarial list green | Yes (required job, not a %) |
| `sdk` | cargo-llvm-cov | real | **80%+** | Yes |
| `coordinator` | cargo-llvm-cov | real | **75%+** | Yes |
| `circuits` | constraint-count assertions + pos/neg vectors | scenarios, not lines | 100% of documented signals have a positive **and** negative test | Yes (required job) |

**Codecov** via `codecov/codecov-action@v5`, uploading merged LCOV from the
workspace. Start `project`/`patch` targets as **informational** for ~2 weeks to
establish a baseline, then flip to blocking. Example `codecov.yml`:

```yaml
coverage:
  status:
    project:
      default: { target: auto, threshold: 1% }   # don't regress
    patch:
      default: { target: 80% }                    # new code must be covered
ignore:
  - "**/tests/**"
  - "**/*.gen.rs"
comment: { layout: "reach, diff, flags" }
```

Use **Codecov flags** (`pool-program`, `sdk`, `coordinator`) so each layer's
target is enforced independently rather than diluted into one repo-wide number.

---

## 4. E2E after every change — the full-stack test

The single most important test: prove that a **real** proof flows through the
**real** deployed verifier and mutates on-chain state correctly. It exercises
the exact seam that unit tests cannot: `circuit → verifier key → program → SDK`.

### What the e2e does (one deterministic round)
1. **Build circuit artifacts** — compile the membership/value-conservation
   circuit with `circom` → `.r1cs` + `.wasm`; run Groth16 setup against a
   **small, committed test `.ptau`** (a fixed low-power tau used *only* for
   tests — **never** the production ceremony output) → `.zkey`; export
   `verification_key.json`.
2. **Wire the VK into the program** — codegen the verifier key the
   `pool-program` compiles in, and assert it equals the just-exported VK (guards
   "circuit changed, program not rebuilt").
3. **Build + deploy** — `anchor build`; deploy to **LiteSVM** (fast path, per-PR)
   or `solana-test-validator` (full-runtime nightly).
4. **Prove client-side** — the **SDK** creates a note, appends a commitment,
   and generates a **real Groth16 proof** with `ark-circom` from the `.wasm`/
   `.zkey`.
5. **Submit + assert** — build `k` intents, call `execute_round`; assert:
   proof verified on-chain, nullifiers marked, **k-floor enforced** (a `k-1`
   batch is rejected), output note commitments appended, vault balances conserved.

### Keeping it fast & deterministic
- **Test-scale parameters:** small Merkle height and small `k` for the CI
  circuit (the production tree is height 32 — do not prove that in CI).
- **Cache circuit artifacts:** the `.zkey`/`.wasm` build is the slow step. Cache
  it in Actions keyed on a hash of the `.circom` sources + `.ptau`, so it only
  recompiles when the circuit changes.
- **Fixed seeds / fixed test ptau** → byte-reproducible proofs; no network, no
  airdrops, no wall-clock timing (LiteSVM lets you set slots/clock directly).
- **Determinism guard:** run the e2e twice in the nightly job and diff the
  resulting on-chain state to catch nondeterminism early.

### How CI runs it
A dedicated `e2e` job (see §5 workflow) that `needs: [build, circuits]` and
consumes the cached circuit artifacts. It is a **required check** for merge.
The heavy full-`solana-test-validator` + anonymity-heuristic variant runs on a
nightly `schedule` and on `workflow_dispatch`, not on every PR, to keep PR
latency low while still honoring "tested e2e after every change" on the fast path.

---

## 5. `.github/workflows/ci.yml` (copy-pasteable)

Concrete, pinned, and structured so early phases can comment-out the jobs whose
components don't exist yet (§7). Pin third-party actions to a tag (shown) or a
commit SHA for stricter supply-chain hygiene.

```yaml
name: ci
on:
  push:
    branches: [main]
  pull_request:
  workflow_dispatch:

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1
  ANCHOR_VERSION: "0.31.1"
  SOLANA_VERSION: "2.2.21"

jobs:
  # ---------- 1. Lint / format (fast fail) ----------
  fmt-clippy:
    name: fmt + clippy
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets --all-features -- -D warnings

  # ---------- 2. Host unit + property tests + coverage ----------
  test-coverage:
    name: unit/prop tests + coverage
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools-preview
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-llvm-cov,cargo-nextest
      # Host-target coverage: SDK, coordinator, and pool-program *library* logic.
      # (In-VM SBF execution is NOT captured here — see docs §3.)
      - name: Test with coverage
        run: cargo llvm-cov nextest --workspace --all-features --lcov --output-path lcov.info
      - uses: codecov/codecov-action@v5
        with:
          files: lcov.info
          token: ${{ secrets.CODECOV_TOKEN }}
          fail_ci_if_error: true

  # ---------- 3. Anchor build + in-VM (LiteSVM) integration tests ----------
  anchor-build-test:
    name: anchor build + litesvm integration
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: metadaoproject/setup-anchor@v3.4
        with:
          anchor-version: ${{ env.ANCHOR_VERSION }}
          solana-cli-version: ${{ env.SOLANA_VERSION }}
          node-version: "20"
      - uses: Swatinem/rust-cache@v2
      - name: Anchor build (SBF)
        run: anchor build
      - name: Upload program .so
        uses: actions/upload-artifact@v4
        with:
          name: program-so
          path: target/deploy/*.so
      # LiteSVM tests are plain Rust integration tests that load the built .so.
      - name: LiteSVM integration + adversarial tests
        run: cargo nextest run -p pool-program --test '*' --profile ci

  # ---------- 4. Circuit tests (correctness + soundness + constraint counts) ----------
  circuits:
    name: circom circuit tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: "20", cache: "npm", cache-dependency-path: circuits/package-lock.json }
      - name: Install circom
        run: |
          curl -L https://github.com/iden3/circom/releases/latest/download/circom-linux-amd64 -o /usr/local/bin/circom
          chmod +x /usr/local/bin/circom
      - name: Install & test
        working-directory: circuits
        run: |
          npm ci
          npm test           # circom_tester/circomkit: positive + negative vectors + constraint-count asserts
      - name: Build + cache proving artifacts (zkey/wasm/vk) for e2e
        working-directory: circuits
        run: npm run build:test-artifacts   # uses committed small test .ptau
      - uses: actions/cache@v4
        with:
          path: circuits/build
          key: circuit-artifacts-${{ hashFiles('circuits/**/*.circom', 'circuits/ptau/test.ptau') }}
      - uses: actions/upload-artifact@v4
        with: { name: circuit-artifacts, path: circuits/build }

  # ---------- 5. Full-stack e2e: real proof -> deployed verifier -> assert ----------
  e2e:
    name: e2e (real Groth16 through deployed program)
    runs-on: ubuntu-latest
    needs: [anchor-build-test, circuits]
    steps:
      - uses: actions/checkout@v4
      - uses: metadaoproject/setup-anchor@v3.4
        with:
          anchor-version: ${{ env.ANCHOR_VERSION }}
          solana-cli-version: ${{ env.SOLANA_VERSION }}
          node-version: "20"
      - uses: Swatinem/rust-cache@v2
      - uses: actions/download-artifact@v4
        with: { name: circuit-artifacts, path: circuits/build }
      - name: Assert in-repo verifier key matches circuit export
        run: cargo run -p xtask -- check-vk   # fail if program VK != circuits/build/verification_key.json
      - name: Run full-stack e2e (SDK proves, pool-program verifies on LiteSVM)
        run: cargo nextest run -p e2e --profile ci
      # Nightly variant (solana-test-validator + anonymity heuristics) runs on schedule, see ci-nightly.yml.

  # ---------- 6. Security / supply chain ----------
  supply-chain:
    name: cargo-deny + audit
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: EmbarkStudios/cargo-deny-action@v2
        with:
          command: check advisories bans licenses sources
      # cargo-deny already covers RustSec advisories; keep audit-check as a
      # scheduled cron in a separate workflow if you want issue auto-filing.
```

### Notes on the workflow
- **`Swatinem/rust-cache@v2` goes *after* the toolchain step** — it keys the
  cache on the active `rustc`, so it must see the pinned toolchain first.
- **`setup-anchor@v3.4`** installs Anchor + Agave CLI + Node in one step; its
  defaults (Anchor `0.31.1`, Solana `2.2.21`, Node `20.11.0`) already match our
  baseline, but we pin explicitly via `env` so a default bump can't drift CI.
- **`taiki-e/install-action`** fetches prebuilt binaries (no `cargo install`
  compile cost) for `cargo-llvm-cov`, `cargo-nextest`, and (below) `cargo-deny`.
- **`--profile ci`** refers to a nextest CI profile (retries + JUnit) in
  `.config/nextest.toml`.
- A **job matrix** is deliberately *not* used for OS here — Solana programs
  target Linux SBF, so multi-OS adds cost without signal. Matrix is worth it
  only if the SDK ships as a cross-platform library (then matrix the `sdk` crate
  over `ubuntu`/`macos`/`windows` on the host-test job only).

### Second workflow — release / verifiable build (`.github/workflows/release.yml`)

Warranted because this is a custody program: deployed bytecode must be provably
the source, and releases should be automated from conventional commits.

```yaml
name: release
on:
  push:
    branches: [main]
    tags: ["v*"]

jobs:
  # Rust-native release PRs (version bump + changelog) from conventional commits.
  release-pr:
    if: github.ref == 'refs/heads/main'
    runs-on: ubuntu-latest
    permissions: { contents: write, pull-requests: write }
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: release-plz/action@v0.5      # cargo-semver-checks aware; Rust workspace native
        with: { command: release-pr }
        env: { GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }} }

  # On a version tag: reproducible build + verify deployed program matches source.
  verifiable-build:
    if: startsWith(github.ref, 'refs/tags/v')
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Deterministic build (Docker)
        run: |
          cargo install solana-verify
          solana-verify build --library-name pool_program
      - name: Verify on-chain match (OtterSec API)
        uses: solana-foundation/github-actions/verify-build@v0.2.11
        with:
          program-id: ${{ vars.POOL_PROGRAM_ID }}
          # network: mainnet-beta; upgrade authority stays on Squads multisig (spec §3.1)
```

> `release-plz` is preferred over `release-please` for a **Rust workspace**
> because it understands `Cargo.toml`, publishes crates, and runs
> `cargo-semver-checks` to catch breaking API changes. If you prefer Google's
> tool, `googleapis/release-please-action@v4` also supports Rust.

---

## 6. Security & supply-chain

| Concern | Tool / action | In CI |
|---|---|---|
| Known vulns (RustSec) | `cargo-deny check advisories` via `EmbarkStudios/cargo-deny-action@v2` | PR + nightly |
| Licenses / banned crates / source allow-list | `cargo-deny check licenses bans sources` (`deny.toml`) | PR |
| Vuln issue auto-filing / scheduled scan | `rustsec/audit-check@v2` (`cargo-audit`) on `schedule` | nightly cron |
| Dependency review (human-audited supply chain) | `cargo-vet` (`cargo vet`) — record trusted audits in `supply-chain/` | PR (advisory → gate as it matures) |
| Solana-specific static analysis | **sec3 `x-ray`** (open-source, 50+ vuln classes, CI-friendly); optionally `l3x` (AI), Auditless `radar`, `solana-fender` | PR (start non-blocking) |
| Generic SAST | `semgrep` (`returntocorp/semgrep` + rust/solana rules) | PR |
| Locked dependencies | commit `Cargo.lock`; `--locked` in CI builds | always |
| Pinned actions | pin third-party actions to a tag **or commit SHA**; enable Dependabot for actions + cargo | always |
| **ZK trusted-setup artifact** | commit the **production verifier key** + its ceremony transcript hash; CI asserts the program-embedded VK matches the checked-in VK and its recorded checksum; the small **test `.ptau`** is clearly separated and never used for a release | PR (`xtask check-vk`) + release |

**`deny.toml` starter:**
```toml
[advisories]
version = 2
yanked = "deny"
[licenses]
version = 2
allow = ["MIT", "Apache-2.0", "BSD-3-Clause", "Unicode-3.0", "ISC"]
[bans]
multiple-versions = "warn"
wildcards = "deny"
[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

**ZK note.** The trusted-setup ceremony (spec §5) is a *release* concern, not a
per-PR one. The pipeline's job is to guarantee the deployed program verifies
against **the** ceremony VK: keep the VK and a `transcript.sha256` in the repo,
and let `xtask check-vk` fail the build if the compiled-in key drifts. Toxic-waste
handling and the multi-party ceremony itself live in the launch runbook.

---

## 7. Phased rollout (matches design build order)

CI grows with the codebase — do not build circuit-e2e infra before circuits exist.

**Phase 1 — `pool-program` core + minimal circuits + minimal SDK** (add now):
- `fmt-clippy` (deny warnings), `test-coverage` (host unit+property, llvm-cov),
  `anchor-build-test` (LiteSVM integration + the adversarial checklist),
  `supply-chain` (cargo-deny).
- Branch protection on `main`: require `fmt-clippy`, `test-coverage`,
  `anchor-build-test`, `supply-chain`; linear history; no direct pushes.
- Pre-commit hooks (below) + conventional commits.
- *Coverage gate:* informational first, then enforce the `pool-program` library
  target (§3). *Defer:* full circuit e2e, verifiable-build (no deploy yet).

**Phase 2 — round engine + coordinator:**
- Add `coordinator` unit/property/API tests to `test-coverage`; add its Codecov
  flag + target. Add the `e2e` job (LiteSVM full lifecycle, real proof) as
  required. Add `sec3 x-ray` (non-blocking).

**Phase 3 — `PooledAction` adapters (stake/swap):**
- Extend `anchor-build-test` with CPI adapter integration tests (LiteSVM with
  cloned mainnet adapter accounts / mocked target programs). Add **Trident**
  fuzzing (nightly). Promote `x-ray` and `cargo-vet` to blocking.

**Phase 4 — incentives + viewing-key disclosure:**
- Property tests for bond/slash/cover-reward accounting; disclosure round-trip
  tests in the SDK. Add the **anonymity test** job (`assert P(attribution) ≤ 1/k`)
  on the nightly schedule.

**Phase 5 — hardening + indexer + audit + ceremony:**
- Turn on `release.yml`: `release-plz` + `solana-verify` verifiable build +
  `verify-build` against the deployed program ID (Squads-controlled upgrade
  authority). Wire the real trusted-setup VK + transcript checksum into
  `check-vk`. Add `sbpf-coverage` nightly report for true SBF line coverage.

### Repo hygiene (all phases)
- **`rustfmt` + `clippy -D warnings`** enforced in CI and locally.
- **pre-commit** (`.pre-commit-config.yaml`) running `cargo fmt`, `cargo clippy`,
  `cargo-deny`, and `commitlint`/conventional-commit check locally so red never
  reaches CI:
  ```yaml
  repos:
    - repo: https://github.com/doublify/pre-commit-rust
      rev: v1.0
      hooks: [{ id: fmt }, { id: clippy, args: ["--", "-D", "warnings"] }]
    - repo: local
      hooks:
        - id: cargo-deny
          name: cargo-deny
          entry: cargo deny check advisories bans
          language: system
          pass_filenames: false
  ```
- **Conventional commits** → feeds `release-plz` changelog/versioning.
- **Branch protection / required checks** as listed per phase; PRs need a green
  pipeline + one review before merge (custody code).

---

## Sources

- [metaDAOproject/setup-anchor (v3.4; defaults Anchor 0.31.1 / Solana 2.2.21 / Node 20)](https://github.com/metaDAOproject/setup-anchor) · [setup-solana](https://github.com/metaDAOproject/setup-solana) · [anchor-test](https://github.com/metaDAOproject/anchor-test)
- [Anchor — installation & AVM](https://www.anchor-lang.com/docs/installation) · [Anchor LiteSVM testing guide](https://www.anchor-lang.com/docs/testing/litesvm)
- [LiteSVM (docs.rs)](https://docs.rs/litesvm/latest/litesvm/) · [anchor-litesvm](https://crates.io/crates/anchor-litesvm) · [QuickNode: testing with LiteSVM](https://www.quicknode.com/guides/solana-development/tooling/litesvm)
- [taiki-e/cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) · [Rust Project Primer — coverage](https://rustprojectprimer.com/measure/coverage.html) · [cargo-tarpaulin](https://crates.io/crates/cargo-tarpaulin)
- [LimeChain/sbpf-coverage (SBF DWARF-based coverage)](https://github.com/LimeChain/sbpf-coverage)
- [codecov/codecov-action@v5 usage (via cargo-llvm-cov README)](https://github.com/taiki-e/cargo-llvm-cov/blob/main/README.md) · [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache)
- [cargo-nextest](https://nexte.st/) · [nextest-rs/nextest](https://github.com/nextest-rs/nextest)
- [Trident — Solana fuzzer (Ackee, honggfuzz-based)](https://github.com/Ackee-Blockchain/trident) · [Introducing Trident](https://ackee.xyz/blog/introducing-trident-the-first-open-source-fuzzer-for-solana-programs/) · [QuickNode: Trident fuzzing](https://www.quicknode.com/guides/solana-development/tooling/trident-fuzzing)
- [iden3/circom_tester](https://github.com/iden3/circom_tester) · [Circomkit](https://github.com/erhant/circomkit) · [Circom 2 docs](https://docs.circom.io/) · [Berkeley ZKP MOOC lab — correctness vs soundness testing](https://github.com/rdi-berkeley/zkp-mooc-lab)
- [ark-circom (worldcoin fork)](https://github.com/worldcoin/circom-compat) · [arkworks-rs/circom-compat](https://github.com/arkworks-rs/circom-compat) · [Lightprotocol/groth16-solana](https://github.com/Lightprotocol/groth16-solana) · [groth16-solana (crates.io, <200k CU / 1.18+)](https://crates.io/crates/groth16-solana) · [Solana Groth16 on-chain example](https://github.com/wkennedy/solana-zk-proof-example)
- [EmbarkStudios/cargo-deny + action](https://github.com/EmbarkStudios/cargo-deny) · [RustSec advisory DB](https://rustsec.org/) · [cargo-audit](https://crates.io/crates/cargo-audit) · [Rust/Cargo supply-chain security guide](https://www.systemshardening.com/articles/cicd/rust-cargo-supply-chain-security/)
- [sec3-product/x-ray (Solana static analysis)](https://github.com/sec3-product/x-ray) · [honey-guard/solana-fender](https://github.com/honey-guard/solana-fender) · [The Solana Security Toolbox in 2026](https://dev.to/ohmygod/the-solana-security-toolbox-in-2026-a-practitioners-guide-to-fuzzing-static-analysis-and-5h7f)
- [solana-verify / solana-verifiable-build](https://github.com/solana-foundation/solana-verifiable-build) · [Solana verified builds docs](https://solana.com/docs/programs/verified-builds) · [solana-foundation/github-actions (verify-build, build-anchor, idl-upload; v0.2.11)](https://github.com/solana-developers/github-actions)
- [release-plz (Rust-native release automation)](https://release-plz.dev/) · [release-plz crate](https://crates.io/crates/release-plz) · [googleapis/release-please](https://github.com/googleapis/release-please)
- [Running Anchor Tests on GitHub Actions](https://dev.to/burgossrodrigo/running-anchor-tests-on-github-actions-without-losing-your-mind-20j0)
