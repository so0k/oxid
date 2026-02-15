use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing;

use crate::config::types::{YamlModuleConfig, YamlSettings};
use crate::executor::output_parser;

/// Result of a terraform command execution.
#[derive(Debug)]
pub struct TerraformResult {
    pub exit_code: i32,
    pub stdout_lines: Vec<String>,
    pub stderr_lines: Vec<String>,
}

impl TerraformResult {
    /// Extract a human-readable error message from the result.
    pub fn error_message(&self) -> String {
        // First try extracting errors from JSON stdout (terraform -json output)
        let json_errors = output_parser::extract_errors(&self.stdout_lines);
        if !json_errors.is_empty() {
            return json_errors.join("; ");
        }

        // Fall back to stderr
        let stderr = self.stderr_lines.join("\n");
        if !stderr.is_empty() {
            return stderr;
        }

        // Fall back to raw stdout for non-JSON error output
        let meaningful: Vec<&String> = self
            .stdout_lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .collect();
        if !meaningful.is_empty() {
            return meaningful
                .iter()
                .rev()
                .take(5)
                .rev()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n");
        }

        format!("exit code {}", self.exit_code)
    }
}

/// Generate a main.tf file for a module in its working directory.
pub fn generate_terraform_files(
    module_name: &str,
    module_config: &YamlModuleConfig,
    resolved_variables: &HashMap<String, serde_json::Value>,
    module_dir: &Path,
    aws_region: Option<&str>,
) -> Result<()> {
    std::fs::create_dir_all(module_dir)?;

    let mut tf = String::new();

    // Terraform block
    tf.push_str("terraform {\n");
    tf.push_str("  required_providers {\n");
    tf.push_str("    aws = {\n");
    tf.push_str("      source = \"hashicorp/aws\"\n");
    tf.push_str("    }\n");
    tf.push_str("  }\n");
    tf.push_str("}\n\n");

    // Provider block with region
    let region = aws_region.unwrap_or("us-east-1");
    tf.push_str("provider \"aws\" {\n");
    tf.push_str(&format!("  region = \"{}\"\n", region));
    tf.push_str("}\n\n");

    // Module block
    tf.push_str("module \"this\" {\n");
    tf.push_str(&format!("  source  = \"{}\"\n", module_config.source));
    if let Some(version) = &module_config.version {
        tf.push_str(&format!("  version = \"{}\"\n", version));
    }
    tf.push('\n');

    // Variables
    for (key, value) in resolved_variables {
        let hcl_value = json_to_hcl(value);
        tf.push_str(&format!("  {} = {}\n", key, hcl_value));
    }

    tf.push_str("}\n");

    // Output blocks
    for output in &module_config.outputs {
        tf.push_str(&format!(
            "\noutput \"{}\" {{\n  value = module.this.{}\n}}\n",
            output, output
        ));
    }

    let tf_path = module_dir.join("main.tf");
    std::fs::write(&tf_path, &tf)
        .with_context(|| format!("Failed to write {}", tf_path.display()))?;

    tracing::debug!(module = module_name, path = %tf_path.display(), "Generated terraform file");
    Ok(())
}

/// Convert a JSON value to HCL syntax.
fn json_to_hcl(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => format!("\"{}\"", s),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(json_to_hcl).collect();
            format!("[{}]", items.join(", "))
        }
        serde_json::Value::Object(map) => {
            let entries: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("  {} = {}", k, json_to_hcl(v)))
                .collect();
            format!("{{\n{}\n}}", entries.join("\n"))
        }
        serde_json::Value::Null => "null".to_string(),
    }
}

/// Run a terraform/tofu command in the given directory.
pub async fn run_terraform(
    settings: &YamlSettings,
    module_dir: &Path,
    args: &[&str],
) -> Result<TerraformResult> {
    let binary = &settings.terraform_binary;
    tracing::info!(binary = binary, args = ?args, dir = %module_dir.display(), "Running terraform");

    let mut cmd = Command::new(binary);
    cmd.args(args)
        .current_dir(module_dir)
        .env("TF_IN_AUTOMATION", "1")
        .env("TF_INPUT", "0")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn {} in {}", binary, module_dir.display()))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let stdout_reader = BufReader::new(stdout);
    let stderr_reader = BufReader::new(stderr);

    let mut stdout_stream = stdout_reader.lines();
    let mut stderr_stream = stderr_reader.lines();

    // Read stdout
    let stdout_handle = tokio::spawn(async move {
        let mut lines = Vec::new();
        while let Ok(Some(line)) = stdout_stream.next_line().await {
            tracing::debug!(stream = "stdout", "{}", line);
            lines.push(line);
        }
        lines
    });

    // Read stderr
    let stderr_handle = tokio::spawn(async move {
        let mut lines = Vec::new();
        while let Ok(Some(line)) = stderr_stream.next_line().await {
            tracing::debug!(stream = "stderr", "{}", line);
            lines.push(line);
        }
        lines
    });

    let stdout_lines = stdout_handle.await?;
    let stderr_lines = stderr_handle.await?;

    let status = child.wait().await?;
    let exit_code = status.code().unwrap_or(-1);

    tracing::info!(exit_code = exit_code, "Terraform command completed");

    Ok(TerraformResult {
        exit_code,
        stdout_lines,
        stderr_lines,
    })
}

/// Run terraform init for a module.
pub async fn terraform_init(settings: &YamlSettings, module_dir: &Path) -> Result<TerraformResult> {
    run_terraform(settings, module_dir, &["init", "-no-color"]).await
}

/// Run terraform plan for a module.
pub async fn terraform_plan(settings: &YamlSettings, module_dir: &Path) -> Result<TerraformResult> {
    run_terraform(
        settings,
        module_dir,
        &["plan", "-json", "-out=plan.tfplan", "-no-color"],
    )
    .await
}

/// Run terraform apply for a module.
pub async fn terraform_apply(
    settings: &YamlSettings,
    module_dir: &Path,
) -> Result<TerraformResult> {
    run_terraform(
        settings,
        module_dir,
        &["apply", "-json", "-auto-approve", "plan.tfplan"],
    )
    .await
}

/// Run terraform destroy for a module.
pub async fn terraform_destroy(
    settings: &YamlSettings,
    module_dir: &Path,
) -> Result<TerraformResult> {
    run_terraform(
        settings,
        module_dir,
        &["destroy", "-json", "-auto-approve", "-no-color"],
    )
    .await
}

/// Run terraform output to capture module outputs.
pub async fn terraform_output(
    settings: &YamlSettings,
    module_dir: &Path,
) -> Result<HashMap<String, serde_json::Value>> {
    let result = run_terraform(settings, module_dir, &["output", "-json"]).await?;
    if result.exit_code != 0 {
        anyhow::bail!(
            "terraform output failed with exit code {}: {}",
            result.exit_code,
            result.error_message()
        );
    }
    let output_str = result.stdout_lines.join("\n");
    let outputs: HashMap<String, serde_json::Value> =
        serde_json::from_str(&output_str).unwrap_or_default();
    Ok(outputs)
}
