// Blanket allow mirrors execute_round.rs/stake_round.rs: the stake execute
// tail requires the deprecated `stake::config::ID`, and the cached fixtures
// use `Keypair::from_bytes`.
#![allow(deprecated)]
mod round_support;
use round_support::{
    build_round_fixture_cached, build_stake_round_fixture_cached, commit_intent_tx, program_id,
    DENOMINATION, FEE,
};

use pool_program::invariants::{MAX_K_STAKE, MAX_K_WITHDRAW, TIMEOUT_SLOTS};
use solana_address_lookup_table_interface::state::{AddressLookupTable, LookupTableMeta};
use solana_sdk::{
    account::{Account, ReadableAccount},
    address_lookup_table,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction, InstructionError},
    message::{v0, AddressLookupTableAccount, Message, VersionedMessage},
    pubkey::Pubkey,
    rent::Rent,
    signature::{Keypair, Signer},
    system_program,
    transaction::{Transaction, TransactionError, VersionedTransaction},
};

// Stake tail ids (mirrors stake_round.rs's execute helper).
use solana_sdk::{stake, sysvar};

fn stake_pda(pool: Pubkey, intent_pda: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"stake", pool.as_ref(), intent_pda.as_ref()],
        &program_id(),
    )
    .0
}

fn execute_ix(
    fx: &round_support::RoundFixture,
    round_id: u64,
    cranker: Pubkey,
    k: usize,
    stake: bool,
) -> Instruction {
    let (round, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &round_id.to_le_bytes()],
        &program_id(),
    );
    let (next_round, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &(round_id + 1).to_le_bytes()],
        &program_id(),
    );
    let mut accounts = vec![
        AccountMeta::new(fx.pool, false),
        AccountMeta::new(round, false),
        AccountMeta::new(next_round, false),
        AccountMeta::new(fx.vault, false),
        AccountMeta::new(cranker, true),
        AccountMeta::new_readonly(system_program::ID, false),
    ];
    for m in fx.intents.iter().take(k) {
        accounts.push(AccountMeta::new(m.intent_pda, false));
        if stake {
            accounts.push(AccountMeta::new(stake_pda(fx.pool, m.intent_pda), false));
        } else {
            accounts.push(AccountMeta::new(m.recipient, false));
        }
        accounts.push(AccountMeta::new(m.relayer, false));
    }
    if stake {
        accounts.push(AccountMeta::new_readonly(fx.validator, false));
        accounts.push(AccountMeta::new_readonly(stake::program::ID, false));
        accounts.push(AccountMeta::new_readonly(stake::config::ID, false));
        accounts.push(AccountMeta::new_readonly(sysvar::clock::ID, false));
        accounts.push(AccountMeta::new_readonly(sysvar::stake_history::ID, false));
        accounts.push(AccountMeta::new_readonly(sysvar::rent::ID, false));
    }
    let mut data = round_support::disc("execute_round").to_vec();
    data.extend_from_slice(&round_id.to_le_bytes());
    Instruction {
        program_id: program_id(),
        accounts,
        data,
    }
}

/// Commit the first `k` cached intents into round 0, expiring the blockhash
/// between sends so identical-fee-payer txs never collide.
fn commit_first(fx: &mut round_support::RoundFixture, k: usize) {
    for i in 0..k {
        fx.svm.expire_blockhash();
        fx.svm
            .send_transaction(commit_intent_tx(fx, i, 0))
            .unwrap_or_else(|e| panic!("commit {i} must succeed: {e:?}"));
    }
}

/// Execute round 0 with `k` intents at the max CU budget; returns the result.
/// `Err` is boxed — `FailedTransactionMetadata` is 208 bytes, too large for a
/// bare `Result` per clippy's `result_large_err`.
fn execute_round0(
    fx: &mut round_support::RoundFixture,
    cranker: &Keypair,
    k: usize,
    stake: bool,
) -> Result<litesvm::types::TransactionMetadata, Box<litesvm::types::FailedTransactionMetadata>> {
    fx.svm.expire_blockhash();
    let ix = execute_ix(fx, 0, cranker.pubkey(), k, stake);
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            ix,
        ],
        Some(&cranker.pubkey()),
    );
    let tx = Transaction::new(&[cranker], msg, fx.svm.latest_blockhash());
    fx.svm.send_transaction(tx).map_err(Box::new)
}

/// A synthetic, always-active lookup table (`deactivation_slot = Slot::MAX`,
/// via `LookupTableMeta::default()`) written directly with `svm.set_account`
/// rather than the real create/extend CPI flow — sufficient because the sweep
/// only needs the resolved addresses to exist for `v0::Message::try_compile`
/// and execution, not a realistic activation-delay lifecycle.
fn set_synthetic_alt(svm: &mut litesvm::LiteSVM, addresses: Vec<Pubkey>) -> Pubkey {
    let key = Pubkey::new_unique();
    let table = AddressLookupTable {
        meta: LookupTableMeta::default(),
        addresses: std::borrow::Cow::Owned(addresses),
    };
    let data = table.serialize_for_tests().unwrap();
    let rent = Rent::default().minimum_balance(data.len());
    svm.set_account(
        key,
        Account {
            lamports: rent,
            data,
            owner: address_lookup_table::program::id(),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    key
}

/// Execute round 0 with `k` intents via a v0+ALT `VersionedTransaction` (the
/// per-intent triples resolved through a synthetic lookup table): the sweep's
/// only path past k~11, since legacy `Message`s panic once the total account
/// count exceeds the `solana-compute-budget-instruction` crate's internal
/// `FILTER_SIZE = PACKET_DATA_SIZE / 32 = 38` (see
/// `sweep_execute_round_ceiling`'s doc comment for the measured detail).
///
/// Deliberately does NOT request a larger heap frame: a 256 KiB
/// `request_heap_frame` was measured to change nothing (see the doc comment's
/// OOM finding — the default bump allocator is hard-capped at 32 KiB
/// regardless of the requested frame size), so the simpler two-instruction tx
/// is also the one that reflects the true default-runtime envelope Task 2's
/// guard tests must hold to.
fn execute_round0_v0(
    fx: &mut round_support::RoundFixture,
    cranker: &Keypair,
    k: usize,
    stake: bool,
) -> Result<litesvm::types::TransactionMetadata, Box<litesvm::types::FailedTransactionMetadata>> {
    // LiteSVM's Clock sysvar defaults to slot 0 and never auto-advances; the
    // synthetic ALT's `last_extended_slot` also defaults to 0, and the lookup
    // table only exposes its addresses as active once `current_slot >
    // last_extended_slot` — so without this warp every lookup fails closed
    // with InvalidAddressLookupTableIndex regardless of k.
    let current_slot = fx.svm.get_sysvar::<solana_sdk::clock::Clock>().slot;
    fx.svm.warp_to_slot(current_slot + 1);

    let per_intent: Vec<Pubkey> = fx
        .intents
        .iter()
        .take(k)
        .flat_map(|m| {
            let second = if stake {
                stake_pda(fx.pool, m.intent_pda)
            } else {
                m.recipient
            };
            [m.intent_pda, second, m.relayer]
        })
        .collect();
    let alt_key = set_synthetic_alt(&mut fx.svm, per_intent.clone());
    let alt_account = AddressLookupTableAccount {
        key: alt_key,
        addresses: per_intent,
    };

    fx.svm.expire_blockhash();
    let ix = execute_ix(fx, 0, cranker.pubkey(), k, stake);
    let cb = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    let msg = v0::Message::try_compile(
        &cranker.pubkey(),
        &[cb, ix],
        &[alt_account],
        fx.svm.latest_blockhash(),
    )
    .expect("v0+ALT compile must succeed");
    let tx = VersionedTransaction::try_new(VersionedMessage::V0(msg), &[cranker]).unwrap();
    fx.svm.send_transaction(tx).map_err(Box::new)
}

#[test]
fn cached_fixture_round_trip_smoke() {
    // k=2 withdraw round entirely from the cached material pool: proves the
    // cache's proofs verify against the fixture's on-chain root before the
    // expensive sweep/guard tests depend on it.
    let (mut fx, _recipients) = build_round_fixture_cached(2, 2);
    commit_first(&mut fx, 2);
    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    execute_round0(&mut fx, &cranker, 2, false).expect("cached k=2 round must execute");
    for m in &fx.intents {
        // The cached fixture airdrops every recipient 1_000_000 lamports (so
        // cancel tests can sign); the payout lands on top of that.
        assert_eq!(
            fx.svm.get_account(&m.recipient).unwrap().lamports(),
            1_000_000 + DENOMINATION - FEE
        );
    }
}

#[test]
fn cached_stake_fixture_round_trip_smoke() {
    let (mut fx, _recipients) = build_stake_round_fixture_cached(2, 2);
    commit_first(&mut fx, 2);
    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    execute_round0(&mut fx, &cranker, 2, true).expect("cached k=2 stake round must execute");
    for m in &fx.intents {
        let s = fx
            .svm
            .get_account(&stake_pda(fx.pool, m.intent_pda))
            .unwrap();
        assert_eq!(s.owner, stake::program::ID, "stake PDA delegated");
    }
}

/// MEASUREMENT, not a guard: probes the per-transaction execute_round ceiling
/// for both action kinds. Run explicitly:
///   cargo test -p pool-program --test max_k -- --ignored --nocapture
/// Interpretation notes:
/// * A first pass using a LEGACY `Message` (matching the brief's original
///   design) revealed a hard finding BEFORE any 64-lock question could even
///   be probed: at k=12 withdraw (6 fixed + 36 per-intent = 42 total accounts)
///   LiteSVM's real Solana-SDK dependency panics — NOT a graceful
///   `TransactionError` — inside
///   `solana-compute-budget-instruction`'s `ComputeBudgetProgramIdFilter`,
///   whose internal fixed-size array is sized
///   `FILTER_SIZE = PACKET_DATA_SIZE / size_of::<Pubkey>() = 1232 / 32 = 38`.
///   Any legacy message with > 38 total static account keys crashes the test
///   process outright. This answers the brief's "does LiteSVM enforce
///   banking-stage sanitization" question directly: for LEGACY transactions,
///   yes — via a hard panic, at an even lower account count (38) than the
///   64-account-lock wall or the ~35-account legacy-MTU estimate this plan
///   assumed. Task 2's guard tests MUST use v0+ALT `VersionedTransaction`s
///   (the brief's own contingency) — legacy `Message`s cannot reach any k
///   near the expected ~17-19 lock-arithmetic ceiling at all.
/// * This sweep therefore compiles v0+ALT `VersionedTransaction`s
///   (`execute_round0_v0`, a synthetic always-active lookup table written via
///   `svm.set_account`) so it can actually reach k=21.
///
/// Measured 2026-07-18 (full logs in task-1-report.md):
///
/// WITHDRAW — every k from 8 to 21 OK, CU rising roughly linearly (~67.6k@8 to
/// ~132-134k@21, some run-to-run noise from LiteSVM's non-deterministic
/// compute metering but no discontinuity). No failure of any kind observed up
/// to k=21: LiteSVM enforces NEITHER the 64-account-lock limit NOR any compute
/// ceiling for a resolved v0+ALT key set in this range. The withdraw ceiling
/// is therefore NOT settled by this sweep; it is set by the lock-arithmetic
/// bound from the plan's Global Constraints (⌊(64-9)/3⌋ = 18, conservative
/// with the ALT table key counted), confirmed non-binding on compute.
///
/// STAKE — k=8..11 OK (CU ~175k-243k, i.e. roughly +20-25k CU per intent);
/// k=12 through 21 ALL fail identically with
/// `InstructionError(_, ProgramFailedToComplete)`, logging
/// `"Program log: Error: memory allocation failed, out of memory"` right
/// before the failing program's invoke — this is the SBF heap, not compute:
/// at k=12 the round is only ~270k CU (extrapolated), far under the 1.4M CU
/// budget, so CU was never the binding resource. Re-measured at k=11..17 with
/// `ComputeBudgetInstruction::request_heap_frame(256 * 1024)` (4x the default
/// 32 KiB) added to the message: IDENTICAL result — k=11 OK, k=12+ fail with
/// the same OOM log line. This confirms the wall is NOT liftable by the
/// cranker: `solana_program`'s default bump-allocator heap is hard-capped at
/// 32 KiB regardless of the requested frame size (a custom global allocator
/// could raise it, but that is a custody-program change, out of this plan's
/// scope — noted as future work, not attempted here). The stake ceiling is
/// therefore HEAP-bound at k=11, not lock- or compute-bound (the lock-
/// arithmetic bound of 16 and the never-hit compute ceiling are both moot —
/// the program runs out of heap first). `execute_round0_v0`'s final,
/// committed form does NOT request a larger heap frame, since doing so
/// changes nothing — the simpler tx is also the one that reflects the true
/// default-runtime envelope Task 2's guard tests must hold to.
#[test]
#[ignore = "measurement sweep; run with --ignored --nocapture"]
fn sweep_execute_round_ceiling() {
    for (label, stake) in [("withdraw", false), ("stake", true)] {
        println!("=== {label} sweep ===");
        for k in [8usize, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21] {
            let (mut fx, _r) = if stake {
                build_stake_round_fixture_cached(2, k)
            } else {
                build_round_fixture_cached(2, k)
            };
            commit_first(&mut fx, k);
            let cranker = Keypair::new();
            fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
            match execute_round0_v0(&mut fx, &cranker, k, stake) {
                Ok(meta) => println!("{label} k={k}: OK, CU={}", meta.compute_units_consumed),
                Err(e) => {
                    println!("{label} k={k}: FAIL {:?}", e.err);
                    println!("{label} k={k}: logs: {:?}", e.meta.logs);
                }
            }
        }
    }
}

/// The (MAX_K+1)-th commit must fail RoundFull — the fail-closed cap that
/// keeps every round settleable by one transaction.
#[test]
fn commit_intent_rejects_round_past_max_k_withdraw() {
    let n = MAX_K_WITHDRAW as usize + 1;
    let (mut fx, _r) = build_round_fixture_cached(2, n);
    commit_first(&mut fx, MAX_K_WITHDRAW as usize);
    fx.svm.expire_blockhash();
    let outcome = fx
        .svm
        .send_transaction(commit_intent_tx(&fx, MAX_K_WITHDRAW as usize, 0))
        .expect_err("commit past MAX_K must fail");
    assert!(matches!(
        outcome.err,
        TransactionError::InstructionError(_, InstructionError::Custom(_))
    ));
    // Anchor logs the variant name ("Error Code: RoundFull") — the idiom
    // initialize_pool.rs already asserts with.
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("RoundFull")),
        "expected RoundFull; logs: {:?}",
        outcome.meta.logs
    );
}

#[test]
fn commit_intent_rejects_round_past_max_k_stake() {
    let n = MAX_K_STAKE as usize + 1;
    let (mut fx, _r) = build_stake_round_fixture_cached(2, n);
    commit_first(&mut fx, MAX_K_STAKE as usize);
    fx.svm.expire_blockhash();
    let outcome = fx
        .svm
        .send_transaction(commit_intent_tx(&fx, MAX_K_STAKE as usize, 0))
        .expect_err("stake commit past MAX_K must fail");
    assert!(
        outcome.meta.logs.iter().any(|l| l.contains("RoundFull")),
        "expected RoundFull; logs: {:?}",
        outcome.meta.logs
    );
}

/// The compute drift guard: a round at exactly MAX_K executes in one tx. If a
/// future change makes execute_round heavier, this fails before production
/// rounds can strand. Uses the v0+ALT path — at MAX_K_WITHDRAW a withdraw
/// round's account count is past the legacy-Message panic threshold (see
/// `sweep_execute_round_ceiling`'s doc comment), so the legacy execute helper
/// cannot be used here at all.
#[test]
fn round_at_exactly_max_k_executes_withdraw() {
    let k = MAX_K_WITHDRAW as usize;
    let (mut fx, _r) = build_round_fixture_cached(2, k);
    commit_first(&mut fx, k);
    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    let vault_before = fx.svm.get_account(&fx.vault).unwrap().lamports();
    let meta = execute_round0_v0(&mut fx, &cranker, k, false)
        .expect("a MAX_K withdraw round must execute in one tx");
    println!("withdraw k={k} CU={}", meta.compute_units_consumed);
    let vault_after = fx.svm.get_account(&fx.vault).unwrap().lamports();
    assert_eq!(vault_before - vault_after, DENOMINATION * k as u64);
}

#[test]
fn round_at_exactly_max_k_executes_stake() {
    let k = MAX_K_STAKE as usize;
    let (mut fx, _r) = build_stake_round_fixture_cached(2, k);
    commit_first(&mut fx, k);
    let cranker = Keypair::new();
    fx.svm.airdrop(&cranker.pubkey(), 1_000_000_000).unwrap();
    let meta = execute_round0_v0(&mut fx, &cranker, k, true)
        .expect("a MAX_K stake round must execute in one tx");
    println!("stake k={k} CU={}", meta.compute_units_consumed);
    for m in fx.intents.iter().take(k) {
        let s = fx
            .svm
            .get_account(&stake_pda(fx.pool, m.intent_pda))
            .unwrap();
        assert_eq!(s.owner, stake::program::ID);
    }
}

/// cancel_intent decrements intent_count, so a full round frees a slot: fill
/// to MAX_K, cancel one (after the timeout), and the next commit succeeds.
#[test]
fn cancel_frees_a_slot_in_a_full_round() {
    let n = MAX_K_WITHDRAW as usize + 1;
    let (mut fx, recipients) = build_round_fixture_cached(2, n);
    commit_first(&mut fx, MAX_K_WITHDRAW as usize);

    // Warp past the cancel timeout, then cancel intent 0 (recipient signs).
    let clock: solana_sdk::clock::Clock = fx.svm.get_sysvar();
    fx.svm.warp_to_slot(clock.slot + TIMEOUT_SLOTS + 1);
    let m0 = &fx.intents[0];
    let (round, _) = Pubkey::find_program_address(
        &[b"round", fx.pool.as_ref(), &0u64.to_le_bytes()],
        &program_id(),
    );
    let mut data = round_support::disc("cancel_intent").to_vec();
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&m0.nullifier_hash);
    let cancel_ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new_readonly(fx.pool, false),
            AccountMeta::new(round, false),
            AccountMeta::new(m0.intent_pda, false),
            AccountMeta::new(fx.vault, false),
            AccountMeta::new(recipients[0].pubkey(), true),
            AccountMeta::new_readonly(system_program::ID, false),
        ],
        data,
    };
    fx.svm.expire_blockhash();
    let msg = Message::new(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(400_000),
            cancel_ix,
        ],
        Some(&recipients[0].pubkey()),
    );
    fx.svm
        .send_transaction(Transaction::new(
            &[&recipients[0]],
            msg,
            fx.svm.latest_blockhash(),
        ))
        .expect("timed-out cancel must succeed");

    // The freed slot accepts the (MAX_K+1)-th cached intent.
    fx.svm.expire_blockhash();
    fx.svm
        .send_transaction(commit_intent_tx(&fx, MAX_K_WITHDRAW as usize, 0))
        .expect("commit into the freed slot must succeed");
}
