# mirror-pool — Pooled Stake (Plan 5) Design Spec

> **Status:** draft for review · **Date:** 2026-07-16
> **Builds on:** Plan 4 (behavioral rounds), merged to `main`. Extends the master
> architecture spec ([`2026-07-15-mirror-pool-design.md`](2026-07-15-mirror-pool-design.md)) §3.1
> "action-adapters" and §2 phase 3 ("PooledAction adapters → behavioral pooling — the novel core").
> **Informed by:** the verified follow-up research
> ([`docs/research/2026-07-16-behavioral-rounds-followup-proposal.md`](../../research/2026-07-16-behavioral-rounds-followup-proposal.md))
> and its adversarial critic ("build the second pooled *action*, not a sixth privacy layer on the exit").

---

## 1. Purpose & thesis

Plan 4 shipped a k-anonymous round engine whose only action is `Withdraw` — an *exit*.
The bounty's mission, though, is **behavioral obscurity of on-chain *actions*** ("Tornado
Cash for synchronized actions, not for funds"). Plan 5 delivers the **second `PooledAction`
adapter — pooled staking** — the first action that is *not* an exit: k participants each
delegate a bucketed amount to a fixed validator, executed as one vault-signed batch, so an
observer sees "the pool staked to validator V" and cannot tell **which** participant built
which staking position.

Adding a second action is also what finally makes the **`PooledAction` seam real**: Plan 4
built the trait with one impl; Plan 5 forces genuine dispatch over two action kinds and is the
proof that "adding a protocol = one adapter" holds.

**Honest scope of the privacy claim.** Pooled stake demonstrates *behavioral action
obscurity* (hide who is accumulating a staking position; uniform actor; fresh unlinkable
authority) — a real step past a withdrawal mixer. It is **not** the copy-trade/front-run
headline: those are attacks on *trades*, which is the pooled **swap** — deferred here because a
Jupiter CPI's ~10-20 accounts/intent collapses a single-tx round to k=2-3 (a useless set) until
a chunked executor exists. Stake is the account-light, uniform-action-shape first behavioral action;
swap graduates in once the envelope allows.

### Locked design decisions

| Decision | Choice | Rationale |
|---|---|---|
| First behavioral action | **Pooled Stake** (delegate to a fixed validator) | Uniform action-shape + ~3 accounts/intent (intent PDA, stake_account PDA, relayer) ⇒ k ≈ 17 (similar to withdraw), a real crowd; the spec's named "novel core"; swap/vote deferred |
| Circuit | **Reuse the existing withdraw circuit** | `extDataHash` is a generic binding; it binds the *stake* params instead of (recipient/relayer/fee). Zero circuit/VK/trusted-setup work. |
| Intent model | **Reused as-is** (`{recipient, relayer, fee}`) | For stake, `recipient` = the participant's proof-bound **stake authority**; `fee`→relayer; `denomination − fee − stake_rent` delegated (§6). No intent rewrite. |
| Action locus | **Per-pool** (a pool is one `action_kind`) | A round must be single-kind for action-shape uniformity — a withdraw and a stake in one batch are trivially distinguishable. Simplest: the kind lives on the `Pool`. |
| Validator | **Fixed per pool** (set at `initialize_pool`) | Byte-uniformity + the validator/sysvars become *shared* batch accounts (the account-light win). Per-intent validator would be distinguishable and account-heavy. |
| Lifecycle | **Delegate-only** | The stake account's authorities = the participant's proof-bound key, so they undelegate/reclaim themselves later. Pooled un-stake + silent reward accrual are the deferred incentive module. |
| Effective-k harness | **Deferred to Plan 6** | Host-side, pure-Rust, no custody; measures both withdraw and stake rounds. Keeps Plan 5 a single custody-focused deliverable. |

---

## 2. Architecture

Plan 5 is entirely additive to the merged round engine — same `commit_intent` → round →
`execute_round`/`cancel_intent` lifecycle, same k-floor, same nullifier/replay guarantees. The
new surface is: the `Pool` declares an action kind + (for stake) a validator; a second
`PooledAction` impl; and `execute_round` dispatching + parsing `remaining_accounts` per kind.

```
initialize_pool(denomination, k_floor, action_kind, validator?)   ← +action_kind, +validator
        │
        ▼
commit_intent(proof, root, nullifier_hash, fee, round_id)         ← unchanged; extDataHash binds
        │                                                            (authority, relayer, fee)
        ▼   [Round Open, accumulating]
execute_round(round_id)                                           ← dispatches on pool.action_kind:
        │                                                            Withdraw → WithdrawAction (unchanged)
        │                                                            Stake    → StakeAction  (NEW)
        ▼   one vault-signed batch, uniform action-shape, k-floor-gated
```

### 2.1 On-chain changes

**`Pool` (state.rs).** Add `action_kind: ActionKind` (u8) and `validator: Pubkey` (the stake
pool's target vote account; `Pubkey::default()` / unused for withdraw pools). Placed with
explicit padding to preserve the no-implicit-padding invariant (bytemuck `Pod`) and keep
`size_of::<Pool>()` a multiple of 8 — same discipline as Plan 4's `k_floor`/`current_round_id`.

**`initialize_pool`.** Gains `action_kind` + `validator` args. For `Stake`, require a non-default
`validator`; for `Withdraw`, require it be default (or ignore). All existing callers updated
(the sweep, as in Plan 4 — but this time additively: withdraw pools pass `action_kind = Withdraw`).

**`action.rs` — the seam made real.**
- `ActionKind { Withdraw, Stake }` (Stake is the new variant, appended).
- `StakeAction` impl of `PooledAction`: vault-signed, per intent — **4 CPIs**, ordered so the
  vault can act unilaterally (the participant's key is never present at execute):
  1. `SystemProgram::CreateAccount` for a program-derived **stake account** PDA
     (`["stake", pool, nullifier_hash]`), sized/rent-funded for a stake account, owned by the
     Stake program (`create_account` with `owner = stake::program::ID`) — vault signs as funder
     and the PDA signs for itself via its seeds.
  2. `StakeProgram::Initialize` with **staker = the VAULT PDA**, **withdrawer = `intent.recipient`**
     (the participant's fresh, proof-bound key), `Lockup::default()`. The vault holds the *staker*
     authority initially — **required**, because `DelegateStake` demands the staker *sign*, and only
     the vault is present; the participant holds *withdraw* from t=0.
  3. `StakeProgram::DelegateStake` to the pool's `validator` — the **vault signs as the staker**.
  4. `StakeProgram::Authorize(StakerAuthorize: VAULT → intent.recipient)` — the vault signs and
     hands staking control to the participant, who now holds **both** authorities (staker +
     withdrawer). `Authorize` needs only `[stake, clock, current-authority]` — all already present —
     so it adds **no new per-intent account slots**; k ≈ 17 holds.
  5. `fee → relayer` (as in withdraw).
  Amount delegated = `denomination − fee − stake_rent` (the note's value, less the relayer fee,
  less the stake account's own rent-exemption, which is locked in the account and recoverable when
  the participant later closes it). `split_payout` (reused) keeps the arithmetic fail-closed. **The
  delegated amount MUST clear the Stake program's minimum delegation** (see §6 — a hard validity
  precondition on the pool's denomination bucket).

**`execute_round` (lib.rs).** Read `pool.action_kind`; dispatch each intent to the matching
`PooledAction`. The `remaining_accounts` layout is **per-kind**:
- `Withdraw`: `[intent, recipient, relayer] × k` (unchanged).
- `Stake`: `[intent, stake_account, recipient(authority), relayer] × k`, plus the **shared**
  tail: the pool's `validator` vote account, Stake program, Stake config, Clock sysvar,
  StakeHistory sysvar, Rent sysvar (validated once, reused for every intent).
  The per-intent binding guards are unchanged in spirit: `intent.pool == pool`, `intent.round_id
  == round_id`, dedup, and the payout/authority accounts key-matched against the stored `Intent`
  (`recipient`, `relayer`), so redirection is impossible. The stake-account PDA is verified by its
  seeds (`["stake", pool, nullifier_hash]`), binding it to the committed intent.

**`commit_intent` / `cancel_intent`.** Unchanged logic. `commit_intent` binds
`extDataHash(recipient, relayer, fee)` and records the `Intent` exactly as today (the *pool*
determines that these are stake params, not withdraw params). `cancel_intent` refunds the
denomination to the authority while the round is Open; the nullifier stays burned. Both work
generically because the intent shape is action-agnostic.

### 2.2 Account-envelope budget (verified constraint)

Per stake intent: **3 writable accounts** — the `intent` PDA, the `stake_account` PDA, and the
`relayer` (fee recipient). The **stake/withdraw authority is instruction *data* to Stake
`Initialize`, not a passed account** (read from the stored `Intent.recipient` and bound via
`extDataHash`), so it consumes no per-intent slot. Against ~6 fixed (`pool, round, next_round,
vault, cranker, system_program`) + ~6-7 shared (validator vote account, Stake program, Stake
config, Clock/StakeHistory/Rent sysvars). Round capacity ≈ `(64 − ~13) / 3 ≈ 17` — the same
order as withdraw (3/intent), NOT higher. This is the whole k-ceiling for a single-tx stake
round; chunked/paginated execution (raising it) is deferred with the swap.

### 2.3 SDK

- `build_initialize_pool_ix` gains `action_kind` + `validator`.
- `build_commit_intent_ix` unchanged (the note/proof/binding are identical).
- `build_execute_round_ix` gains the per-kind `remaining_accounts` assembly (stake pools append
  the stake-account PDAs + the shared validator/sysvar tail).
- A `build_stake_pool_ix` helper (or a parameter on the existing initializer) and a
  `stake_account_pda(pool, nullifier_hash)` derivation helper.

---

## 3. Data flow (one pooled-stake round)

```
① INIT     initialize_pool(denomination D, k_floor, action_kind = Stake, validator V)
② DEPOSIT  value D → vault; append note commitment to the tree (unchanged)
③ COMMIT   client proves note ownership; extDataHash binds (stake_authority A, relayer R, fee f);
           submit {proof, N, A, R, f} → records Intent, burns nullifier (unchanged)
④ FORM     round accumulates intents until ≥ k (unchanged; k-floor on-chain)
⑤ EXECUTE  pool-program, one vault-signed tx: for each intent — create stake_account PDA;
   (chain) Initialize(staker=VAULT, withdrawer=A); DelegateStake → V (vault signs as staker);
           Authorize(staker: VAULT→A); pay f → R. "The pool staked k×(D−f−rent) to V."
⑥ SETTLE   participant holds authority A over their stake account; can undelegate/withdraw later
           (self-service, outside the pool). Reward accrual = deferred incentive module.
⑦ CANCEL   (while Open) authority A reclaims D via cancel_intent; nullifier stays burned (unchanged)
```

**Guarantees preserved from Plan 4:** k-anonymity by construction on the batch (sub-k rejected
on-chain); uniform actor (vault is sole signer; delegations uniform in validator + amount, with
fresh unlinkable per-participant authorities/stake-accounts — the same privacy model as withdraw's
differing-but-unlinkable recipients);
value conservation (each intent backed 1:1 by a burned nullifier ⇐ a deposit); no
fund/authority redirection (payout/authority accounts key-matched to the proof-bound `Intent`);
single-commit + replay closed (nullifier PDA); privacy (no secret/preimage logged).

---

## 4. Threat model deltas (vs Plan 4)

| Adversary | Attack | Defense |
|---|---|---|
| Clustering on the stake side | Link the *stake account* back to a depositor | Authority `A` is a fresh, ZK-unlinkable key (like `recipient`); the stake account is a pool-derived PDA created uniformly by the vault — no participant-derived seed |
| Non-uniform batch | Distinguish intents by validator/amount | Fixed validator + fixed fee ⇒ every delegation is the same amount to the same validator (only the per-participant stake-account/authority differ, and those are ZK-unlinkable); a pool is single-action-kind so no withdraw/stake mixing |
| Authority redirection | Steer a stake account's authority to the attacker | Both authorities end at `intent.recipient` (withdrawer at `Initialize`, staker via `Authorize`), read from the stored `Intent` and bound in the proof via `extDataHash` — not an execute-time account the cranker supplies, so it can't be substituted without a fresh proof (which the attacker can't produce for someone else's note) |
| Sub-k stake round | Fire a thin stake round | Same on-chain k-floor (`meets_k_floor`); `execute_round` rejects below k |
| Whale self-fill | Satisfy k with own intents | **Unchanged residual** — the k-floor still counts intents, not distinct funders (Plan 6 harness measures it; Sybil-pricing is deferred). Stated honestly, not solved here. |

---

## 5. Testing strategy

1. **LiteSVM pooled-stake round (happy path):** set up a validator vote account in the SVM;
   initialize a Stake pool; deposit + commit k intents; `execute_round`; assert each stake
   account PDA exists, is owned by the Stake program, is **delegated to V**, and has stake &
   withdraw authority = the intent's proof-bound `recipient`; assert the batch is one
   vault-signed tx and the vault is debited exactly `k × denomination`. Print CU.
2. **Adversarial (mandatory, fail-closed):** sub-k stake round → `KFloorNotMet`; a substituted
   authority account → `IntentAccountMismatch`; a foreign-pool/foreign-round intent →
   `IntentInvalid`; a duplicated intent → `DuplicateIntent`; a wrong stake-account PDA →
   rejected; re-execute → rejected.
3. **cancel_intent on a stake pool:** the authority reclaims the denomination while Open; the
   nullifier stays burned (note not re-committable).
4. **Seam regression:** the existing withdraw pool + its full suite stay green — proving
   `execute_round`'s per-kind dispatch didn't regress the withdraw path.
5. **Value conservation / uniformity asserts:** the delegation *shape* is uniform across
   participants — same validator, same delegated amount (fee is fixed per pool, §6) — while the
   per-participant stake-account PDA and authority key necessarily differ but are ZK-unlinkable
   (assert the delegated amount + validator are identical across intents; do NOT assert byte-
   identity on the identity fields, which is impossible and not the property we want).

---

## 6. Open questions / notes

- **Fee uniformity — LOCKED: stake pools use a fixed per-pool fee.** A free per-intent `fee`
  would let delegated amounts differ across a round — an amount-uniformity leak the verified
  research flagged. Decision: a **Stake** pool sets its `fee` at `initialize_pool` (a
  `stake_fee: u64` field on the `Pool`, alongside `validator`), and `commit_intent` on a stake
  pool requires the intent's `fee == pool.stake_fee` — so every delegation in a round is
  `denomination − stake_fee`, so the delegated *amount* is identical across the round (the
  per-participant stake-account/authority still differ, and are ZK-unlinkable). (Withdraw pools keep the existing variable
  `fee`; their uniformity is a pre-existing concern out of Plan 5 scope.)
- **Denomination validity — LOCKED constraint (rent + minimum-delegation are one decision).** The
  vault funds the stake account from the deposit, so the value splits three ways:
  `denomination = stake_fee + stake_rent + delegated_amount`, i.e. **`delegated_amount =
  denomination − stake_fee − stake_rent`** (this supersedes the earlier "delegated = D − fee";
  §2.1/§3 are corrected to match). Two hard floors apply: `stake_rent` = the rent-exempt minimum
  for a stake account (fixed size, `Rent::minimum_balance(StakeStateV2::size_of())`), and
  `delegated_amount` **must be ≥ the Stake program's minimum delegation** (`get_minimum_delegation()`
  — currently 1 SOL on mainnet) or `DelegateStake` rejects. So a Stake pool is valid only if
  `denomination − stake_fee − stake_rent ≥ min_delegation`; `initialize_pool` enforces this
  (fail-closed) for `action_kind = Stake`. The plan pins the exact lamport constants; the *design
  constraint* is fixed here (no longer an open question).
- **Threat-model residual (nullifier-bound position).** The stake PDA is seeded by the public
  `nullifier_hash`, so the on-chain staking position is permanently bound to `N`. This does **not**
  leak the deposit (`N` is ZK-unlinkable to the note), but a participant who later undelegates /
  withdraws from a *doxxed* wallet links their identity → the position → `N`. Same class as the
  whale-self-fill residual (§4): an honest, stated tradeoff of the delegate-only, self-service exit,
  not a defect. Mitigations (fresh withdraw destination; a future pooled un-stake) are deferred.
- **Effective-k (Plan 6).** This spec does not build the harness; it ensures the stake round is
  *measurable* by it (single-action-kind, uniform, k-floor-gated).

---

## 7. Non-goals (Plan 5)

- No pooled **un-stake** / reward claim (needs the incentive module; participants self-service
  via their authority).
- No pooled **swap** (account-envelope; needs the chunked executor).
- No **multi-action pools** (a pool is one action kind, for uniformity).
- No **incentive/bonding** (Sybil-pricing) — the whale-self-fill residual is stated, not fixed.
- No **effective-k harness** (Plan 6).
- No new **circuit / trusted-setup** work (the withdraw circuit is reused via `extDataHash`).
