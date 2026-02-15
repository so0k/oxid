use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing;

use crate::config::types::YamlConfig;
use crate::executor::terraform;
use crate::state::store::StateStore;

/// Execute module batches in parallel, respecting dependency order.
pub async fn execute_batches(
    config: &YamlConfig,
    batches: &[Vec<String>],
    store: &StateStore,
    targets: &[String],
    force: bool,
) -> Result<()> {
    let semaphore = Arc::new(Semaphore::new(config.project.settings.parallelism));
    let target_set: HashSet<&str> = targets.iter().map(|s| s.as_str()).collect();
    let mut failed_modules: HashSet<String> = HashSet::new();
    let mut cancelled_modules: HashSet<String> = HashSet::new();

    // Extract aws_region from project variables
    let aws_region = config
        .project
        .variables
        .get("aws_region")
        .and_then(|v| v.as_str())
        .unwrap_or("us-east-1")
        .to_string();

    for (batch_idx, batch) in batches.iter().enumerate() {
        tracing::info!(batch = batch_idx + 1, modules = ?batch, "Starting batch");

        let mut handles = Vec::new();

        for module_name in batch {
            // Skip if not in targets (when targets are specified)
            if !target_set.is_empty() && !target_set.contains(module_name.as_str()) {
                continue;
            }

            // Skip if a dependency has failed
            let module_config = &config.project.modules[module_name];
            let dep_failed = module_config
                .depends_on
                .iter()
                .any(|d| failed_modules.contains(d) || cancelled_modules.contains(d));
            if dep_failed {
                tracing::warn!(
                    module = module_name.as_str(),
                    "Skipping due to failed dependency"
                );
                cancelled_modules.insert(module_name.clone());
                store.update_module_status(module_name, "cancelled")?;
                continue;
            }

            // Skip already-succeeded modules unless force is set
            if !force {
                if let Some(status) = store.get_module_status(module_name)? {
                    if status == "succeeded" {
                        tracing::info!(
                            module = module_name.as_str(),
                            "Skipping (already succeeded)"
                        );
                        continue;
                    }
                }
            }

            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let settings = config.project.settings.clone();
            let module_name = module_name.clone();
            let module_config = module_config.clone();
            let working_dir = config.project.settings.working_dir.clone();
            let resolved_vars = resolve_module_variables(config, &module_name, store)?;
            let region = aws_region.clone();

            let handle = tokio::spawn(async move {
                let _permit = permit;
                let module_dir = PathBuf::from(&working_dir)
                    .join("modules")
                    .join(&module_name);

                tracing::info!(module = module_name.as_str(), "Executing module");

                // Generate terraform files
                terraform::generate_terraform_files(
                    &module_name,
                    &module_config,
                    &resolved_vars,
                    &module_dir,
                    Some(&region),
                )?;

                // Init
                let init_result = terraform::terraform_init(&settings, &module_dir).await?;
                if init_result.exit_code != 0 {
                    anyhow::bail!(
                        "terraform init failed for module '{}': {}",
                        module_name,
                        init_result.error_message()
                    );
                }

                // Plan
                let plan_result = terraform::terraform_plan(&settings, &module_dir).await?;
                if plan_result.exit_code != 0 && plan_result.exit_code != 2 {
                    anyhow::bail!(
                        "terraform plan failed for module '{}': {}",
                        module_name,
                        plan_result.error_message()
                    );
                }

                // Apply
                let apply_result = terraform::terraform_apply(&settings, &module_dir).await?;
                if apply_result.exit_code != 0 {
                    anyhow::bail!(
                        "terraform apply failed for module '{}': {}",
                        module_name,
                        apply_result.error_message()
                    );
                }

                // Capture outputs
                let outputs = terraform::terraform_output(&settings, &module_dir).await?;

                Ok::<(String, HashMap<String, serde_json::Value>), anyhow::Error>((
                    module_name,
                    outputs,
                ))
            });

            handles.push(handle);
        }

        // Wait for all modules in this batch
        for handle in handles {
            match handle.await? {
                Ok((module_name, outputs)) => {
                    store.update_module_status(&module_name, "succeeded")?;
                    for (key, value) in &outputs {
                        let value_str = serde_json::to_string(value)?;
                        store.set_output(&module_name, key, &value_str)?;
                    }
                    tracing::info!(module = module_name.as_str(), "Module succeeded");
                }
                Err(e) => {
                    let err_msg = format!("{}", e);
                    tracing::error!(error = %e, "Module execution failed");
                    if let Some(name) = extract_module_name_from_error(&err_msg) {
                        failed_modules.insert(name.clone());
                        store.update_module_status(&name, "failed")?;
                    }
                }
            }
        }
    }

    if !failed_modules.is_empty() {
        anyhow::bail!(
            "The following modules failed: {}",
            failed_modules.into_iter().collect::<Vec<_>>().join(", ")
        );
    }

    Ok(())
}

/// Execute destroy in reverse batch order.
pub async fn execute_destroy(
    config: &YamlConfig,
    reversed_batches: &[Vec<String>],
    store: &StateStore,
    targets: &[String],
) -> Result<()> {
    let semaphore = Arc::new(Semaphore::new(config.project.settings.parallelism));
    let target_set: HashSet<&str> = targets.iter().map(|s| s.as_str()).collect();

    for (batch_idx, batch) in reversed_batches.iter().enumerate() {
        tracing::info!(batch = batch_idx + 1, modules = ?batch, "Destroying batch");

        let mut handles = Vec::new();

        for module_name in batch {
            if !target_set.is_empty() && !target_set.contains(module_name.as_str()) {
                continue;
            }

            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let settings = config.project.settings.clone();
            let module_name = module_name.clone();
            let working_dir = config.project.settings.working_dir.clone();

            let handle = tokio::spawn(async move {
                let _permit = permit;
                let module_dir = PathBuf::from(&working_dir)
                    .join("modules")
                    .join(&module_name);

                if !module_dir.exists() {
                    tracing::warn!(
                        module = module_name.as_str(),
                        "No working directory found, skipping destroy"
                    );
                    return Ok::<String, anyhow::Error>(module_name);
                }

                let result = terraform::terraform_destroy(&settings, &module_dir).await?;
                if result.exit_code != 0 {
                    anyhow::bail!(
                        "terraform destroy failed for module '{}': {}",
                        module_name,
                        result.error_message()
                    );
                }

                Ok(module_name)
            });

            handles.push(handle);
        }

        for handle in handles {
            match handle.await? {
                Ok(module_name) => {
                    store.update_module_status(&module_name, "destroyed")?;
                    store.clear_outputs(&module_name)?;
                    tracing::info!(module = module_name.as_str(), "Module destroyed");
                }
                Err(e) => {
                    tracing::error!(error = %e, "Module destroy failed");
                }
            }
        }
    }

    Ok(())
}

/// Resolve variables for a module, substituting ${var.*} and ${module.*.*} references.
fn resolve_module_variables(
    config: &YamlConfig,
    module_name: &str,
    store: &StateStore,
) -> Result<HashMap<String, serde_json::Value>> {
    let module = &config.project.modules[module_name];
    let mut resolved = HashMap::new();

    for (key, value) in &module.variables {
        let resolved_value = resolve_yaml_value(value, &config.project.variables, store)?;
        resolved.insert(key.clone(), resolved_value);
    }

    Ok(resolved)
}

/// Resolve a single YAML value, replacing variable references.
fn resolve_yaml_value(
    value: &serde_yaml::Value,
    project_vars: &HashMap<String, serde_yaml::Value>,
    store: &StateStore,
) -> Result<serde_json::Value> {
    match value {
        serde_yaml::Value::String(s) => {
            let resolved = resolve_string_references(s, project_vars, store)?;
            Ok(serde_json::Value::String(resolved))
        }
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(serde_json::Value::Number(i.into()))
            } else if let Some(f) = n.as_f64() {
                Ok(serde_json::json!(f))
            } else {
                Ok(serde_json::Value::Null)
            }
        }
        serde_yaml::Value::Bool(b) => Ok(serde_json::Value::Bool(*b)),
        serde_yaml::Value::Sequence(seq) => {
            let items: Result<Vec<serde_json::Value>> = seq
                .iter()
                .map(|v| resolve_yaml_value(v, project_vars, store))
                .collect();
            Ok(serde_json::Value::Array(items?))
        }
        serde_yaml::Value::Mapping(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                let key = k.as_str().unwrap_or_default().to_string();
                let val = resolve_yaml_value(v, project_vars, store)?;
                obj.insert(key, val);
            }
            Ok(serde_json::Value::Object(obj))
        }
        serde_yaml::Value::Null => Ok(serde_json::Value::Null),
        _ => Ok(serde_json::Value::Null),
    }
}

/// Resolve ${var.*} and ${module.*.*} references in a string.
fn resolve_string_references(
    s: &str,
    project_vars: &HashMap<String, serde_yaml::Value>,
    store: &StateStore,
) -> Result<String> {
    let mut result = s.to_string();

    // Resolve ${var.*}
    let var_re = regex::Regex::new(r"\$\{var\.([^}]+)\}").unwrap();
    let var_replacements: Vec<(String, String)> = var_re
        .captures_iter(&result)
        .filter_map(|cap| {
            let full_match = cap[0].to_string();
            let var_name = &cap[1];
            project_vars.get(var_name).map(|v| {
                let replacement = match v {
                    serde_yaml::Value::String(s) => s.clone(),
                    other => serde_yaml::to_string(other)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                };
                (full_match, replacement)
            })
        })
        .collect();

    for (pattern, replacement) in var_replacements {
        result = result.replace(&pattern, &replacement);
    }

    // Resolve ${module.*.*}
    let mod_re = regex::Regex::new(r"\$\{module\.([^.}]+)\.([^}]+)\}").unwrap();
    let mod_replacements: Vec<(String, String)> = mod_re
        .captures_iter(&result)
        .filter_map(|cap| {
            let full_match = cap[0].to_string();
            let mod_name = &cap[1];
            let output_name = &cap[2];
            store
                .get_output(mod_name, output_name)
                .ok()
                .flatten()
                .map(|v| (full_match, v))
        })
        .collect();

    for (pattern, replacement) in mod_replacements {
        result = result.replace(&pattern, &replacement);
    }

    Ok(result)
}

/// Try to extract a module name from an error message.
fn extract_module_name_from_error(err: &str) -> Option<String> {
    let re = regex::Regex::new(r"module '([^']+)'").ok()?;
    re.captures(err).map(|cap| cap[1].to_string())
}
