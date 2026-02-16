use std::collections::HashSet;

use colored::Colorize;

use crate::config::types::*;

/// Validation error for count/for_each reference issues.
#[derive(Debug)]
pub struct ValidationError {
    pub source: String,
    pub ref_address: String,
    pub attr_accessed: String,
}

/// Print validation errors with colored, formatted output.
pub fn print_validation_errors(errors: &[ValidationError]) {
    for (i, err) in errors.iter().enumerate() {
        if i > 0 {
            eprintln!();
        }
        eprintln!(
            "{} {}",
            "Error:".red().bold(),
            "Missing resource instance key".bold()
        );
        eprintln!();
        eprintln!(
            "  {} {}",
            "on".dimmed(),
            err.source.yellow()
        );
        eprintln!();
        eprintln!(
            "  Because {} has {} set, its attributes must be",
            err.ref_address.cyan().bold(),
            "\"count\" or \"for_each\"".white().bold()
        );
        eprintln!("  accessed on specific instances.");
        eprintln!();
        eprintln!("  {} to correlate with indices of a referring resource:", "For example,".dimmed());
        eprintln!(
            "      {}",
            format!("{}[count.index].{}", err.ref_address, err.attr_accessed).green()
        );
        eprintln!();
        eprintln!("  {} to access all instances:", "Or".dimmed());
        eprintln!(
            "      {}",
            format!("{}[*].{}", err.ref_address, err.attr_accessed).green()
        );
    }
    eprintln!();
    eprintln!(
        "{} Configuration contains {} error(s). Fix the errors above to continue.",
        "Error:".red().bold(),
        errors.len().to_string().red().bold()
    );
}

/// Validate that references to resources with count/for_each include an index or splat.
///
/// Terraform requires that when a resource has `count` or `for_each`, any reference to it
/// must use an index (e.g. `aws_instance.main[0].id`) or splat (`aws_instance.main[*].id`).
/// A bare reference like `aws_instance.main.id` is invalid.
pub fn validate_count_references(workspace: &WorkspaceConfig) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    // Build set of multi-instance resources (those with count or for_each)
    let mut multi_instance: HashSet<String> = HashSet::new();
    for resource in &workspace.resources {
        if resource.count.is_some() || resource.for_each.is_some() {
            multi_instance.insert(format!("{}.{}", resource.resource_type, resource.name));
        }
    }
    for data_source in &workspace.data_sources {
        if data_source.count.is_some() || data_source.for_each.is_some() {
            multi_instance.insert(format!(
                "data.{}.{}",
                data_source.resource_type, data_source.name
            ));
        }
    }

    if multi_instance.is_empty() {
        return errors;
    }

    // Check resource attributes
    for resource in &workspace.resources {
        let source_addr = format!("{}.{}", resource.resource_type, resource.name);
        for (attr_name, expr) in &resource.attributes {
            check_expression(
                expr,
                &multi_instance,
                &source_addr,
                Some(attr_name),
                &mut errors,
            );
        }
        if let Some(ref count_expr) = resource.count {
            check_expression(
                count_expr,
                &multi_instance,
                &source_addr,
                Some("count"),
                &mut errors,
            );
        }
        if let Some(ref for_each_expr) = resource.for_each {
            check_expression(
                for_each_expr,
                &multi_instance,
                &source_addr,
                Some("for_each"),
                &mut errors,
            );
        }
    }

    // Check data source attributes
    for data_source in &workspace.data_sources {
        let source_addr = format!(
            "data.{}.{}",
            data_source.resource_type, data_source.name
        );
        for (attr_name, expr) in &data_source.attributes {
            check_expression(
                expr,
                &multi_instance,
                &source_addr,
                Some(attr_name),
                &mut errors,
            );
        }
    }

    // Check output values
    for output in &workspace.outputs {
        let source_addr = format!("output.{}", output.name);
        check_expression(
            &output.value,
            &multi_instance,
            &source_addr,
            None,
            &mut errors,
        );
    }

    // Check locals
    for (name, expr) in &workspace.locals {
        let source_addr = format!("local.{}", name);
        check_expression(expr, &multi_instance, &source_addr, None, &mut errors);
    }

    errors
}

fn check_expression(
    expr: &Expression,
    multi_instance: &HashSet<String>,
    source_addr: &str,
    attr_name: Option<&str>,
    errors: &mut Vec<ValidationError>,
) {
    match expr {
        Expression::Reference(parts) => {
            check_reference(parts, multi_instance, source_addr, attr_name, errors);
        }
        Expression::Literal(val) => {
            check_value(val, multi_instance, source_addr, attr_name, errors);
        }
        Expression::FunctionCall { args, .. } => {
            for arg in args {
                check_expression(arg, multi_instance, source_addr, attr_name, errors);
            }
        }
        Expression::Conditional {
            condition,
            true_val,
            false_val,
        } => {
            check_expression(condition, multi_instance, source_addr, attr_name, errors);
            check_expression(true_val, multi_instance, source_addr, attr_name, errors);
            check_expression(false_val, multi_instance, source_addr, attr_name, errors);
        }
        Expression::ForExpr {
            collection,
            key_expr,
            value_expr,
            condition,
            ..
        } => {
            check_expression(collection, multi_instance, source_addr, attr_name, errors);
            if let Some(k) = key_expr {
                check_expression(k, multi_instance, source_addr, attr_name, errors);
            }
            check_expression(value_expr, multi_instance, source_addr, attr_name, errors);
            if let Some(c) = condition {
                check_expression(c, multi_instance, source_addr, attr_name, errors);
            }
        }
        Expression::Template(parts) => {
            for part in parts {
                match part {
                    TemplatePart::Interpolation(e) | TemplatePart::Directive(e) => {
                        check_expression(e, multi_instance, source_addr, attr_name, errors);
                    }
                    TemplatePart::Literal(_) => {}
                }
            }
        }
        Expression::Index { collection, key } => {
            check_expression(collection, multi_instance, source_addr, attr_name, errors);
            check_expression(key, multi_instance, source_addr, attr_name, errors);
        }
        Expression::GetAttr { object, .. } => {
            check_expression(object, multi_instance, source_addr, attr_name, errors);
        }
        Expression::BinaryOp { left, right, .. } => {
            check_expression(left, multi_instance, source_addr, attr_name, errors);
            check_expression(right, multi_instance, source_addr, attr_name, errors);
        }
        Expression::UnaryOp { operand, .. } => {
            check_expression(operand, multi_instance, source_addr, attr_name, errors);
        }
        Expression::Splat { source, each } => {
            check_expression(source, multi_instance, source_addr, attr_name, errors);
            check_expression(each, multi_instance, source_addr, attr_name, errors);
        }
    }
}

/// Check a Reference for bare access to a multi-instance resource.
fn check_reference(
    parts: &[String],
    multi_instance: &HashSet<String>,
    source_addr: &str,
    attr_name: Option<&str>,
    errors: &mut Vec<ValidationError>,
) {
    if parts.len() < 3 {
        return;
    }

    match parts[0].as_str() {
        "var" | "local" | "each" | "count" | "path" | "terraform" | "self" | "module" => return,
        "data" if parts.len() >= 4 => {
            let ref_address = format!("data.{}.{}", parts[1], parts[2]);
            if multi_instance.contains(&ref_address) && !parts[3].starts_with('[') {
                push_missing_key_error(
                    errors,
                    source_addr,
                    attr_name,
                    &ref_address,
                    &parts[3],
                );
            }
        }
        _ => {
            let ref_address = format!("{}.{}", parts[0], parts[1]);
            if multi_instance.contains(&ref_address) && !parts[2].starts_with('[') {
                push_missing_key_error(
                    errors,
                    source_addr,
                    attr_name,
                    &ref_address,
                    &parts[2],
                );
            }
        }
    }
}

fn push_missing_key_error(
    errors: &mut Vec<ValidationError>,
    source_addr: &str,
    attr_name: Option<&str>,
    ref_address: &str,
    attr_accessed: &str,
) {
    let source = match attr_name {
        Some(a) => format!("{}, in attribute \"{}\"", source_addr, a),
        None => source_addr.to_string(),
    };
    errors.push(ValidationError {
        source,
        ref_address: ref_address.to_string(),
        attr_accessed: attr_accessed.to_string(),
    });
}

/// Check literal values for ${...} references to multi-instance resources.
fn check_value(
    val: &Value,
    multi_instance: &HashSet<String>,
    source_addr: &str,
    attr_name: Option<&str>,
    errors: &mut Vec<ValidationError>,
) {
    match val {
        Value::String(s) => {
            let mut remaining = s.as_str();
            while let Some(start) = remaining.find("${") {
                if let Some(end) = remaining[start + 2..].find('}') {
                    let ref_str = &remaining[start + 2..start + 2 + end];
                    let parts: Vec<String> =
                        ref_str.split('.').map(|p| p.trim().to_string()).collect();
                    check_reference(&parts, multi_instance, source_addr, attr_name, errors);
                    remaining = &remaining[start + 2 + end + 1..];
                } else {
                    break;
                }
            }
        }
        Value::List(items) => {
            for item in items {
                check_value(item, multi_instance, source_addr, attr_name, errors);
            }
        }
        Value::Map(entries) => {
            for (_, v) in entries {
                check_value(v, multi_instance, source_addr, attr_name, errors);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_workspace_with_count() -> WorkspaceConfig {
        WorkspaceConfig {
            resources: vec![ResourceConfig {
                resource_type: "aws_instance".to_string(),
                name: "main".to_string(),
                provider_ref: None,
                count: Some(Expression::Literal(Value::Int(3))),
                for_each: None,
                depends_on: vec![],
                lifecycle: LifecycleConfig::default(),
                attributes: HashMap::new(),
                provisioners: vec![],
                source_location: None,
            }],
            outputs: vec![OutputConfig {
                name: "instance_id".to_string(),
                value: Expression::Reference(vec![
                    "aws_instance".to_string(),
                    "main".to_string(),
                    "id".to_string(),
                ]),
                description: None,
                sensitive: false,
                depends_on: vec![],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn bare_reference_to_counted_resource_errors() {
        let ws = make_workspace_with_count();
        let errors = validate_count_references(&ws);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].source.contains("output.instance_id"));
        assert_eq!(errors[0].ref_address, "aws_instance.main");
        assert_eq!(errors[0].attr_accessed, "id");
    }

    #[test]
    fn indexed_reference_to_counted_resource_ok() {
        let mut ws = make_workspace_with_count();
        ws.outputs[0].value = Expression::Reference(vec![
            "aws_instance".to_string(),
            "main".to_string(),
            "[0]".to_string(),
            "id".to_string(),
        ]);
        let errors = validate_count_references(&ws);
        assert!(errors.is_empty());
    }

    #[test]
    fn splat_reference_to_counted_resource_ok() {
        let mut ws = make_workspace_with_count();
        ws.outputs[0].value = Expression::Reference(vec![
            "aws_instance".to_string(),
            "main".to_string(),
            "[*]".to_string(),
            "id".to_string(),
        ]);
        let errors = validate_count_references(&ws);
        assert!(errors.is_empty());
    }

    #[test]
    fn bare_reference_to_non_counted_resource_ok() {
        let mut ws = make_workspace_with_count();
        ws.resources[0].count = None; // remove count
        let errors = validate_count_references(&ws);
        assert!(errors.is_empty());
    }

    #[test]
    fn interpolated_bare_reference_errors() {
        let mut ws = make_workspace_with_count();
        ws.outputs[0].value = Expression::Literal(Value::String(
            "${aws_instance.main.id}".to_string(),
        ));
        let errors = validate_count_references(&ws);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].ref_address, "aws_instance.main");
    }

    #[test]
    fn no_multi_instance_resources_ok() {
        let ws = WorkspaceConfig {
            resources: vec![ResourceConfig {
                resource_type: "aws_vpc".to_string(),
                name: "main".to_string(),
                provider_ref: None,
                count: None,
                for_each: None,
                depends_on: vec![],
                lifecycle: LifecycleConfig::default(),
                attributes: HashMap::new(),
                provisioners: vec![],
                source_location: None,
            }],
            outputs: vec![OutputConfig {
                name: "vpc_id".to_string(),
                value: Expression::Reference(vec![
                    "aws_vpc".to_string(),
                    "main".to_string(),
                    "id".to_string(),
                ]),
                description: None,
                sensitive: false,
                depends_on: vec![],
            }],
            ..Default::default()
        };
        let errors = validate_count_references(&ws);
        assert!(errors.is_empty());
    }

    #[test]
    fn var_and_local_references_skipped() {
        let ws = WorkspaceConfig {
            resources: vec![ResourceConfig {
                resource_type: "aws_instance".to_string(),
                name: "main".to_string(),
                provider_ref: None,
                count: Some(Expression::Literal(Value::Int(3))),
                for_each: None,
                depends_on: vec![],
                lifecycle: LifecycleConfig::default(),
                attributes: HashMap::new(),
                provisioners: vec![],
                source_location: None,
            }],
            outputs: vec![OutputConfig {
                name: "region".to_string(),
                value: Expression::Reference(vec![
                    "var".to_string(),
                    "aws_region".to_string(),
                ]),
                description: None,
                sensitive: false,
                depends_on: vec![],
            }],
            ..Default::default()
        };
        let errors = validate_count_references(&ws);
        assert!(errors.is_empty());
    }
}
