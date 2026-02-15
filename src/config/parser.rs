use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::types::{YamlConfig, YamlProject, YamlSettings};

/// Load config by auto-discovering yaml files.
///
/// - If `path` is a `.yaml`/`.yml` file, load just that file.
/// - If `path` is a directory, discover and merge all `*.yaml`/`*.yml` files in it.
/// - If `path` is the default `oxid.yaml` and doesn't exist, scan the current directory.
pub fn load_config(path: &str) -> Result<YamlConfig> {
    let p = Path::new(path);

    // Single file explicitly given
    if p.is_file() {
        let content = fs::read_to_string(p)
            .with_context(|| format!("Failed to read config file: {}", path))?;
        return parse_config(&content);
    }

    // Directory given — scan it
    if p.is_dir() {
        return load_from_directory(p);
    }

    // Default "oxid.yaml" doesn't exist — try current directory
    if path == "oxid.yaml" && !p.exists() {
        let cwd = Path::new(".");
        let yamls = find_yaml_files(cwd)?;
        if !yamls.is_empty() {
            return merge_yaml_files(&yamls);
        }
    }

    bail!(
        "Config not found: '{}'. Place .yaml files in the current directory or specify a path with -c",
        path
    )
}

/// Load and merge all yaml files from a directory.
fn load_from_directory(dir: &Path) -> Result<YamlConfig> {
    let yamls = find_yaml_files(dir)?;
    if yamls.is_empty() {
        bail!("No .yaml files found in directory: {}", dir.display());
    }
    merge_yaml_files(&yamls)
}

/// Find all .yaml/.yml files in a directory (non-recursive).
fn find_yaml_files(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files: Vec<std::path::PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory: {}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| {
            p.is_file()
                && matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("yaml") | Some("yml")
                )
        })
        .collect();
    files.sort();
    Ok(files)
}

/// Merge multiple yaml config files into a single YamlConfig.
///
/// YamlSettings come from the first file that defines them.
/// Variables and modules are merged across all files.
fn merge_yaml_files(files: &[std::path::PathBuf]) -> Result<YamlConfig> {
    tracing::info!(
        files = ?files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>(),
        "Discovered config files"
    );

    let mut merged_name: Option<String> = None;
    let mut merged_version: Option<String> = None;
    let mut merged_settings: Option<YamlSettings> = None;
    let mut merged_variables: HashMap<String, serde_yaml::Value> = HashMap::new();
    let mut merged_modules = HashMap::new();
    let mut merged_hooks = None;

    for file in files {
        let content = fs::read_to_string(file)
            .with_context(|| format!("Failed to read config file: {}", file.display()))?;
        let config: YamlConfig = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse YAML in: {}", file.display()))?;

        let project = config.project;

        if merged_name.is_none() {
            merged_name = Some(project.name);
        }
        if merged_version.is_none() {
            merged_version = Some(project.version);
        }
        if merged_settings.is_none() {
            merged_settings = Some(project.settings);
        }

        for (k, v) in project.variables {
            merged_variables.entry(k).or_insert(v);
        }

        for (name, module) in project.modules {
            if merged_modules.contains_key(&name) {
                bail!(
                    "Duplicate module '{}' found across config files. Module names must be unique.",
                    name
                );
            }
            merged_modules.insert(name, module);
        }

        if merged_hooks.is_none() && project.hooks.is_some() {
            merged_hooks = project.hooks;
        }
    }

    Ok(YamlConfig {
        project: YamlProject {
            name: merged_name.unwrap_or_else(|| "oxid-project".to_string()),
            version: merged_version.unwrap_or_else(|| "1.0".to_string()),
            settings: merged_settings.unwrap_or_default(),
            variables: merged_variables,
            modules: merged_modules,
            hooks: merged_hooks,
        },
    })
}

/// Parse YAML content into an YamlConfig.
pub fn parse_config(content: &str) -> Result<YamlConfig> {
    let config: YamlConfig =
        serde_yaml::from_str(content).context("Failed to parse YAML configuration")?;
    Ok(config)
}
