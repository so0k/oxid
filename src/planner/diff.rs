use anyhow::Result;

use crate::config::types::YamlConfig;
use crate::state::store::StateStore;

/// Represents a detected drift for a module.
#[derive(Debug)]
pub struct DriftResult {
    pub module_name: String,
    pub drift_type: DriftType,
    pub details: String,
}

/// Type of drift detected.
#[derive(Debug)]
pub enum DriftType {
    /// Module exists in config but not in state.
    NewModule,
    /// Module exists in state but not in config.
    RemovedModule,
    /// Module source or version changed.
    ConfigChanged,
    /// Module was previously applied but status is not succeeded.
    StateInconsistent,
}

/// Detect drift between the config and the current state.
pub fn detect_drift(config: &YamlConfig, store: &StateStore) -> Result<Vec<DriftResult>> {
    let mut drifts = Vec::new();
    let stored_modules = store.list_modules()?;
    let stored_names: std::collections::HashSet<String> =
        stored_modules.iter().map(|m| m.name.clone()).collect();

    // Check for new modules (in config but not in state)
    for name in config.project.modules.keys() {
        if !stored_names.contains(name) {
            drifts.push(DriftResult {
                module_name: name.clone(),
                drift_type: DriftType::NewModule,
                details: "Module defined in config but has never been applied".to_string(),
            });
        }
    }

    // Check for removed modules (in state but not in config)
    for module in &stored_modules {
        if !config.project.modules.contains_key(&module.name) {
            drifts.push(DriftResult {
                module_name: module.name.clone(),
                drift_type: DriftType::RemovedModule,
                details: "Module exists in state but no longer in config".to_string(),
            });
        }
    }

    // Check for state inconsistencies
    for module in &stored_modules {
        if config.project.modules.contains_key(&module.name) && module.status != "succeeded" {
            drifts.push(DriftResult {
                module_name: module.name.clone(),
                drift_type: DriftType::StateInconsistent,
                details: format!("Module status is '{}', expected 'succeeded'", module.status),
            });
        }
    }

    Ok(drifts)
}
