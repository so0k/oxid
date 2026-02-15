use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet, VecDeque};

use super::types::YamlConfig;

/// Validate the entire configuration for correctness.
pub fn validate(config: &YamlConfig) -> Result<()> {
    validate_module_references(config)?;
    validate_no_cycles(config)?;
    validate_variable_references(config)?;
    Ok(())
}

/// Ensure all depends_on references point to existing modules.
fn validate_module_references(config: &YamlConfig) -> Result<()> {
    let module_names: HashSet<&str> = config.project.modules.keys().map(|s| s.as_str()).collect();
    for (name, module) in &config.project.modules {
        for dep in &module.depends_on {
            if !module_names.contains(dep.as_str()) {
                bail!(
                    "Module '{}' depends on '{}', which does not exist",
                    name,
                    dep
                );
            }
        }
    }
    Ok(())
}

/// Detect circular dependencies using Kahn's algorithm.
fn validate_no_cycles(config: &YamlConfig) -> Result<()> {
    let modules = &config.project.modules;
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();

    for name in modules.keys() {
        in_degree.entry(name.as_str()).or_insert(0);
        adjacency.entry(name.as_str()).or_default();
    }

    for (name, module) in modules {
        for dep in &module.depends_on {
            adjacency
                .entry(dep.as_str())
                .or_default()
                .push(name.as_str());
            *in_degree.entry(name.as_str()).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<&str> = VecDeque::new();
    for (name, &degree) in &in_degree {
        if degree == 0 {
            queue.push_back(name);
        }
    }

    let mut visited = 0;
    while let Some(node) = queue.pop_front() {
        visited += 1;
        if let Some(neighbors) = adjacency.get(node) {
            for &neighbor in neighbors {
                let deg = in_degree.get_mut(neighbor).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(neighbor);
                }
            }
        }
    }

    if visited != modules.len() {
        bail!("Circular dependency detected in module configuration");
    }

    Ok(())
}

/// Validate that ${var.*} and ${module.*.*} references are resolvable.
fn validate_variable_references(config: &YamlConfig) -> Result<()> {
    let var_names: HashSet<&str> = config
        .project
        .variables
        .keys()
        .map(|s| s.as_str())
        .collect();
    let module_names: HashSet<&str> = config.project.modules.keys().map(|s| s.as_str()).collect();

    for (mod_name, module) in &config.project.modules {
        for (var_key, var_value) in &module.variables {
            let value_str = yaml_value_to_string(var_value);
            // Check ${var.*} references
            for cap in regex::Regex::new(r"\$\{var\.([^}]+)\}")
                .unwrap()
                .captures_iter(&value_str)
            {
                let ref_name = &cap[1];
                if !var_names.contains(ref_name) {
                    bail!(
                        "Module '{}' variable '{}' references undefined variable 'var.{}'",
                        mod_name,
                        var_key,
                        ref_name
                    );
                }
            }
            // Check ${module.*.*} references
            for cap in regex::Regex::new(r"\$\{module\.([^.}]+)\.([^}]+)\}")
                .unwrap()
                .captures_iter(&value_str)
            {
                let ref_module = &cap[1];
                if !module_names.contains(ref_module) {
                    bail!(
                        "Module '{}' variable '{}' references undefined module '{}'",
                        mod_name,
                        var_key,
                        ref_module
                    );
                }
            }
        }
    }

    Ok(())
}

/// Convert a serde_yaml::Value to a string for reference scanning.
fn yaml_value_to_string(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::String(s) => s.clone(),
        other => serde_yaml::to_string(other).unwrap_or_default(),
    }
}
