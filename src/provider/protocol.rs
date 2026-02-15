use std::path::Path;
use std::process::Stdio;

use anyhow::{bail, Context, Result};
use tokio::io::AsyncBufReadExt;
use tokio::net::{TcpListener, UnixStream};
use tokio::process::{Child, Command};
use tonic::transport::Channel;
use tracing::{debug, info, warn};

use super::tfplugin5::provider_client::ProviderClient as V5Client;
use super::tfplugin6::provider_client::ProviderClient as V6Client;
use super::ProtocolVersion;

/// The go-plugin handshake magic cookie.
const MAGIC_COOKIE_KEY: &str = "TF_PLUGIN_MAGIC_COOKIE";
const MAGIC_COOKIE_VALUE: &str = "d602bf8f470bc67ca7faa0386276bbdd4330efaf76d1a219cb4d6991ca9872b2";

/// A connected provider instance wrapping the gRPC client.
pub struct ProviderConnection {
    pub protocol_version: ProtocolVersion,
    v5_client: Option<V5Client<Channel>>,
    v6_client: Option<V6Client<Channel>>,
    child: Child,
    /// Cached schema type names for resource_types()/data_source_types().
    schemas: Option<SchemaCache>,
    /// Full schema as JSON for external caching.
    schema_json: Option<serde_json::Value>,
}

/// Cached schema info extracted from either v5 or v6 GetSchema responses.
struct SchemaCache {
    resource_schemas: std::collections::HashMap<String, serde_json::Value>,
    data_source_schemas: std::collections::HashMap<String, serde_json::Value>,
    provider_meta_schema: Option<serde_json::Value>,
}

impl ProviderConnection {
    /// Start a provider binary and establish a gRPC connection.
    pub async fn start(binary_path: &Path) -> Result<Self> {
        info!("Starting provider: {}", binary_path.display());

        let mut child = Command::new(binary_path)
            .env(MAGIC_COOKIE_KEY, MAGIC_COOKIE_VALUE)
            .env("PLUGIN_MIN_PORT", "10000")
            .env("PLUGIN_MAX_PORT", "25000")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("Failed to start provider binary")?;

        // Drain stderr in a background task to prevent the provider from blocking
        // when its log output exceeds the OS pipe buffer (typically 64KB on macOS).
        let stderr = child
            .stderr
            .take()
            .context("Failed to capture provider stderr")?;
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let trimmed = line.trim_end();
                        if !trimmed.is_empty() {
                            // Provider stderr is JSON-structured logs (go-hclog format).
                            // Only surface warn/error/fatal at warn level; everything else
                            // goes to debug to avoid flooding the terminal.
                            let is_important = if let Ok(parsed) =
                                serde_json::from_str::<serde_json::Value>(trimmed)
                            {
                                matches!(
                                    parsed.get("@level").and_then(|l| l.as_str()),
                                    Some("warn" | "error" | "fatal")
                                )
                            } else {
                                // Non-JSON output — only show lines that look like actual
                                // errors (panic, fatal, stack traces). Provider startup
                                // messages and debug/trace/info lines are suppressed.
                                let upper = trimmed.to_uppercase();
                                upper.contains("PANIC")
                                    || upper.contains("FATAL")
                                    || upper.starts_with("GOROUTINE ")
                                    || upper.starts_with("[ERROR]")
                                    || upper.starts_with("[WARN]")
                            };
                            if is_important {
                                warn!(target: "provider_stderr", "{}", trimmed);
                            } else {
                                debug!(target: "provider_stderr", "{}", trimmed);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // Read the handshake line from stdout
        let stdout = child
            .stdout
            .take()
            .context("Failed to capture provider stdout")?;

        let mut reader = tokio::io::BufReader::new(stdout);
        let mut handshake_line = String::new();

        let read_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            reader.read_line(&mut handshake_line),
        )
        .await;

        match read_result {
            Ok(Ok(0)) => bail!("Provider exited before handshake"),
            Ok(Err(e)) => bail!("Failed to read provider handshake: {}", e),
            Err(_) => bail!("Provider handshake timed out after 30 seconds"),
            Ok(Ok(_)) => {}
        }

        let handshake = parse_handshake(handshake_line.trim())?;
        debug!("Provider handshake: {:?}", handshake);

        let protocol_version = if handshake.app_protocol == 6 {
            ProtocolVersion::V6
        } else if handshake.app_protocol == 5 {
            ProtocolVersion::V5
        } else {
            bail!(
                "Unsupported provider protocol version: {}",
                handshake.app_protocol
            );
        };

        // Connect via gRPC (supports both TCP and Unix socket)
        // For unix sockets, we spin up a local TCP proxy because tonic's connect_with_connector
        // doesn't properly apply h2 connection-level flow control window sizes, causing large
        // responses (like the AWS provider's ~20MB schema) to hang indefinitely.
        let endpoint_addr = if handshake.network_type == "unix" {
            let socket_path = handshake.address.clone();
            info!("Connecting to provider gRPC via unix socket: {}", socket_path);

            // Bind a TCP listener on an ephemeral port
            let tcp_listener = TcpListener::bind("127.0.0.1:0")
                .await
                .context("Failed to bind TCP proxy listener")?;
            let proxy_addr = tcp_listener
                .local_addr()
                .context("Failed to get proxy address")?;
            info!("TCP proxy for unix socket listening on {}", proxy_addr);

            // Spawn the proxy task in the background
            tokio::spawn(async move {
                loop {
                    match tcp_listener.accept().await {
                        Ok((tcp_stream, _)) => {
                            let path = socket_path.clone();
                            tokio::spawn(async move {
                                match UnixStream::connect(&path).await {
                                    Ok(unix_stream) => {
                                        let (mut tcp_read, mut tcp_write) =
                                            tokio::io::split(tcp_stream);
                                        let (mut unix_read, mut unix_write) =
                                            tokio::io::split(unix_stream);

                                        let t2u = tokio::io::copy(&mut tcp_read, &mut unix_write);
                                        let u2t = tokio::io::copy(&mut unix_read, &mut tcp_write);

                                        let _ = tokio::try_join!(t2u, u2t);
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "Failed to connect to unix socket {}: {}",
                                            path, e
                                        );
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("TCP proxy accept error: {}", e);
                            break;
                        }
                    }
                }
            });

            format!("http://{}", proxy_addr)
        } else {
            let endpoint = format!("http://{}", handshake.address);
            info!("Connecting to provider gRPC at {}", endpoint);
            endpoint
        };

        let channel = Channel::from_shared(endpoint_addr.clone())
            .context("Invalid provider endpoint")?
            .initial_stream_window_size((1 << 31) - 1)
            .initial_connection_window_size((1 << 31) - 1)
            .connect()
            .await
            .context("Failed to connect to provider gRPC")?;

        // AWS provider schema is ~256MB, so we need large message limits
        const MAX_MSG_SIZE: usize = 256 * 1024 * 1024;

        let (v5_client, v6_client) = match protocol_version {
            ProtocolVersion::V5 => {
                info!("Using tfplugin5 protocol");
                let client = V5Client::new(channel)
                    .max_decoding_message_size(MAX_MSG_SIZE)
                    .max_encoding_message_size(MAX_MSG_SIZE);
                (Some(client), None)
            }
            ProtocolVersion::V6 => {
                info!("Using tfplugin6 protocol");
                let client = V6Client::new(channel)
                    .max_decoding_message_size(MAX_MSG_SIZE)
                    .max_encoding_message_size(MAX_MSG_SIZE);
                (None, Some(client))
            }
        };

        Ok(Self {
            protocol_version,
            v5_client,
            v6_client,
            child,
            schemas: None,
            schema_json: None,
        })
    }

    /// Fetch the provider schema. Returns a lightweight JSON with provider config schema
    /// and resource/data source type names.
    pub async fn get_schema(&mut self) -> Result<serde_json::Value> {
        if let Some(ref cached) = self.schema_json {
            return Ok(cached.clone());
        }

        info!("Fetching provider schema (this may take a moment for large providers)...");

        let timeout_dur = std::time::Duration::from_secs(300);

        let schema_json = match self.protocol_version {
            ProtocolVersion::V5 => {
                let client = self.v5_client.as_mut().context("No v5 client")?;
                let response = tokio::time::timeout(
                    timeout_dur,
                    client.get_schema(super::tfplugin5::get_provider_schema::Request {}),
                )
                .await
                .map_err(|_| anyhow::anyhow!("GetSchema RPC timed out after 300s"))?
                .context("GetSchema RPC failed")?;
                let inner = response.into_inner();
                check_diagnostics_v5(&inner.diagnostics)?;
                info!(
                    "Schema loaded: {} resource types, {} data source types",
                    inner.resource_schemas.len(),
                    inner.data_source_schemas.len()
                );
                let resource_schemas: std::collections::HashMap<String, serde_json::Value> =
                    inner.resource_schemas.iter()
                        .map(|(k, v)| (k.clone(), schema_to_json_v5(v)))
                        .collect();
                let data_source_schemas: std::collections::HashMap<String, serde_json::Value> =
                    inner.data_source_schemas.iter()
                        .map(|(k, v)| (k.clone(), schema_to_json_v5(v)))
                        .collect();
                let resource_types: Vec<&String> = resource_schemas.keys().collect();
                let data_source_types: Vec<&String> = data_source_schemas.keys().collect();
                let schema_json = serde_json::json!({
                    "provider": inner.provider.as_ref().map(schema_to_json_v5),
                    "resource_types": resource_types,
                    "data_source_types": data_source_types,
                });
                let provider_meta_schema = inner.provider_meta.as_ref().map(schema_to_json_v5);
                if provider_meta_schema.is_some() {
                    info!("Provider has provider_meta schema");
                }
                self.schemas = Some(SchemaCache {
                    resource_schemas,
                    data_source_schemas,
                    provider_meta_schema,
                });
                schema_json
            }
            ProtocolVersion::V6 => {
                let client = self.v6_client.as_mut().context("No v6 client")?;
                let response = tokio::time::timeout(
                    timeout_dur,
                    client.get_provider_schema(super::tfplugin6::get_provider_schema::Request {}),
                )
                .await
                .map_err(|_| anyhow::anyhow!("GetProviderSchema RPC timed out after 300s"))?
                .context("GetProviderSchema RPC failed")?;
                let inner = response.into_inner();
                check_diagnostics_v6(&inner.diagnostics)?;
                info!(
                    "Schema loaded: {} resource types, {} data source types",
                    inner.resource_schemas.len(),
                    inner.data_source_schemas.len()
                );
                let resource_schemas: std::collections::HashMap<String, serde_json::Value> =
                    inner.resource_schemas.iter()
                        .map(|(k, v)| (k.clone(), schema_to_json_v6(v)))
                        .collect();
                let data_source_schemas: std::collections::HashMap<String, serde_json::Value> =
                    inner.data_source_schemas.iter()
                        .map(|(k, v)| (k.clone(), schema_to_json_v6(v)))
                        .collect();
                let resource_types: Vec<&String> = resource_schemas.keys().collect();
                let data_source_types: Vec<&String> = data_source_schemas.keys().collect();
                let schema_json = serde_json::json!({
                    "provider": inner.provider.as_ref().map(schema_to_json_v6),
                    "resource_types": resource_types,
                    "data_source_types": data_source_types,
                });
                let provider_meta_schema = inner.provider_meta.as_ref().map(schema_to_json_v6);
                self.schemas = Some(SchemaCache {
                    resource_schemas,
                    data_source_schemas,
                    provider_meta_schema,
                });
                schema_json
            }
        };

        self.schema_json = Some(schema_json.clone());
        Ok(schema_json)
    }

    /// Configure the provider.
    pub async fn configure(
        &mut self,
        terraform_version: &str,
        config: &serde_json::Value,
    ) -> Result<()> {
        info!("Sending Configure RPC...");
        let timeout_dur = std::time::Duration::from_secs(30);

        match self.protocol_version {
            ProtocolVersion::V5 => {
                let client = self.v5_client.as_mut().context("No v5 client")?;
                let config_msgpack = rmp_serde::to_vec_named(config)
                    .context("Failed to encode config as msgpack")?;
                let request = super::tfplugin5::configure::Request {
                    terraform_version: terraform_version.to_string(),
                    config: Some(super::tfplugin5::DynamicValue {
                        msgpack: config_msgpack,
                        json: vec![],
                    }),
                    client_capabilities: None,
                };
                let response = tokio::time::timeout(timeout_dur, client.configure(request))
                    .await
                    .map_err(|_| anyhow::anyhow!("Configure RPC timed out after 30s"))?
                    .context("Configure RPC failed")?;
                check_diagnostics_v5(&response.into_inner().diagnostics)?;
            }
            ProtocolVersion::V6 => {
                let client = self.v6_client.as_mut().context("No v6 client")?;
                let config_msgpack = rmp_serde::to_vec_named(config)
                    .context("Failed to encode config as msgpack")?;
                let request = super::tfplugin6::configure_provider::Request {
                    terraform_version: terraform_version.to_string(),
                    config: Some(super::tfplugin6::DynamicValue {
                        msgpack: config_msgpack,
                        json: vec![],
                    }),
                    client_capabilities: None,
                };
                let response = tokio::time::timeout(
                    timeout_dur,
                    client.configure_provider(request),
                )
                .await
                .map_err(|_| anyhow::anyhow!("ConfigureProvider RPC timed out after 30s"))?
                .context("ConfigureProvider RPC failed")?;
                check_diagnostics_v6(&response.into_inner().diagnostics)?;
            }
        }
        info!("Provider configured successfully");
        Ok(())
    }

    /// Plan a resource change.
    pub async fn plan_resource_change(
        &self,
        type_name: &str,
        prior_state: Option<&serde_json::Value>,
        proposed_new_state: Option<&serde_json::Value>,
        config: &serde_json::Value,
    ) -> Result<PlanResult> {
        debug!("PlanResourceChange for {}: config keys = {:?}",
            type_name,
            config.as_object().map(|m| m.keys().collect::<Vec<_>>()));

        let timeout_dur = std::time::Duration::from_secs(30);

        // Build provider_meta from schema (populate all attrs as null)
        let provider_meta_val = self.build_provider_meta();

        match self.protocol_version {
            ProtocolVersion::V5 => {
                // Clone is cheap — shares the underlying HTTP/2 channel
                let mut client = self.v5_client.as_ref().context("No v5 client")?.clone();
                let null_val = serde_json::Value::Null;
                let request = super::tfplugin5::plan_resource_change::Request {
                    type_name: type_name.to_string(),
                    prior_state: Some(json_to_dynamic_v5(prior_state.unwrap_or(&null_val))),
                    proposed_new_state: Some(json_to_dynamic_v5(proposed_new_state.unwrap_or(&null_val))),
                    config: Some(json_to_dynamic_v5(config)),
                    prior_private: vec![],
                    provider_meta: Some(json_to_dynamic_v5(&provider_meta_val)),
                    client_capabilities: None,
                    prior_identity: None,
                };
                let response = tokio::time::timeout(timeout_dur, client
                    .plan_resource_change(request))
                    .await
                    .map_err(|_| anyhow::anyhow!("PlanResourceChange RPC timed out after 30s for {}", type_name))?
                    .map_err(|e| anyhow::anyhow!("PlanResourceChange RPC failed for {}: {}", type_name, e))?;
                let inner = response.into_inner();
                // Log the error details for debugging
                for d in &inner.diagnostics {
                    if d.severity == super::tfplugin5::diagnostic::Severity::Error as i32 {
                        if let Some(ref attr) = d.attribute {
                            info!("Diagnostic error at {:?}: {} - {}",
                                attribute_path_to_string_v5(attr), d.summary, d.detail);
                        } else {
                            info!("Diagnostic error: {} - {}", d.summary, d.detail);
                        }
                    }
                }
                check_diagnostics_v5(&inner.diagnostics)?;
                let planned_state = inner
                    .planned_state
                    .map(|dv| dynamic_to_json_v5(&dv))
                    .transpose()?;
                let requires_replace: Vec<String> = inner
                    .requires_replace
                    .iter()
                    .map(attribute_path_to_string_v5)
                    .collect();
                Ok(PlanResult {
                    planned_state,
                    requires_replace,
                    planned_private: inner.planned_private,
                })
            }
            ProtocolVersion::V6 => {
                let mut client = self.v6_client.as_ref().context("No v6 client")?.clone();
                let null_val = serde_json::Value::Null;
                let request = super::tfplugin6::plan_resource_change::Request {
                    type_name: type_name.to_string(),
                    prior_state: Some(json_to_dynamic_v6(prior_state.unwrap_or(&null_val))),
                    proposed_new_state: Some(json_to_dynamic_v6(proposed_new_state.unwrap_or(&null_val))),
                    config: Some(json_to_dynamic_v6(config)),
                    prior_private: vec![],
                    provider_meta: None,
                    client_capabilities: None,
                    prior_identity: None,
                };
                let response = tokio::time::timeout(timeout_dur, client
                    .plan_resource_change(request))
                    .await
                    .map_err(|_| anyhow::anyhow!("PlanResourceChange RPC timed out after 30s for {}", type_name))?
                    .map_err(|e| anyhow::anyhow!("PlanResourceChange RPC failed for {}: {}", type_name, e))?;
                let inner = response.into_inner();
                check_diagnostics_v6(&inner.diagnostics)?;
                let planned_state = inner
                    .planned_state
                    .map(|dv| dynamic_to_json_v6(&dv))
                    .transpose()?;
                let requires_replace: Vec<String> = inner
                    .requires_replace
                    .iter()
                    .map(attribute_path_to_string_v6)
                    .collect();
                Ok(PlanResult {
                    planned_state,
                    requires_replace,
                    planned_private: inner.planned_private,
                })
            }
        }
    }

    /// Apply a resource change.
    pub async fn apply_resource_change(
        &self,
        type_name: &str,
        prior_state: Option<&serde_json::Value>,
        planned_state: Option<&serde_json::Value>,
        config: &serde_json::Value,
        planned_private: &[u8],
    ) -> Result<ApplyResult> {
        // Apply can take a long time — EC2 instances need ~60s to terminate, IGW detach
        // can take ~50s, and the provider retries operations like VPC deletion internally.
        // Use 600s (10 min) to match Terraform's default resource operation timeout.
        let timeout_dur = std::time::Duration::from_secs(600);

        // Build provider_meta from schema (required by framework-based resources)
        let provider_meta_val = self.build_provider_meta();

        let null_val = serde_json::Value::Null;

        match self.protocol_version {
            ProtocolVersion::V5 => {
                let mut client = self.v5_client.as_ref().context("No v5 client")?.clone();
                let request = super::tfplugin5::apply_resource_change::Request {
                    type_name: type_name.to_string(),
                    prior_state: Some(json_to_dynamic_v5(prior_state.unwrap_or(&null_val))),
                    planned_state: Some(json_to_dynamic_v5(planned_state.unwrap_or(&null_val))),
                    config: Some(json_to_dynamic_v5(config)),
                    planned_private: planned_private.to_vec(),
                    provider_meta: Some(json_to_dynamic_v5(&provider_meta_val)),
                    planned_identity: None,
                };
                let response = tokio::time::timeout(timeout_dur, client
                    .apply_resource_change(request))
                    .await
                    .map_err(|_| anyhow::anyhow!("ApplyResourceChange RPC timed out after 600s for {}", type_name))?
                    .map_err(|e| anyhow::anyhow!("ApplyResourceChange RPC failed for {}: {}", type_name, e))?;
                let inner = response.into_inner();
                for d in &inner.diagnostics {
                    if d.severity == super::tfplugin5::diagnostic::Severity::Error as i32 {
                        if let Some(ref attr) = d.attribute {
                            info!("Apply diagnostic error at {:?}: {} - {}",
                                attribute_path_to_string_v5(attr), d.summary, d.detail);
                        } else {
                            info!("Apply diagnostic error: {} - {}", d.summary, d.detail);
                        }
                    }
                }
                check_diagnostics_v5(&inner.diagnostics)?;
                let new_state = inner
                    .new_state
                    .map(|dv| dynamic_to_json_v5(&dv))
                    .transpose()?;
                Ok(ApplyResult {
                    new_state,
                    private_data: inner.private,
                })
            }
            ProtocolVersion::V6 => {
                let mut client = self.v6_client.as_ref().context("No v6 client")?.clone();
                let request = super::tfplugin6::apply_resource_change::Request {
                    type_name: type_name.to_string(),
                    prior_state: Some(json_to_dynamic_v6(prior_state.unwrap_or(&null_val))),
                    planned_state: Some(json_to_dynamic_v6(planned_state.unwrap_or(&null_val))),
                    config: Some(json_to_dynamic_v6(config)),
                    planned_private: planned_private.to_vec(),
                    provider_meta: Some(json_to_dynamic_v6(&provider_meta_val)),
                    planned_identity: None,
                };
                let response = tokio::time::timeout(timeout_dur, client
                    .apply_resource_change(request))
                    .await
                    .map_err(|_| anyhow::anyhow!("ApplyResourceChange RPC timed out after 600s for {}", type_name))?
                    .map_err(|e| anyhow::anyhow!("ApplyResourceChange RPC failed for {}: {}", type_name, e))?;
                let inner = response.into_inner();
                check_diagnostics_v6(&inner.diagnostics)?;
                let new_state = inner
                    .new_state
                    .map(|dv| dynamic_to_json_v6(&dv))
                    .transpose()?;
                Ok(ApplyResult {
                    new_state,
                    private_data: inner.private,
                })
            }
        }
    }

    /// Read the current state of a resource from the provider.
    pub async fn read_resource(
        &self,
        type_name: &str,
        current_state: &serde_json::Value,
    ) -> Result<Option<serde_json::Value>> {
        let timeout_dur = std::time::Duration::from_secs(30);
        let provider_meta_val = self.build_provider_meta();

        match self.protocol_version {
            ProtocolVersion::V5 => {
                let mut client = self.v5_client.as_ref().context("No v5 client")?.clone();
                let request = super::tfplugin5::read_resource::Request {
                    type_name: type_name.to_string(),
                    current_state: Some(json_to_dynamic_v5(current_state)),
                    private: vec![],
                    provider_meta: Some(json_to_dynamic_v5(&provider_meta_val)),
                    client_capabilities: None,
                    current_identity: None,
                };
                let response = tokio::time::timeout(timeout_dur, client
                    .read_resource(request))
                    .await
                    .map_err(|_| anyhow::anyhow!("ReadResource RPC timed out after 30s for {}", type_name))?
                    .map_err(|e| anyhow::anyhow!("ReadResource RPC failed for {}: {}", type_name, e))?;
                let inner = response.into_inner();
                check_diagnostics_v5(&inner.diagnostics)?;
                inner.new_state.map(|dv| dynamic_to_json_v5(&dv)).transpose()
            }
            ProtocolVersion::V6 => {
                let mut client = self.v6_client.as_ref().context("No v6 client")?.clone();
                let request = super::tfplugin6::read_resource::Request {
                    type_name: type_name.to_string(),
                    current_state: Some(json_to_dynamic_v6(current_state)),
                    private: vec![],
                    provider_meta: Some(json_to_dynamic_v6(&provider_meta_val)),
                    client_capabilities: None,
                    current_identity: None,
                };
                let response = tokio::time::timeout(timeout_dur, client
                    .read_resource(request))
                    .await
                    .map_err(|_| anyhow::anyhow!("ReadResource RPC timed out after 30s for {}", type_name))?
                    .map_err(|e| anyhow::anyhow!("ReadResource RPC failed for {}: {}", type_name, e))?;
                let inner = response.into_inner();
                check_diagnostics_v6(&inner.diagnostics)?;
                inner.new_state.map(|dv| dynamic_to_json_v6(&dv)).transpose()
            }
        }
    }

    /// Read a data source.
    pub async fn read_data_source(
        &self,
        type_name: &str,
        config: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let provider_meta_val = self.build_provider_meta();
        match self.protocol_version {
            ProtocolVersion::V5 => {
                let mut client = self.v5_client.as_ref().context("No v5 client")?.clone();
                let request = super::tfplugin5::read_data_source::Request {
                    type_name: type_name.to_string(),
                    config: Some(json_to_dynamic_v5(config)),
                    provider_meta: Some(json_to_dynamic_v5(&provider_meta_val)),
                    client_capabilities: None,
                };
                let response = client
                    .read_data_source(request)
                    .await
                    .map_err(|e| anyhow::anyhow!("ReadDataSource RPC failed for {}: {}", type_name, e))?;
                let inner = response.into_inner();
                check_diagnostics_v5(&inner.diagnostics)?;
                inner
                    .state
                    .map(|dv| dynamic_to_json_v5(&dv))
                    .transpose()?
                    .ok_or_else(|| anyhow::anyhow!("Data source returned no state"))
            }
            ProtocolVersion::V6 => {
                let mut client = self.v6_client.as_ref().context("No v6 client")?.clone();
                let request = super::tfplugin6::read_data_source::Request {
                    type_name: type_name.to_string(),
                    config: Some(json_to_dynamic_v6(config)),
                    provider_meta: None,
                    client_capabilities: None,
                };
                let response = client
                    .read_data_source(request)
                    .await
                    .map_err(|e| anyhow::anyhow!("ReadDataSource RPC failed for {}: {}", type_name, e))?;
                let inner = response.into_inner();
                check_diagnostics_v6(&inner.diagnostics)?;
                inner
                    .state
                    .map(|dv| dynamic_to_json_v6(&dv))
                    .transpose()?
                    .ok_or_else(|| anyhow::anyhow!("Data source returned no state"))
            }
        }
    }

    /// Import a resource by its ID.
    pub async fn import_resource(
        &self,
        type_name: &str,
        id: &str,
    ) -> Result<Vec<ImportedResource>> {
        match self.protocol_version {
            ProtocolVersion::V5 => {
                let mut client = self.v5_client.as_ref().context("No v5 client")?.clone();
                let request = super::tfplugin5::import_resource_state::Request {
                    type_name: type_name.to_string(),
                    id: id.to_string(),
                    client_capabilities: None,
                    identity: None,
                };
                let response = client
                    .import_resource_state(request)
                    .await
                    .map_err(|e| anyhow::anyhow!("ImportResourceState RPC failed for {}: {}", type_name, e))?;
                let inner = response.into_inner();
                check_diagnostics_v5(&inner.diagnostics)?;
                let mut results = Vec::new();
                for imported in inner.imported_resources {
                    let state = imported.state.map(|dv| dynamic_to_json_v5(&dv)).transpose()?;
                    results.push(ImportedResource {
                        type_name: imported.type_name,
                        state: state.unwrap_or(serde_json::Value::Null),
                        private_data: imported.private,
                    });
                }
                Ok(results)
            }
            ProtocolVersion::V6 => {
                let mut client = self.v6_client.as_ref().context("No v6 client")?.clone();
                let request = super::tfplugin6::import_resource_state::Request {
                    type_name: type_name.to_string(),
                    id: id.to_string(),
                    client_capabilities: None,
                    identity: None,
                };
                let response = client
                    .import_resource_state(request)
                    .await
                    .map_err(|e| anyhow::anyhow!("ImportResourceState RPC failed for {}: {}", type_name, e))?;
                let inner = response.into_inner();
                check_diagnostics_v6(&inner.diagnostics)?;
                let mut results = Vec::new();
                for imported in inner.imported_resources {
                    let state = imported.state.map(|dv| dynamic_to_json_v6(&dv)).transpose()?;
                    results.push(ImportedResource {
                        type_name: imported.type_name,
                        state: state.unwrap_or(serde_json::Value::Null),
                        private_data: imported.private,
                    });
                }
                Ok(results)
            }
        }
    }

    /// Validate a resource configuration.
    pub async fn validate_resource_config(
        &self,
        type_name: &str,
        config: &serde_json::Value,
    ) -> Result<()> {
        match self.protocol_version {
            ProtocolVersion::V5 => {
                let mut client = self.v5_client.as_ref().context("No v5 client")?.clone();
                let request = super::tfplugin5::validate_resource_type_config::Request {
                    type_name: type_name.to_string(),
                    config: Some(json_to_dynamic_v5(config)),
                    client_capabilities: None,
                };
                let response = client
                    .validate_resource_type_config(request)
                    .await
                    .map_err(|e| anyhow::anyhow!("ValidateResourceTypeConfig RPC failed for {}: {}", type_name, e))?;
                check_diagnostics_v5(&response.into_inner().diagnostics)?;
            }
            ProtocolVersion::V6 => {
                let mut client = self.v6_client.as_ref().context("No v6 client")?.clone();
                let request = super::tfplugin6::validate_resource_config::Request {
                    type_name: type_name.to_string(),
                    config: Some(json_to_dynamic_v6(config)),
                    client_capabilities: None,
                };
                let response = client
                    .validate_resource_config(request)
                    .await
                    .map_err(|e| anyhow::anyhow!("ValidateResourceConfig RPC failed for {}: {}", type_name, e))?;
                check_diagnostics_v6(&response.into_inner().diagnostics)?;
            }
        }
        Ok(())
    }

    /// Gracefully stop the provider.
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(ref mut client) = self.v5_client {
            let _ = client
                .stop(super::tfplugin5::stop::Request {})
                .await;
        }
        if let Some(ref mut client) = self.v6_client {
            let _ = client
                .stop_provider(super::tfplugin6::stop_provider::Request {})
                .await;
        }
        let _ = self.child.kill().await;
        Ok(())
    }

    /// Get the schema for a specific resource type.
    pub fn get_resource_schema(&self, type_name: &str) -> Option<serde_json::Value> {
        self.schemas
            .as_ref()
            .and_then(|s| s.resource_schemas.get(type_name).cloned())
    }

    /// Get the schema for a specific data source type.
    pub fn get_data_source_schema(&self, type_name: &str) -> Option<serde_json::Value> {
        self.schemas
            .as_ref()
            .and_then(|s| s.data_source_schemas.get(type_name).cloned())
    }

    /// Build the provider_meta DynamicValue from the stored schema.
    /// Populates all attributes with null values.
    fn build_provider_meta(&self) -> serde_json::Value {
        if let Some(schema) = self.schemas.as_ref().and_then(|s| s.provider_meta_schema.as_ref()) {
            if let Some(block) = schema.get("block") {
                let mut meta = serde_json::Map::new();
                if let Some(attrs) = block.get("attributes").and_then(|a| a.as_array()) {
                    for attr in attrs {
                        if let Some(name) = attr.get("name").and_then(|n| n.as_str()) {
                            meta.insert(name.to_string(), serde_json::Value::Null);
                        }
                    }
                }
                if !meta.is_empty() {
                    return serde_json::Value::Object(meta);
                }
            }
        }
        serde_json::json!({})
    }

    /// Get the provider_meta schema (if the provider defines one).
    pub fn get_provider_meta_schema(&self) -> Option<serde_json::Value> {
        self.schemas
            .as_ref()
            .and_then(|s| s.provider_meta_schema.clone())
    }

    /// Get the resource types supported by this provider.
    pub fn resource_types(&self) -> Vec<String> {
        self.schemas
            .as_ref()
            .map(|s| s.resource_schemas.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Get the data source types supported by this provider.
    pub fn data_source_types(&self) -> Vec<String> {
        self.schemas
            .as_ref()
            .map(|s| s.data_source_schemas.keys().cloned().collect())
            .unwrap_or_default()
    }
}

// ─── Result Types ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct PlanResult {
    pub planned_state: Option<serde_json::Value>,
    pub requires_replace: Vec<String>,
    pub planned_private: Vec<u8>,
}

#[derive(Debug)]
pub struct ApplyResult {
    pub new_state: Option<serde_json::Value>,
    pub private_data: Vec<u8>,
}

#[derive(Debug)]
pub struct ImportedResource {
    pub type_name: String,
    pub state: serde_json::Value,
    pub private_data: Vec<u8>,
}

// ─── Handshake ───────────────────────────────────────────────────────────────

#[derive(Debug)]
struct Handshake {
    core_protocol: u32,
    app_protocol: u32,
    network_type: String,
    address: String,
    protocol: String,
}

fn parse_handshake(line: &str) -> Result<Handshake> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() < 5 {
        bail!(
            "Invalid provider handshake (expected 5 pipe-separated fields): '{}'",
            line
        );
    }

    Ok(Handshake {
        core_protocol: parts[0].parse().context("Invalid core protocol version")?,
        app_protocol: parts[1].parse().context("Invalid app protocol version")?,
        network_type: parts[2].to_string(),
        address: parts[3].to_string(),
        protocol: parts[4].to_string(),
    })
}

// ─── Msgpack/cty Helpers ─────────────────────────────────────────────────────

/// Convert rmpv::Value to serde_json::Value, handling cty extension types.
/// cty uses msgpack extension type 0 for "unknown" values (computed at apply time).
fn rmpv_to_json(val: rmpv::Value) -> serde_json::Value {
    match val {
        rmpv::Value::Nil => serde_json::Value::Null,
        rmpv::Value::Boolean(b) => serde_json::Value::Bool(b),
        rmpv::Value::Integer(i) => {
            if let Some(n) = i.as_i64() {
                serde_json::Value::Number(n.into())
            } else if let Some(n) = i.as_u64() {
                serde_json::Value::Number(n.into())
            } else {
                serde_json::Value::Null
            }
        }
        rmpv::Value::F32(f) => serde_json::Number::from_f64(f as f64)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        rmpv::Value::F64(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        rmpv::Value::String(s) => {
            serde_json::Value::String(s.into_str().unwrap_or_default().to_string())
        }
        rmpv::Value::Binary(b) => {
            serde_json::Value::String(base64_encode(&b))
        }
        rmpv::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(rmpv_to_json).collect())
        }
        rmpv::Value::Map(entries) => {
            let mut map = serde_json::Map::new();
            for (k, v) in entries {
                let key = match k {
                    rmpv::Value::String(s) => s.into_str().unwrap_or_default().to_string(),
                    other => format!("{}", other),
                };
                map.insert(key, rmpv_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        rmpv::Value::Ext(type_id, _data) => {
            // cty extension type 0 = unknown value (will be computed at apply time)
            if type_id == 0 {
                serde_json::Value::Null // Treat unknown as null for planning purposes
            } else {
                serde_json::Value::Null
            }
        }
    }
}

/// Simple base64 encoding for binary msgpack values.
fn base64_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(data.len() * 4 / 3 + 4);
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        let _ = write!(s, "{}", CHARS[(n >> 18 & 63) as usize] as char);
        let _ = write!(s, "{}", CHARS[(n >> 12 & 63) as usize] as char);
        if chunk.len() > 1 {
            let _ = write!(s, "{}", CHARS[(n >> 6 & 63) as usize] as char);
        } else {
            s.push('=');
        }
        if chunk.len() > 2 {
            let _ = write!(s, "{}", CHARS[(n & 63) as usize] as char);
        } else {
            s.push('=');
        }
    }
    s
}

// ─── v5 Helpers ──────────────────────────────────────────────────────────────

fn json_to_dynamic_v5(value: &serde_json::Value) -> super::tfplugin5::DynamicValue {
    super::tfplugin5::DynamicValue {
        msgpack: rmp_serde::to_vec_named(value).unwrap_or_default(),
        json: vec![],
    }
}

fn dynamic_to_json_v5(dv: &super::tfplugin5::DynamicValue) -> Result<serde_json::Value> {
    if !dv.msgpack.is_empty() {
        // Use rmpv to handle cty extension types (e.g., unknown values = ext type 0)
        let raw: rmpv::Value = rmpv::decode::read_value(&mut &dv.msgpack[..])
            .context("Failed to decode msgpack")?;
        Ok(rmpv_to_json(raw))
    } else if !dv.json.is_empty() {
        Ok(serde_json::from_slice(&dv.json)?)
    } else {
        Ok(serde_json::Value::Null)
    }
}

fn attribute_path_to_string_v5(path: &super::tfplugin5::AttributePath) -> String {
    path.steps
        .iter()
        .map(|step| {
            use super::tfplugin5::attribute_path::step::Selector;
            match &step.selector {
                Some(Selector::AttributeName(name)) => name.clone(),
                Some(Selector::ElementKeyString(key)) => format!("[{}]", key),
                Some(Selector::ElementKeyInt(idx)) => format!("[{}]", idx),
                None => "?".to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn check_diagnostics_v5(diagnostics: &[super::tfplugin5::Diagnostic]) -> Result<()> {
    let errors: Vec<String> = diagnostics
        .iter()
        .filter(|d| d.severity == super::tfplugin5::diagnostic::Severity::Error as i32)
        .map(|d| {
            if d.detail.is_empty() {
                d.summary.clone()
            } else {
                format!("{}: {}", d.summary, d.detail)
            }
        })
        .collect();

    if errors.is_empty() {
        for d in diagnostics {
            if d.severity == super::tfplugin5::diagnostic::Severity::Warning as i32 {
                warn!("Provider warning: {}", d.summary);
            }
        }
        Ok(())
    } else {
        bail!("Provider errors:\n{}", errors.join("\n"))
    }
}

// ─── v6 Helpers ──────────────────────────────────────────────────────────────

fn json_to_dynamic_v6(value: &serde_json::Value) -> super::tfplugin6::DynamicValue {
    super::tfplugin6::DynamicValue {
        msgpack: rmp_serde::to_vec_named(value).unwrap_or_default(),
        json: vec![],
    }
}

fn dynamic_to_json_v6(dv: &super::tfplugin6::DynamicValue) -> Result<serde_json::Value> {
    if !dv.msgpack.is_empty() {
        let raw: rmpv::Value = rmpv::decode::read_value(&mut &dv.msgpack[..])
            .context("Failed to decode msgpack")?;
        Ok(rmpv_to_json(raw))
    } else if !dv.json.is_empty() {
        Ok(serde_json::from_slice(&dv.json)?)
    } else {
        Ok(serde_json::Value::Null)
    }
}

fn attribute_path_to_string_v6(path: &super::tfplugin6::AttributePath) -> String {
    path.steps
        .iter()
        .map(|step| {
            use super::tfplugin6::attribute_path::step::Selector;
            match &step.selector {
                Some(Selector::AttributeName(name)) => name.clone(),
                Some(Selector::ElementKeyString(key)) => format!("[{}]", key),
                Some(Selector::ElementKeyInt(idx)) => format!("[{}]", idx),
                None => "?".to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn check_diagnostics_v6(diagnostics: &[super::tfplugin6::Diagnostic]) -> Result<()> {
    let errors: Vec<String> = diagnostics
        .iter()
        .filter(|d| d.severity == super::tfplugin6::diagnostic::Severity::Error as i32)
        .map(|d| {
            if d.detail.is_empty() {
                d.summary.clone()
            } else {
                format!("{}: {}", d.summary, d.detail)
            }
        })
        .collect();

    if errors.is_empty() {
        for d in diagnostics {
            if d.severity == super::tfplugin6::diagnostic::Severity::Warning as i32 {
                warn!("Provider warning: {}", d.summary);
            }
        }
        Ok(())
    } else {
        bail!("Provider errors:\n{}", errors.join("\n"))
    }
}

// ─── Schema-to-JSON Conversions ──────────────────────────────────────────────

fn schema_to_json_v5(schema: &super::tfplugin5::Schema) -> serde_json::Value {
    serde_json::json!({
        "version": schema.version,
        "block": schema.block.as_ref().map(block_to_json_v5),
    })
}

fn block_to_json_v5(block: &super::tfplugin5::schema::Block) -> serde_json::Value {
    serde_json::json!({
        "version": block.version,
        "attributes": block.attributes.iter().map(|a| {
            // Parse cty type from bytes (JSON-encoded)
            let cty_type = if !a.r#type.is_empty() {
                serde_json::from_slice::<serde_json::Value>(&a.r#type).ok()
            } else {
                None
            };
            serde_json::json!({
                "name": a.name,
                "type": cty_type,
                "required": a.required,
                "optional": a.optional,
                "computed": a.computed,
                "sensitive": a.sensitive,
                "description": a.description,
            })
        }).collect::<Vec<_>>(),
        "block_types": block.block_types.iter().map(|bt| {
            serde_json::json!({
                "type_name": bt.type_name,
                "nesting": bt.nesting,
                "min_items": bt.min_items,
                "max_items": bt.max_items,
                "block": bt.block.as_ref().map(block_to_json_v5),
            })
        }).collect::<Vec<_>>(),
    })
}

fn schema_to_json_v6(schema: &super::tfplugin6::Schema) -> serde_json::Value {
    serde_json::json!({
        "version": schema.version,
        "block": schema.block.as_ref().map(block_to_json_v6),
    })
}

fn block_to_json_v6(block: &super::tfplugin6::schema::Block) -> serde_json::Value {
    serde_json::json!({
        "version": block.version,
        "attributes": block.attributes.iter().map(|a| {
            let cty_type = if !a.r#type.is_empty() {
                serde_json::from_slice::<serde_json::Value>(&a.r#type).ok()
            } else {
                None
            };
            serde_json::json!({
                "name": a.name,
                "type": cty_type,
                "required": a.required,
                "optional": a.optional,
                "computed": a.computed,
                "sensitive": a.sensitive,
                "description": a.description,
            })
        }).collect::<Vec<_>>(),
        "block_types": block.block_types.iter().map(|bt| {
            serde_json::json!({
                "type_name": bt.type_name,
                "nesting": bt.nesting,
                "min_items": bt.min_items,
                "max_items": bt.max_items,
                "block": bt.block.as_ref().map(block_to_json_v6),
            })
        }).collect::<Vec<_>>(),
    })
}
