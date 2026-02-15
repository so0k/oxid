use std::collections::HashMap;

use anyhow::Result;

use super::types::*;

/// Convert a legacy YamlConfig into the unified WorkspaceConfig IR.
pub fn yaml_to_workspace(yaml: &YamlConfig) -> Result<WorkspaceConfig> {
    let mut workspace = WorkspaceConfig::default();

    // Convert project-level variables to VariableConfig
    for (name, value) in &yaml.project.variables {
        workspace.variables.push(VariableConfig {
            name: name.clone(),
            var_type: None,
            default: Some(yaml_value_to_expression(value)),
            description: None,
            sensitive: false,
            validation: vec![],
        });
    }

    // Convert YAML modules to ModuleRef
    for (name, module) in &yaml.project.modules {
        let mut variables = HashMap::new();
        for (k, v) in &module.variables {
            variables.insert(k.clone(), yaml_value_to_expression(v));
        }

        workspace.modules.push(ModuleRef {
            name: name.clone(),
            source: module.source.clone(),
            version: module.version.clone(),
            depends_on: module.depends_on.clone(),
            variables,
            providers: HashMap::new(),
            outputs: module.outputs.clone(),
        });
    }

    Ok(workspace)
}

/// Convert a serde_yaml::Value into an Expression.
fn yaml_value_to_expression(value: &serde_yaml::Value) -> Expression {
    match value {
        serde_yaml::Value::Null => Expression::Literal(Value::Null),
        serde_yaml::Value::Bool(b) => Expression::Literal(Value::Bool(*b)),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Expression::Literal(Value::Int(i))
            } else if let Some(f) = n.as_f64() {
                Expression::Literal(Value::Float(f))
            } else {
                Expression::Literal(Value::Null)
            }
        }
        serde_yaml::Value::String(s) => {
            // Check for variable references: ${var.xxx} or ${module.xxx.yyy}
            if s.contains("${") {
                parse_yaml_template(s)
            } else {
                Expression::Literal(Value::String(s.clone()))
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            let items: Vec<Value> = seq.iter().map(|v| yaml_value_to_value(v)).collect();
            Expression::Literal(Value::List(items))
        }
        serde_yaml::Value::Mapping(map) => {
            let entries: Vec<(String, Value)> = map
                .iter()
                .map(|(k, v)| {
                    let key = k.as_str().unwrap_or("").to_string();
                    (key, yaml_value_to_value(v))
                })
                .collect();
            Expression::Literal(Value::Map(entries))
        }
        _ => Expression::Literal(Value::Null),
    }
}

/// Convert a serde_yaml::Value to our Value type.
fn yaml_value_to_value(value: &serde_yaml::Value) -> Value {
    match value {
        serde_yaml::Value::Null => Value::Null,
        serde_yaml::Value::Bool(b) => Value::Bool(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Null
            }
        }
        serde_yaml::Value::String(s) => Value::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            let items: Vec<Value> = seq.iter().map(|v| yaml_value_to_value(v)).collect();
            Value::List(items)
        }
        serde_yaml::Value::Mapping(map) => {
            let entries: Vec<(String, Value)> = map
                .iter()
                .map(|(k, v)| {
                    let key = k.as_str().unwrap_or("").to_string();
                    (key, yaml_value_to_value(v))
                })
                .collect();
            Value::Map(entries)
        }
        _ => Value::Null,
    }
}

/// Parse a YAML string with ${...} references into a Template expression.
fn parse_yaml_template(s: &str) -> Expression {
    let mut parts = Vec::new();
    let mut remaining = s;

    while let Some(start) = remaining.find("${") {
        if start > 0 {
            parts.push(TemplatePart::Literal(remaining[..start].to_string()));
        }

        if let Some(end) = remaining[start + 2..].find('}') {
            let ref_str = &remaining[start + 2..start + 2 + end];
            let ref_parts: Vec<String> = ref_str.split('.').map(|s| s.trim().to_string()).collect();
            parts.push(TemplatePart::Interpolation(Box::new(
                Expression::Reference(ref_parts),
            )));
            remaining = &remaining[start + 2 + end + 1..];
        } else {
            parts.push(TemplatePart::Literal(remaining.to_string()));
            remaining = "";
        }
    }

    if !remaining.is_empty() {
        parts.push(TemplatePart::Literal(remaining.to_string()));
    }

    // If the template is just a single reference, unwrap it
    if parts.len() == 1 {
        if let TemplatePart::Interpolation(expr) = &parts[0] {
            return *expr.clone();
        }
    }

    Expression::Template(parts)
}
