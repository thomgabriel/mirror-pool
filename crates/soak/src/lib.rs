//! Library surface for the `soak` binary — split out so the RPC/report
//! helpers Tasks 2-3 consume (`send_v0`, `create_and_fill_alt`,
//! `Report::assertion`) are a real public API, not dead code the bin-only
//! reachability graph would flag before their callers land.

pub mod phases;
pub mod report;
pub mod rpc;
