# Handoff: `zk-tests` CI job fails to link on Linux x86_64 (wasmer `__rust_probestack`)

> **Status:** OPEN. 5 of 6 CI jobs are green; only `zk proof tests` fails.
> **Scope:** this is a **CI-environment linker** problem, NOT a code bug — the
> proof/verify/e2e tests pass locally on macOS. Do not "fix" the Rust code.
> **Repo:** `mirror-pool` (Rust-only Solana privacy protocol). Branch: `main`.

## TL;DR

The `zk-tests` job builds a test binary that links `wasmer` (pulled transitively
by `ark-circom` 0.5's witness generator). `wasmer_vm` has a hand-written asm
function `wasmer_vm_probestack` that references the symbol `__rust_probestack`.
On the **Linux x86_64** GitHub runner the linker (`rust-lld`, the Rust default)
reports:

```
rust-lld: error: undefined symbol: __rust_probestack
    >>> referenced by libcalls.rs:668
    >>> wasmer_vm-<hash>.wasmer_vm.<hash>-cgu.15.rcgu.o:(wasmer_vm_probestack)
        in archive .../libwasmer_vm-<hash>.rlib
collect2: error: ld returned 1 exit status
```

It passes locally because **local dev is macOS** (Apple `ld64` resolves it), and
`__rust_probestack` is an **x86_64-specific** stack-probe symbol (on aarch64 it's
effectively absent/no-op, so it never surfaces there).

## Why the obvious fixes did not work

| Attempt | RUSTFLAGS / change | Result |
|---|---|---|
| Force GNU ld | `-Clink-self-contained=-linker` alone | `collect2: cannot find 'ld'` — rustc still requested the `lld` flavor (`-fuse-ld=lld`) and no system lld exists on the runner. |
| Force bfd | `-Clink-self-contained=-linker -Clink-arg=-fuse-ld=bfd` | `/usr/bin/ld.bfd: ... wasmer_vm ... some extern functions couldn't be found` — GNU ld *also* couldn't resolve `__rust_probestack` (no `--undefined` yet). |
| Force-undefined the symbol | `-Clink-arg=-Wl,--undefined=__rust_probestack` (rust-lld) | Still `rust-lld: error: undefined symbol: __rust_probestack`. `--undefined`/`-u` did **not** pull it from `libcompiler_builtins` under rust-lld on x86_64. |
| Pin Rust toolchain | `rust-toolchain.toml` → `1.92.0` (matches local) | No change to the link error (confirmed it's not a rustc-version-dropped-symbol issue; 1.92 on Linux behaves like stable). Kept the pin anyway — good reproducibility hygiene. |

### The misleading "local success"

`-Clink-arg=-Wl,--undefined=__rust_probestack` **did** let the `prover` test
binary link inside a **local `rust:1.92` Docker container** — but that container
is **aarch64** (Apple Silicon). On aarch64 the probestack symbol isn't required,
so that success was not representative of the x86_64 runner. **Any future local
repro MUST use `--platform linux/amd64`** (qemu-emulated x86_64), which is slow
(~6× native) but faithful. Do not trust an aarch64 container for this bug.

## Reproduce faithfully (x86_64)

```bash
# From the repo root. Emulated x86_64; debuginfo=0 keeps the link under the
# ~2 GiB Docker VM default (the debug link OOMs otherwise — `ld ... signal 9`).
docker run --rm --platform linux/amd64 \
  -v "$PWD":/work -w /work \
  -v mp-cargo-x86:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/tmp/t \
  rust:1.92 bash -c \
  'RUSTFLAGS="-Cdebuginfo=0 <candidate flags>" cargo test -p prover --no-run 2>&1 | tail -20'
```
`cargo test -p prover --no-run` is enough — it links the wasmer-containing test
binary without needing circom/ptau artifacts. If it prints `Executable ...` the
candidate flags work; iterate flags here, then apply to `.github/workflows/ci.yml`.

## Candidate next steps (ranked, untried on x86_64)

1. **GNU ld + force-undefined together:** `-Clink-self-contained=-linker
   -Clink-arg=-fuse-ld=bfd -Clink-arg=-Wl,--undefined=__rust_probestack`. bfd was
   tried without `--undefined`, and `--undefined` was tried without bfd — the
   *combination* hasn't run on x86_64. Verify `/usr/bin/ld.bfd` exists in the
   `rust:1.92` image (it does; debian binutils).
2. **Confirm the symbol even exists** in x86_64 1.92's compiler-builtins:
   `nm -A $(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/lib/libcompiler_builtins-*.rlib | grep probestack`.
   If it's absent, no linker flag helps — the fix must come from wasmer/Rust
   version alignment (option 4).
3. **`-Clink-args=-Wl,-u,__rust_probestack -Clink-args=-Wl,--no-gc-sections`** —
   in case `--gc-sections` is dropping the forced symbol.
4. **Toolchain/dep alignment:** the real root cause is old `wasmer` (pinned by
   `ark-circom = 0.5`, which the project intentionally pins — bumping to 0.6
   splits the arkworks major and breaks `groth16-solana` 0.2 interop, so DON'T).
   An older Rust that predates the probestack ABI change *might* link; needs a
   bisection and must not regress the other jobs.
5. **Run this job on `macos-latest`** where the link works (as local dev does).
   Cost: the `install-circom` / `fetch-ptau` composite actions
   (`.github/actions/*`) are currently Linux-oriented and would need macOS
   variants — potential new yak-shave.
6. **Pragmatic unblock:** mark the job `continue-on-error: true` with a TODO. It
   still runs and reports proof/verify/e2e results; the pinned-dep linker quirk
   just stops gating the pipeline. The code stays validated locally + by the
   green non-proof LiteSVM job.

## What IS fixed and green (don't re-investigate these)

- **`build (sbf) + non-proof litesvm integration`**, **`host unit tests`**,
  **`fmt + clippy`**, **`check-vk`** — all green.
- **`cargo-deny`** — green. The workflow previously pinned a **non-existent
  Solana version** (`SOLANA_VERSION: 2.2.21` → 404), which broke
  `metadaoproject/setup-anchor@v3.4`. Replaced with a direct Agave install
  (`release.anza.xyz/v3.0.1/install`) + `cargo build-sbf` (no Anchor CLI needed —
  the LiteSVM tests hand-build instructions, no IDL). `deny.toml` was tuned:
  `[bans] allow-wildcard-paths = true` + all workspace crates `publish = false`
  (fixes the intra-workspace path-dep wildcards), `[advisories] unmaintained =
  "workspace"`, and 6 transitive CVEs ignored with per-ID justifications (all in
  the client-side proving/tooling tree — curve25519/ed25519-dalek via
  solana/wasmer, 3× webpki + ANSI-log via `ethers-core -> reqwest`; none reachable
  from the on-chain custody path). Verified locally with `cargo deny check
  advisories bans licenses sources` → all ok.
- **`rust-toolchain.toml`** pins the workspace to Rust **1.92.0** (matches local
  dev; stops `@stable` from drifting lints/symbols).

## Key facts for whoever picks this up

- CI: Linux **x86_64**, rust-lld default. Local dev: macOS **aarch64**. This arch
  gap is the whole trap.
- Rust pinned to **1.92.0** via `rust-toolchain.toml`.
- `ark-circom = "0.5"` is a HARD pin (see plan Global Constraints) → old `wasmer`
  → the `__rust_probestack` reference. Bumping ark-circom is off the table.
- The failing step is `cargo test -p pool-program --test withdraw --test verifier`
  (and the subsequent `-p prover` / `-p sdk` steps) in the `zk-tests` job of
  `.github/workflows/ci.yml`. The RUSTFLAGS are currently set for that job via a
  `Force-link __rust_probestack ...` step writing to `$GITHUB_ENV` — that step is
  the place to change the flags.
