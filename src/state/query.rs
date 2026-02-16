use anyhow::{bail, Result};

use super::backend::StateBackend;

/// Output format for query results.
#[derive(Debug, Clone, Copy)]
pub enum QueryFormat {
    Table,
    Json,
    Csv,
}

impl QueryFormat {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => QueryFormat::Json,
            "csv" => QueryFormat::Csv,
            _ => QueryFormat::Table,
        }
    }
}

/// Execute a user query and format the results.
pub async fn execute_query(
    backend: &dyn StateBackend,
    sql: &str,
    format: QueryFormat,
) -> Result<String> {
    // Basic safety: only allow SELECT queries
    let trimmed = sql.trim().to_uppercase();
    if !trimmed.starts_with("SELECT") {
        bail!("Only SELECT queries are allowed. Use oxid commands for mutations.");
    }

    let rows = backend.query_raw(sql).await?;

    if rows.is_empty() {
        return Ok("No results.".to_string());
    }

    match format {
        QueryFormat::Table => format_table(&rows),
        QueryFormat::Json => format_json(&rows),
        QueryFormat::Csv => format_csv(&rows),
    }
}

fn format_table(rows: &[serde_json::Value]) -> Result<String> {
    let first = rows[0].as_object().unwrap();
    let columns: Vec<String> = first.keys().cloned().collect();

    // Calculate column widths
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
    for row in rows {
        if let Some(obj) = row.as_object() {
            for (i, col) in columns.iter().enumerate() {
                let val = obj.get(col).map(value_to_display).unwrap_or_default();
                widths[i] = widths[i].max(val.len());
            }
        }
    }

    let mut output = String::new();

    // Header
    let header: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{:width$}", c.to_uppercase(), width = widths[i]))
        .collect();
    output.push_str(&header.join(" | "));
    output.push('\n');

    // Separator
    let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    output.push_str(&sep.join("-+-"));
    output.push('\n');

    // Rows
    for row in rows {
        if let Some(obj) = row.as_object() {
            let vals: Vec<String> = columns
                .iter()
                .enumerate()
                .map(|(i, col)| {
                    let val = obj.get(col).map(value_to_display).unwrap_or_default();
                    format!("{:width$}", val, width = widths[i])
                })
                .collect();
            output.push_str(&vals.join(" | "));
            output.push('\n');
        }
    }

    output.push_str(&format!("\n({} rows)", rows.len()));
    Ok(output)
}

fn format_json(rows: &[serde_json::Value]) -> Result<String> {
    Ok(serde_json::to_string_pretty(rows)?)
}

fn format_csv(rows: &[serde_json::Value]) -> Result<String> {
    let first = rows[0].as_object().unwrap();
    let columns: Vec<String> = first.keys().cloned().collect();

    let mut output = String::new();

    // Header
    output.push_str(&columns.join(","));
    output.push('\n');

    // Rows
    for row in rows {
        if let Some(obj) = row.as_object() {
            let vals: Vec<String> = columns
                .iter()
                .map(|col| {
                    let val = obj.get(col).map(value_to_display).unwrap_or_default();
                    if val.contains(',') || val.contains('"') {
                        format!("\"{}\"", val.replace('"', "\"\""))
                    } else {
                        val
                    }
                })
                .collect();
            output.push_str(&vals.join(","));
            output.push('\n');
        }
    }

    Ok(output)
}

fn value_to_display(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}
