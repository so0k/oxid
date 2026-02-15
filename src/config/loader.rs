use std::path::Path;

use anyhow::Result;

use crate::config::types::WorkspaceConfig;

/// Detection result for config format.
#[derive(Debug, PartialEq)]
pub enum ConfigMode {
    Hcl,
    Yaml,
    Both,
}

/// Detect whether the given path contains HCL, YAML, or both config formats.
pub fn detect_mode(path: &Path) -> ConfigMode {
    let has_tf = has_tf_files(path);
    let has_yaml = has_yaml_files(path);

    match (has_tf, has_yaml) {
        (true, true) => ConfigMode::Both,
        (true, false) => ConfigMode::Hcl,
        (false, true) => ConfigMode::Yaml,
        (false, false) => ConfigMode::Yaml, // Default to YAML mode for error handling
    }
}

/// Load configuration from a path, auto-detecting the format.
///
/// - If .tf files exist → HCL mode (parse .tf files into WorkspaceConfig)
/// - If .yaml/.yml files exist → YAML mode (parse YAML into WorkspaceConfig)
/// - If both exist → merge both (HCL resources + YAML orchestration)
pub fn load_workspace(path: &Path) -> Result<WorkspaceConfig> {
    let mode = detect_mode(path);

    match mode {
        ConfigMode::Hcl => {
            tracing::info!("Detected HCL mode (.tf files)");
            crate::hcl::parse_directory(path)
        }
        ConfigMode::Yaml => {
            tracing::info!("Detected YAML mode (.yaml files)");
            let yaml_config = crate::config::parser::load_config(&path.to_string_lossy())?;
            crate::config::yaml_converter::yaml_to_workspace(&yaml_config)
        }
        ConfigMode::Both => {
            tracing::info!("Detected mixed mode (both .tf and .yaml files)");
            // Parse HCL first (resources, providers), then overlay YAML (orchestration)
            let mut workspace = crate::hcl::parse_directory(path)?;

            let yaml_config = crate::config::parser::load_config(&path.to_string_lossy())?;
            let yaml_workspace = crate::config::yaml_converter::yaml_to_workspace(&yaml_config)?;

            // Merge YAML modules and variables into the HCL workspace
            workspace.modules.extend(yaml_workspace.modules);
            workspace.variables.extend(yaml_workspace.variables);

            Ok(workspace)
        }
    }
}

fn has_tf_files(path: &Path) -> bool {
    if path.is_file() {
        return path.extension().map(|e| e == "tf").unwrap_or(false);
    }
    if path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            return entries
                .filter_map(|e| e.ok())
                .any(|e| e.path().extension().map(|ext| ext == "tf").unwrap_or(false));
        }
    }
    false
}

fn has_yaml_files(path: &Path) -> bool {
    if path.is_file() {
        return path
            .extension()
            .map(|e| e == "yaml" || e == "yml")
            .unwrap_or(false);
    }
    if path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            return entries.filter_map(|e| e.ok()).any(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "yaml" || ext == "yml")
                    .unwrap_or(false)
            });
        }
    }
    false
}
