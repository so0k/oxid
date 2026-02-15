use anyhow::Result;

use crate::state::store::StateStore;

/// Execution summary report.
#[derive(Debug)]
pub struct Report {
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub cancelled: usize,
}

/// Generate a report from the current state.
pub fn generate_report(store: &StateStore) -> Result<Report> {
    let modules = store.list_modules()?;

    let succeeded = modules.iter().filter(|m| m.status == "succeeded").count();
    let failed = modules.iter().filter(|m| m.status == "failed").count();
    let skipped = modules.iter().filter(|m| m.status == "pending").count();
    let cancelled = modules.iter().filter(|m| m.status == "cancelled").count();

    Ok(Report {
        succeeded,
        failed,
        skipped,
        cancelled,
    })
}
