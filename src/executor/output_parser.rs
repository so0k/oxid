use serde::Deserialize;

/// A single line of terraform JSON output.
#[derive(Debug, Deserialize)]
pub struct TerraformJsonLine {
    #[serde(rename = "@level")]
    pub level: Option<String>,
    #[serde(rename = "@message")]
    pub message: Option<String>,
    #[serde(rename = "@module")]
    pub module: Option<String>,
    #[serde(rename = "type")]
    pub line_type: Option<String>,
    pub change: Option<TerraformChange>,
    pub diagnostic: Option<TerraformDiagnostic>,
}

/// Terraform change summary from plan output.
#[derive(Debug, Deserialize)]
pub struct TerraformChange {
    pub resource: Option<TerraformResource>,
    pub action: Option<String>,
}

/// A terraform resource reference.
#[derive(Debug, Deserialize)]
pub struct TerraformResource {
    pub addr: Option<String>,
    pub resource_type: Option<String>,
    pub resource_name: Option<String>,
}

/// A terraform diagnostic (error/warning).
#[derive(Debug, Deserialize)]
pub struct TerraformDiagnostic {
    pub severity: Option<String>,
    pub summary: Option<String>,
    pub detail: Option<String>,
}

/// Summary of changes from a terraform plan.
#[derive(Debug, Default)]
pub struct PlanSummary {
    pub to_create: usize,
    pub to_update: usize,
    pub to_destroy: usize,
}

/// Parse terraform JSON output lines into a plan summary.
pub fn parse_plan_output(lines: &[String]) -> PlanSummary {
    let mut summary = PlanSummary::default();

    for line in lines {
        if let Ok(parsed) = serde_json::from_str::<TerraformJsonLine>(line) {
            if let Some(change) = &parsed.change {
                match change.action.as_deref() {
                    Some("create") => summary.to_create += 1,
                    Some("update") => summary.to_update += 1,
                    Some("delete") => summary.to_destroy += 1,
                    _ => {}
                }
            }
        }
    }

    summary
}

/// Extract errors from terraform JSON output.
pub fn extract_errors(lines: &[String]) -> Vec<String> {
    let mut errors = Vec::new();

    for line in lines {
        if let Ok(parsed) = serde_json::from_str::<TerraformJsonLine>(line) {
            if let Some(diag) = &parsed.diagnostic {
                if diag.severity.as_deref() == Some("error") {
                    let msg = diag
                        .summary
                        .as_deref()
                        .unwrap_or("Unknown error")
                        .to_string();
                    errors.push(msg);
                }
            }
        }
    }

    errors
}
