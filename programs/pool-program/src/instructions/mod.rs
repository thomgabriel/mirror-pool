pub mod cancel_intent;
pub mod commit_intent;
pub mod deposit;
pub mod execute_round;
pub mod initialize_pool;

pub use cancel_intent::CancelIntent;
pub use commit_intent::CommitIntent;
pub use deposit::{Deposit, DepositEvent};
pub use execute_round::ExecuteRound;
pub use initialize_pool::InitializePool;

// Anchor's #[program] macro emits `pub use crate::__client_accounts_<ix>::*;`
// (and, under the `cpi` feature, `pub use crate::__cpi_client_accounts_<ix>::*;`)
// for each instruction, assuming those anchor-derive-accounts-generated
// (`pub(crate)`) modules sit at the crate root. Since the Accounts structs now
// live one level down in instructions::<name>, re-export the generated modules
// up to this level so lib.rs's `pub use instructions::*;` carries them the rest
// of the way.
pub(crate) use cancel_intent::__client_accounts_cancel_intent;
pub(crate) use commit_intent::__client_accounts_commit_intent;
pub(crate) use deposit::__client_accounts_deposit;
pub(crate) use execute_round::__client_accounts_execute_round;
pub(crate) use initialize_pool::__client_accounts_initialize_pool;

#[cfg(feature = "cpi")]
pub(crate) use cancel_intent::__cpi_client_accounts_cancel_intent;
#[cfg(feature = "cpi")]
pub(crate) use commit_intent::__cpi_client_accounts_commit_intent;
#[cfg(feature = "cpi")]
pub(crate) use deposit::__cpi_client_accounts_deposit;
#[cfg(feature = "cpi")]
pub(crate) use execute_round::__cpi_client_accounts_execute_round;
#[cfg(feature = "cpi")]
pub(crate) use initialize_pool::__cpi_client_accounts_initialize_pool;
