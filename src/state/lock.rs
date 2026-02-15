use anyhow::{bail, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// File-based lock for concurrent safety.
pub struct FileLock {
    lock_path: PathBuf,
}

impl FileLock {
    /// Acquire a lock file. Fails if the lock already exists.
    pub fn acquire(working_dir: &str, module_name: &str) -> Result<Self> {
        let lock_path = Path::new(working_dir)
            .join("locks")
            .join(format!("{}.lock", module_name));
        fs::create_dir_all(lock_path.parent().unwrap())?;

        if lock_path.exists() {
            bail!(
                "Module '{}' is locked. Another process may be running.",
                module_name
            );
        }

        let lock_info = format!(
            "pid={}\ntime={}",
            std::process::id(),
            chrono::Utc::now().to_rfc3339()
        );
        fs::write(&lock_path, lock_info)?;

        Ok(Self { lock_path })
    }

    /// Release the lock file.
    pub fn release(self) -> Result<()> {
        if self.lock_path.exists() {
            fs::remove_file(&self.lock_path)?;
        }
        Ok(())
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_path);
    }
}
