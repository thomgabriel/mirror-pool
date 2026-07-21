//! RPC plumbing shared by every phase: `Ctx` bundles the blocking RPC client,
//! the operator keypair (fee payer + sole on-chain signer for every send),
//! and the running `Report`. The three send helpers are the only places a
//! transaction leaves this process, so blockhash freshness and tx-table
//! recording live here once instead of at every call site.

use std::time::Duration;

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    address_lookup_table::{
        instruction::{create_lookup_table, extend_lookup_table},
        state::AddressLookupTable,
        AddressLookupTableAccount,
    },
    instruction::Instruction,
    message::{v0, Message, VersionedMessage},
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    transaction::{Transaction, VersionedTransaction},
};

use crate::report::Report;

#[derive(Debug)]
pub struct SoakError(String);

impl SoakError {
    pub fn new(msg: impl Into<String>) -> Self {
        SoakError(msg.into())
    }
}

impl std::fmt::Display for SoakError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SoakError {}

pub type SoakResult<T> = Result<T, SoakError>;

pub struct Ctx {
    pub client: RpcClient,
    pub operator: Keypair,
    pub report: Report,
}

/// Sends a legacy-message transaction, fee-payer/operator-signed plus
/// whatever extra `signers` the instructions require. Fetches its own
/// blockhash — never reuse one across sends.
pub fn send_ixs(
    ctx: &Ctx,
    label: &str,
    ixs: &[Instruction],
    signers: &[&Keypair],
) -> SoakResult<Signature> {
    let bh = ctx
        .client
        .get_latest_blockhash()
        .map_err(|e| SoakError::new(format!("{label}: get_latest_blockhash: {e}")))?;
    let msg = Message::new(ixs, Some(&ctx.operator.pubkey()));
    let tx = Transaction::new(signers, msg, bh);
    let sig = ctx
        .client
        .send_and_confirm_transaction(&tx)
        .map_err(|e| SoakError::new(format!("{label}: send_and_confirm_transaction: {e}")))?;
    ctx.report.record_tx(label, sig);
    Ok(sig)
}

/// Sends a v0 transaction compiled against `alt`, for instructions whose
/// account list is too large for a legacy message. Same fresh-blockhash and
/// tx-table discipline as `send_ixs`.
pub fn send_v0(
    ctx: &Ctx,
    label: &str,
    ixs: &[Instruction],
    alt: &AddressLookupTableAccount,
    signers: &[&Keypair],
) -> SoakResult<Signature> {
    let bh = ctx
        .client
        .get_latest_blockhash()
        .map_err(|e| SoakError::new(format!("{label}: get_latest_blockhash: {e}")))?;
    let msg = v0::Message::try_compile(&ctx.operator.pubkey(), ixs, std::slice::from_ref(alt), bh)
        .map_err(|e| SoakError::new(format!("{label}: v0 message compile: {e}")))?;
    let tx = VersionedTransaction::try_new(VersionedMessage::V0(msg), signers)
        .map_err(|e| SoakError::new(format!("{label}: versioned tx sign: {e}")))?;
    let sig = ctx
        .client
        .send_and_confirm_transaction(&tx)
        .map_err(|e| SoakError::new(format!("{label}: send_and_confirm_transaction: {e}")))?;
    ctx.report.record_tx(label, sig);
    Ok(sig)
}

const ALT_CHUNK_SIZE: usize = 20;
const ALT_ACTIVATION_POLL: Duration = Duration::from_millis(400);

/// Creates an Address Lookup Table, extends it with `addresses` in chunks of
/// `ALT_CHUNK_SIZE` (a safe margin under the ~30/tx wire ceiling), waits
/// until activation (≥1 slot past the LAST extend's landed slot), then reads
/// the table back from chain so the returned account reflects what actually
/// landed rather than what was requested.
pub fn create_and_fill_alt(
    ctx: &Ctx,
    addresses: &[Pubkey],
) -> SoakResult<AddressLookupTableAccount> {
    let authority = ctx.operator.pubkey();
    let payer = ctx.operator.pubkey();
    let table_key = create_alt_table(ctx, authority, payer)?;

    let mut last_extend_slot = None;
    for (i, chunk) in addresses.chunks(ALT_CHUNK_SIZE).enumerate() {
        let ix = extend_lookup_table(table_key, authority, Some(payer), chunk.to_vec());
        let label = format!("alt: extend_lookup_table[{i}]");
        let sig = send_ixs(ctx, &label, &[ix], &[&ctx.operator])?;
        last_extend_slot = Some(signature_slot(ctx, sig)?);
    }
    let last_extend_slot = last_extend_slot
        .ok_or_else(|| SoakError::new("alt: create_and_fill_alt called with no addresses"))?;

    loop {
        let slot = ctx
            .client
            .get_slot()
            .map_err(|e| SoakError::new(format!("alt activation: get_slot: {e}")))?;
        if slot > last_extend_slot {
            break;
        }
        std::thread::sleep(ALT_ACTIVATION_POLL);
    }

    let account = ctx
        .client
        .get_account(&table_key)
        .map_err(|e| SoakError::new(format!("alt fetch: get_account({table_key}): {e}")))?;
    let table = AddressLookupTable::deserialize(&account.data)
        .map_err(|e| SoakError::new(format!("alt fetch: deserialize table {table_key}: {e}")))?;
    Ok(AddressLookupTableAccount {
        key: table_key,
        addresses: table.addresses.to_vec(),
    })
}

fn create_alt_table(ctx: &Ctx, authority: Pubkey, payer: Pubkey) -> SoakResult<Pubkey> {
    let mut retried = false;
    loop {
        let current_slot = ctx
            .client
            .get_slot()
            .map_err(|e| SoakError::new(format!("alt create: get_slot: {e}")))?;
        // The on-chain check is `slot_hashes.get(&recent_slot).is_some()` — only slots
        // strictly BEFORE the executing bank's own slot have a hash recorded. A fast
        // single-node localnet can land this transaction in the very slot `get_slot()`
        // just returned, so the current slot itself is not yet a valid `recent_slot`.
        let recent_slot = current_slot.saturating_sub(1);
        let (ix, table_key) = create_lookup_table(authority, payer, recent_slot);
        match send_ixs(ctx, "alt: create_lookup_table", &[ix], &[&ctx.operator]) {
            Ok(_) => return Ok(table_key),
            Err(_) if !retried => {
                // "not a recent slot" class of errors — one retry with a fresher slot.
                retried = true;
            }
            Err(e) => return Err(e),
        }
    }
}

fn signature_slot(ctx: &Ctx, sig: Signature) -> SoakResult<u64> {
    let statuses = ctx
        .client
        .get_signature_statuses(&[sig])
        .map_err(|e| SoakError::new(format!("get_signature_statuses({sig}): {e}")))?;
    statuses
        .value
        .into_iter()
        .next()
        .flatten()
        .map(|s| s.slot)
        .ok_or_else(|| SoakError::new(format!("no landed slot found for signature {sig}")))
}
