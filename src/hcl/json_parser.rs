use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value as JsonValue;

use super::parser::parse_hcl_body;
use crate::config::types::WorkspaceConfig;

/// Known block types and their expected label counts, matching Terraform's configFileSchema.
const BLOCK_SCHEMA: &[(&str, usize)] = &[
    ("resource", 2),
    ("data", 2),
    ("provider", 1),
    ("variable", 1),
    ("output", 1),
    ("module", 1),
    ("terraform", 0),
    ("locals", 0),
];

/// Parse a `.tf.json` file into a WorkspaceConfig.
///
/// Converts JSON → hcl::Body → reuses the existing parse_hcl_body() logic.
pub fn parse_tf_json(content: &str, file_path: &Path) -> Result<WorkspaceConfig> {
    let root: JsonValue = serde_json::from_str(content)
        .with_context(|| format!("Failed to parse JSON in: {}", file_path.display()))?;

    let root_obj = root
        .as_object()
        .with_context(|| format!("Expected JSON object at root of {}", file_path.display()))?;

    let body = json_to_body(root_obj, file_path)?;
    parse_hcl_body(body, file_path)
}

/// Convert a root-level JSON object into an hcl::Body.
///
/// Each top-level key is matched against BLOCK_SCHEMA to determine how many
/// label levels to peel before reaching the block body.
fn json_to_body(root: &serde_json::Map<String, JsonValue>, file_path: &Path) -> Result<hcl::Body> {
    let mut body_builder = hcl::Body::builder();

    for (key, value) in root {
        // Strip "//" comment keys (FR-010)
        if key == "//" {
            continue;
        }

        if let Some(&(_, label_count)) = BLOCK_SCHEMA.iter().find(|&&(name, _)| name == key) {
            if key == "locals" {
                // locals is special: top-level key-value pairs become attributes
                let blocks = convert_locals_block(value, file_path)?;
                for block in blocks {
                    body_builder = body_builder.add_block(block);
                }
            } else {
                let blocks = convert_block(key, value, label_count, file_path)?;
                for block in blocks {
                    body_builder = body_builder.add_block(block);
                }
            }
        } else {
            tracing::debug!(
                "Ignoring unknown top-level key '{}' in {}",
                key,
                file_path.display()
            );
        }
    }

    Ok(body_builder.build())
}

/// Recursively peel labels from nested JSON objects to produce HCL blocks.
///
/// For a block type with N labels, this peels N levels of JSON nesting.
/// At any level, if the value is a JSON array, each element produces a separate block.
fn convert_block(
    block_type: &str,
    value: &JsonValue,
    labels_remaining: usize,
    file_path: &Path,
) -> Result<Vec<hcl::Block>> {
    if labels_remaining == 0 {
        // No more labels to peel — this value IS the block body
        return convert_block_body_values(block_type, &[], value, file_path);
    }

    // Peel one label level
    match value {
        JsonValue::Object(obj) => {
            let mut blocks = Vec::new();
            for (label, inner_value) in obj {
                if label == "//" {
                    continue;
                }
                if labels_remaining == 1 {
                    // Last label — inner_value is the block body (or array of bodies)
                    let mut produced =
                        convert_block_body_values(block_type, &[label], inner_value, file_path)?;
                    blocks.append(&mut produced);
                } else {
                    // More labels to peel
                    match inner_value {
                        JsonValue::Object(inner_obj) => {
                            for (label2, deeper_value) in inner_obj {
                                if label2 == "//" {
                                    continue;
                                }
                                let mut produced = convert_block_body_values(
                                    block_type,
                                    &[label, label2],
                                    deeper_value,
                                    file_path,
                                )?;
                                blocks.append(&mut produced);
                            }
                        }
                        JsonValue::Null => {} // skip null values
                        other => {
                            anyhow::bail!(
                                "Expected object for block type '{}' label peeling in {}, got {}",
                                block_type,
                                file_path.display(),
                                json_type_name(other)
                            );
                        }
                    }
                }
            }
            Ok(blocks)
        }
        JsonValue::Null => Ok(Vec::new()),
        other => {
            anyhow::bail!(
                "Expected object for block type '{}' in {}, got {}",
                block_type,
                file_path.display(),
                json_type_name(other)
            );
        }
    }
}

/// Convert a JSON value into one or more HCL blocks with the given type and labels.
/// Handles the array-vs-object disambiguation (FR-012).
fn convert_block_body_values(
    block_type: &str,
    labels: &[&str],
    value: &JsonValue,
    file_path: &Path,
) -> Result<Vec<hcl::Block>> {
    match value {
        JsonValue::Array(arr) => {
            // Array → each element is a separate block instance
            let mut blocks = Vec::new();
            for element in arr {
                let block = build_single_block(block_type, labels, element, file_path)?;
                blocks.push(block);
            }
            Ok(blocks)
        }
        JsonValue::Object(_) => {
            let block = build_single_block(block_type, labels, value, file_path)?;
            Ok(vec![block])
        }
        JsonValue::Null => Ok(Vec::new()),
        other => {
            anyhow::bail!(
                "Expected object or array for block '{}' body in {}, got {}",
                block_type,
                file_path.display(),
                json_type_name(other)
            );
        }
    }
}

/// Build a single HCL block from a JSON object body.
fn build_single_block(
    block_type: &str,
    labels: &[&str],
    body_value: &JsonValue,
    file_path: &Path,
) -> Result<hcl::Block> {
    let obj = body_value.as_object().with_context(|| {
        format!(
            "Expected object for block '{}' body in {}",
            block_type,
            file_path.display()
        )
    })?;

    let mut builder = hcl::Block::builder(block_type);
    for label in labels {
        builder = builder.add_label(*label);
    }

    for (key, val) in obj {
        if key == "//" {
            continue;
        }

        // Check if this key represents a nested block (value is object or array of objects)
        // Nested blocks within resources follow the same pattern as top-level blocks.
        // We need to distinguish attributes from nested blocks:
        // - Known nested block types (lifecycle, provisioner, backend, required_providers, etc.)
        //   are always blocks
        // - For resource/data bodies, objects that look like block bodies are treated as
        //   attributes (maps) unless they are known nested block names
        if is_nested_block(block_type, key, val) {
            let nested_blocks = convert_nested_blocks(key, val, file_path)?;
            for nested in nested_blocks {
                builder = builder.add_block(nested);
            }
        } else {
            let expr = json_value_to_expression(val);
            builder = builder.add_attribute((key.as_str(), expr));
        }
    }

    Ok(builder.build())
}

/// Determine if a key within a block body represents a nested block.
///
/// In Terraform JSON, nested blocks are represented as objects or arrays of objects
/// with specific known keys. This checks against known nested block names.
fn is_nested_block(parent_block_type: &str, key: &str, _value: &JsonValue) -> bool {
    // Known nested blocks by parent type
    let is_known_nested = match parent_block_type {
        "resource" | "data" => matches!(key, "lifecycle" | "provisioner" | "connection"),
        "terraform" => matches!(key, "backend" | "required_providers" | "cloud"),
        _ => false,
    };

    if is_known_nested {
        return true;
    }

    // For terraform > backend, the value is {"backend_type": {config}} which is a labeled block
    // For provisioner blocks in resources, value is {"provisioner_type": {config}}
    false
}

/// Convert a nested block value (object or array of objects) into HCL blocks.
fn convert_nested_blocks(
    key: &str,
    value: &JsonValue,
    file_path: &Path,
) -> Result<Vec<hcl::Block>> {
    match key {
        "backend" => {
            // backend is a labeled block: {"local": {"path": "..."}}
            if let JsonValue::Object(obj) = value {
                let mut blocks = Vec::new();
                for (backend_type, config) in obj {
                    if backend_type == "//" {
                        continue;
                    }
                    let block = build_single_block("backend", &[backend_type], config, file_path)?;
                    blocks.push(block);
                }
                Ok(blocks)
            } else {
                Ok(Vec::new())
            }
        }
        "required_providers" => {
            // required_providers is an unlabeled block with attributes
            if let JsonValue::Object(obj) = value {
                let mut builder = hcl::Block::builder("required_providers");
                for (provider_name, constraint) in obj {
                    if provider_name == "//" {
                        continue;
                    }
                    let expr = json_value_to_expression(constraint);
                    builder = builder.add_attribute((provider_name.as_str(), expr));
                }
                Ok(vec![builder.build()])
            } else {
                Ok(Vec::new())
            }
        }
        "provisioner" => {
            // provisioner is a labeled block: {"local-exec": {"command": "echo hello"}}
            if let JsonValue::Object(obj) = value {
                let mut blocks = Vec::new();
                for (prov_type, config) in obj {
                    if prov_type == "//" {
                        continue;
                    }
                    match config {
                        JsonValue::Array(arr) => {
                            for item in arr {
                                let block = build_single_block(
                                    "provisioner",
                                    &[prov_type],
                                    item,
                                    file_path,
                                )?;
                                blocks.push(block);
                            }
                        }
                        _ => {
                            let block =
                                build_single_block("provisioner", &[prov_type], config, file_path)?;
                            blocks.push(block);
                        }
                    }
                }
                Ok(blocks)
            } else {
                Ok(Vec::new())
            }
        }
        _ => {
            // Generic nested block (lifecycle, connection, etc.)
            match value {
                JsonValue::Array(arr) => {
                    let mut blocks = Vec::new();
                    for item in arr {
                        if let JsonValue::Object(obj) = item {
                            let mut builder = hcl::Block::builder(key);
                            for (k, v) in obj {
                                if k == "//" {
                                    continue;
                                }
                                let expr = json_value_to_expression(v);
                                builder = builder.add_attribute((k.as_str(), expr));
                            }
                            blocks.push(builder.build());
                        }
                    }
                    Ok(blocks)
                }
                JsonValue::Object(obj) => {
                    let mut builder = hcl::Block::builder(key);
                    for (k, v) in obj {
                        if k == "//" {
                            continue;
                        }
                        let expr = json_value_to_expression(v);
                        builder = builder.add_attribute((k.as_str(), expr));
                    }
                    Ok(vec![builder.build()])
                }
                _ => Ok(Vec::new()),
            }
        }
    }
}

/// Convert a "locals" block. In JSON, locals is an object of key-value pairs
/// that become attributes in a `locals {}` block.
fn convert_locals_block(value: &JsonValue, file_path: &Path) -> Result<Vec<hcl::Block>> {
    let obj = value.as_object().with_context(|| {
        format!(
            "Expected object for 'locals' block in {}",
            file_path.display()
        )
    })?;

    let mut builder = hcl::Block::builder("locals");
    for (key, val) in obj {
        if key == "//" {
            continue;
        }
        let expr = json_value_to_expression(val);
        builder = builder.add_attribute((key.as_str(), expr));
    }

    Ok(vec![builder.build()])
}

/// Convert a serde_json::Value into an hcl::Expression.
fn json_value_to_expression(value: &JsonValue) -> hcl::Expression {
    match value {
        JsonValue::Null => hcl::Expression::Null,
        JsonValue::Bool(b) => hcl::Expression::Bool(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                hcl::Expression::Number(hcl::Number::from(i))
            } else if let Some(f) = n.as_f64() {
                hcl::Number::from_f64(f)
                    .map(hcl::Expression::Number)
                    .unwrap_or(hcl::Expression::Null)
            } else {
                hcl::Expression::Null
            }
        }
        JsonValue::String(s) => hcl::Expression::String(s.clone()),
        JsonValue::Array(arr) => {
            let items: Vec<hcl::Expression> = arr.iter().map(json_value_to_expression).collect();
            hcl::Expression::Array(items)
        }
        JsonValue::Object(obj) => {
            let entries: Vec<(hcl::expr::ObjectKey, hcl::Expression)> = obj
                .iter()
                .filter(|(k, _)| k.as_str() != "//")
                .map(|(k, v)| {
                    (
                        hcl::expr::ObjectKey::from(k.as_str()),
                        json_value_to_expression(v),
                    )
                })
                .collect();
            hcl::Expression::Object(entries.into_iter().collect())
        }
    }
}

/// Return a human-readable name for a JSON value type (for error messages).
fn json_type_name(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}
