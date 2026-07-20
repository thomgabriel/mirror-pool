//! The structured evidence report (`docs/soak-report.md`). Every section is
//! append-only through `&self` methods (backed by a `RefCell`) so a single
//! `Ctx` can be threaded around by shared reference while every phase and
//! every RPC send records into the same report.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::Duration;

use solana_sdk::{pubkey::Pubkey, signature::Signature};

struct AssertionRecord {
    id: String,
    desc: String,
    pass: bool,
    evidence: String,
}

struct ReportInner {
    program_id: Pubkey,
    validator_version: Option<String>,
    phase_timings: Vec<(String, Duration)>,
    tx_table: Vec<(String, Signature)>,
    assertions: Vec<AssertionRecord>,
    notes: Vec<String>,
    failed: Option<String>,
}

pub struct Report {
    inner: RefCell<ReportInner>,
}

impl Report {
    pub fn new(program_id: Pubkey) -> Self {
        Report {
            inner: RefCell::new(ReportInner {
                program_id,
                validator_version: None,
                phase_timings: Vec::new(),
                tx_table: Vec::new(),
                assertions: Vec::new(),
                notes: Vec::new(),
                failed: None,
            }),
        }
    }

    pub fn set_validator_version(&self, version: String) {
        self.inner.borrow_mut().validator_version = Some(version);
    }

    pub fn record_tx(&self, label: &str, sig: Signature) {
        self.inner
            .borrow_mut()
            .tx_table
            .push((label.to_string(), sig));
    }

    pub fn phase_timing(&self, phase: &str, dur: Duration) {
        self.inner
            .borrow_mut()
            .phase_timings
            .push((phase.to_string(), dur));
    }

    pub fn assertion(&self, id: &str, desc: &str, pass: bool, evidence: String) {
        self.inner.borrow_mut().assertions.push(AssertionRecord {
            id: id.to_string(),
            desc: desc.to_string(),
            pass,
            evidence,
        });
    }

    pub fn note(&self, text: &str) {
        self.inner.borrow_mut().notes.push(text.to_string());
    }

    /// Marks the run FAILED independent of the assertion table — for a phase
    /// that errors out before producing any assertions at all (the report
    /// must never read as a pass just because nothing failed an assertion).
    pub fn mark_failed(&self, reason: String) {
        self.inner.borrow_mut().failed = Some(reason);
    }

    pub fn finish(&self, path: &Path) -> std::io::Result<()> {
        let inner = self.inner.borrow();
        let mut out = String::new();

        writeln!(out, "# Soak Report").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "- Date: {}", now_utc_string()).unwrap();
        writeln!(out, "- Git commit: {}", git_commit_sha()).unwrap();
        writeln!(out, "- Program ID: {}", inner.program_id).unwrap();
        writeln!(
            out,
            "- Validator version: {}",
            inner.validator_version.as_deref().unwrap_or("unknown")
        )
        .unwrap();
        writeln!(out).unwrap();

        writeln!(out, "## Phase timings").unwrap();
        writeln!(out).unwrap();
        if inner.phase_timings.is_empty() {
            writeln!(out, "(none recorded)").unwrap();
        } else {
            writeln!(out, "| Phase | Duration |").unwrap();
            writeln!(out, "|---|---|").unwrap();
            for (phase, dur) in &inner.phase_timings {
                writeln!(out, "| {phase} | {:.2}s |", dur.as_secs_f64()).unwrap();
            }
        }
        writeln!(out).unwrap();

        writeln!(out, "## Transactions").unwrap();
        writeln!(out).unwrap();
        if inner.tx_table.is_empty() {
            writeln!(out, "(none recorded)").unwrap();
        } else {
            writeln!(out, "| Label | Signature |").unwrap();
            writeln!(out, "|---|---|").unwrap();
            for (label, sig) in &inner.tx_table {
                writeln!(out, "| {label} | `{sig}` |").unwrap();
            }
        }
        writeln!(out).unwrap();

        writeln!(out, "## Assertions").unwrap();
        writeln!(out).unwrap();
        if inner.assertions.is_empty() {
            writeln!(out, "(none recorded)").unwrap();
        } else {
            writeln!(out, "| ID | Description | Result | Evidence |").unwrap();
            writeln!(out, "|---|---|---|---|").unwrap();
            for a in &inner.assertions {
                let result = if a.pass { "PASS" } else { "FAIL" };
                writeln!(
                    out,
                    "| {} | {} | {} | {} |",
                    a.id, a.desc, result, a.evidence
                )
                .unwrap();
            }
        }
        writeln!(out).unwrap();

        if !inner.notes.is_empty() {
            writeln!(out, "## Notes").unwrap();
            writeln!(out).unwrap();
            for n in &inner.notes {
                writeln!(out, "- {n}").unwrap();
            }
            writeln!(out).unwrap();
        }

        let all_assertions_pass = inner.assertions.iter().all(|a| a.pass);
        let run_passed = inner.failed.is_none() && all_assertions_pass;
        if run_passed {
            writeln!(out, "**RUN PASSED**").unwrap();
        } else {
            writeln!(out, "**RUN FAILED**").unwrap();
            if let Some(reason) = &inner.failed {
                writeln!(out, "- Failure: {reason}").unwrap();
            }
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, out)
    }
}

fn git_commit_sha() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn now_utc_string() -> String {
    std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
