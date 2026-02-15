use std::path::{Path, PathBuf};

use anyhow::Result;
use tracing::debug;

/// Manages a local cache of downloaded provider binaries.
///
/// Cache layout:
///   .oxid/providers/
///     registry.terraform.io/
///       hashicorp/aws/
///         5.70.0/
///           terraform-provider-aws_v5.70.0_x5
///       hashicorp/google/
///         6.10.0/
///           terraform-provider-google_v6.10.0_x5
pub struct ProviderCache {
    root: PathBuf,
}

impl ProviderCache {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Get the cache directory for a specific provider version.
    pub fn version_dir(&self, namespace: &str, provider_type: &str, version: &str) -> PathBuf {
        self.root
            .join("registry.terraform.io")
            .join(namespace)
            .join(provider_type)
            .join(version)
    }

    /// Find a cached provider binary matching the given constraint.
    /// For now, supports exact version match or "latest cached".
    pub fn find(
        &self,
        namespace: &str,
        provider_type: &str,
        version_constraint: &str,
    ) -> Result<Option<PathBuf>> {
        let provider_dir = self
            .root
            .join("registry.terraform.io")
            .join(namespace)
            .join(provider_type);

        if !provider_dir.exists() {
            return Ok(None);
        }

        // Exact version match
        let version_constraint = version_constraint.trim();
        if !version_constraint.starts_with('~')
            && !version_constraint.starts_with('>')
            && !version_constraint.starts_with('<')
            && !version_constraint.starts_with('=')
        {
            return self.find_exact(namespace, provider_type, version_constraint);
        }

        // For constraint-based lookups, find any cached version.
        // The actual version resolution happens in the registry client;
        // here we just check if we already have any matching version.
        let entries = std::fs::read_dir(&provider_dir)?;
        let mut versions: Vec<(String, PathBuf)> = Vec::new();

        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let version = entry.file_name().to_string_lossy().to_string();
                if let Some(binary) = find_binary_in_dir(&entry.path()) {
                    versions.push((version, binary));
                }
            }
        }

        if versions.is_empty() {
            return Ok(None);
        }

        // Sort by version descending and return the latest
        versions.sort_by(|a, b| compare_versions(&b.0, &a.0));
        Ok(Some(versions[0].1.clone()))
    }

    /// Find a cached provider binary for an exact version.
    pub fn find_exact(
        &self,
        namespace: &str,
        provider_type: &str,
        version: &str,
    ) -> Result<Option<PathBuf>> {
        let version_dir = self.version_dir(namespace, provider_type, version);

        if !version_dir.exists() {
            return Ok(None);
        }

        Ok(find_binary_in_dir(&version_dir))
    }

    /// List all cached providers.
    pub fn list_cached(&self) -> Result<Vec<CachedProvider>> {
        let registry_dir = self.root.join("registry.terraform.io");
        if !registry_dir.exists() {
            return Ok(Vec::new());
        }

        let mut result = Vec::new();

        for ns_entry in std::fs::read_dir(&registry_dir)? {
            let ns_entry = ns_entry?;
            if !ns_entry.file_type()?.is_dir() {
                continue;
            }
            let namespace = ns_entry.file_name().to_string_lossy().to_string();

            for type_entry in std::fs::read_dir(ns_entry.path())? {
                let type_entry = type_entry?;
                if !type_entry.file_type()?.is_dir() {
                    continue;
                }
                let provider_type = type_entry.file_name().to_string_lossy().to_string();

                for ver_entry in std::fs::read_dir(type_entry.path())? {
                    let ver_entry = ver_entry?;
                    if !ver_entry.file_type()?.is_dir() {
                        continue;
                    }
                    let version = ver_entry.file_name().to_string_lossy().to_string();

                    if let Some(binary) = find_binary_in_dir(&ver_entry.path()) {
                        let size = std::fs::metadata(&binary)
                            .map(|m| m.len())
                            .unwrap_or(0);

                        result.push(CachedProvider {
                            namespace: namespace.clone(),
                            provider_type: provider_type.clone(),
                            version: version.clone(),
                            binary_path: binary,
                            size_bytes: size,
                        });
                    }
                }
            }
        }

        Ok(result)
    }

    /// Remove a specific cached provider version.
    pub fn remove(
        &self,
        namespace: &str,
        provider_type: &str,
        version: &str,
    ) -> Result<()> {
        let version_dir = self.version_dir(namespace, provider_type, version);
        if version_dir.exists() {
            std::fs::remove_dir_all(&version_dir)?;
            debug!("Removed cached provider {}/{} v{}", namespace, provider_type, version);
        }
        Ok(())
    }

    /// Clear the entire provider cache.
    pub fn clear(&self) -> Result<()> {
        if self.root.exists() {
            std::fs::remove_dir_all(&self.root)?;
        }
        Ok(())
    }

    /// Total size of the cache in bytes.
    pub fn total_size(&self) -> Result<u64> {
        if !self.root.exists() {
            return Ok(0);
        }
        dir_size(&self.root)
    }
}

/// A cached provider entry.
#[derive(Debug)]
pub struct CachedProvider {
    pub namespace: String,
    pub provider_type: String,
    pub version: String,
    pub binary_path: PathBuf,
    pub size_bytes: u64,
}

impl std::fmt::Display for CachedProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}/{}@{} ({:.1} MB)",
            self.namespace,
            self.provider_type,
            self.version,
            self.size_bytes as f64 / 1_048_576.0
        )
    }
}

/// Find a provider binary in a directory.
fn find_binary_in_dir(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries {
        let entry = entry.ok()?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("terraform-provider-") {
            return Some(entry.path());
        }
    }
    None
}

/// Calculate total size of a directory recursively.
fn dir_size(path: &Path) -> Result<u64> {
    let mut total = 0;
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                total += dir_size(&path)?;
            } else {
                total += std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    Ok(total)
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let a_parts: Vec<u64> = a.split('.').filter_map(|p| p.parse().ok()).collect();
    let b_parts: Vec<u64> = b.split('.').filter_map(|p| p.parse().ok()).collect();
    let max_len = a_parts.len().max(b_parts.len());
    for i in 0..max_len {
        let av = a_parts.get(i).copied().unwrap_or(0);
        let bv = b_parts.get(i).copied().unwrap_or(0);
        match av.cmp(&bv) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}
