use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

use super::models::{ModuleOutput, ModuleState};

/// SQLite-backed state store for tracking module execution state.
pub struct StateStore {
    conn: Mutex<Connection>,
}

impl StateStore {
    /// Open or create the state database.
    pub fn open(working_dir: &str) -> Result<Self> {
        let db_path = Path::new(working_dir).join("state.db");
        std::fs::create_dir_all(working_dir)?;
        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open state database at {}", db_path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Create the database tables if they don't exist.
    pub fn initialize(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS modules (
                name TEXT PRIMARY KEY,
                source TEXT NOT NULL DEFAULT '',
                version TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                last_plan_at TEXT,
                last_apply_at TEXT
            );

            CREATE TABLE IF NOT EXISTS outputs (
                module_name TEXT NOT NULL,
                output_key TEXT NOT NULL,
                output_value TEXT NOT NULL,
                PRIMARY KEY (module_name, output_key)
            );

            CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                started_at TEXT NOT NULL,
                completed_at TEXT,
                status TEXT NOT NULL DEFAULT 'running',
                modules_planned INTEGER DEFAULT 0,
                modules_applied INTEGER DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS locks (
                module_name TEXT PRIMARY KEY,
                locked_at TEXT NOT NULL,
                locked_by TEXT NOT NULL
            );
            ",
        )?;
        Ok(())
    }

    /// Update the status of a module.
    pub fn update_module_status(&self, name: &str, status: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO modules (name, status) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET status = ?2",
            rusqlite::params![name, status],
        )?;

        if status == "succeeded" {
            conn.execute(
                "UPDATE modules SET last_apply_at = datetime('now') WHERE name = ?1",
                rusqlite::params![name],
            )?;
        }

        Ok(())
    }

    /// Get the current status of a module.
    pub fn get_module_status(&self, name: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT status FROM modules WHERE name = ?1")?;
        let result = stmt
            .query_row(rusqlite::params![name], |row| row.get(0))
            .ok();
        Ok(result)
    }

    /// List all modules and their states.
    pub fn list_modules(&self) -> Result<Vec<ModuleState>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT name, source, version, status, last_plan_at, last_apply_at FROM modules",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ModuleState {
                    name: row.get(0)?,
                    source: row.get(1)?,
                    version: row.get(2)?,
                    status: row.get(3)?,
                    last_plan_at: row.get(4)?,
                    last_apply_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Set an output value for a module.
    pub fn set_output(&self, module_name: &str, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO outputs (module_name, output_key, output_value) VALUES (?1, ?2, ?3)
             ON CONFLICT(module_name, output_key) DO UPDATE SET output_value = ?3",
            rusqlite::params![module_name, key, value],
        )?;
        Ok(())
    }

    /// Get an output value for a module.
    pub fn get_output(&self, module_name: &str, output_key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT output_value FROM outputs WHERE module_name = ?1 AND output_key = ?2",
        )?;
        let result = stmt
            .query_row(rusqlite::params![module_name, output_key], |row| row.get(0))
            .ok();
        Ok(result)
    }

    /// Get all outputs for a module.
    pub fn get_module_outputs(&self, module_name: &str) -> Result<Vec<ModuleOutput>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT module_name, output_key, output_value FROM outputs WHERE module_name = ?1",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![module_name], |row| {
                Ok(ModuleOutput {
                    module_name: row.get(0)?,
                    output_key: row.get(1)?,
                    output_value: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Clear all outputs for a module.
    pub fn clear_outputs(&self, module_name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM outputs WHERE module_name = ?1",
            rusqlite::params![module_name],
        )?;
        Ok(())
    }

    /// Record the start of an execution run.
    pub fn start_run(&self, modules_planned: i32) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO runs (id, started_at, status, modules_planned) VALUES (?1, datetime('now'), 'running', ?2)",
            rusqlite::params![id, modules_planned],
        )?;
        Ok(id)
    }

    /// Complete an execution run.
    pub fn complete_run(&self, run_id: &str, status: &str, modules_applied: i32) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE runs SET completed_at = datetime('now'), status = ?2, modules_applied = ?3 WHERE id = ?1",
            rusqlite::params![run_id, status, modules_applied],
        )?;
        Ok(())
    }

    /// Get the most recent run.
    pub fn get_latest_run(&self) -> Result<Option<LegacyRunRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, started_at, completed_at, status, modules_planned, modules_applied FROM runs ORDER BY started_at DESC LIMIT 1",
        )?;
        let result = stmt
            .query_row([], |row| {
                Ok(LegacyRunRecord {
                    id: row.get(0)?,
                    started_at: row.get(1)?,
                    completed_at: row.get(2)?,
                    status: row.get(3)?,
                    modules_planned: row.get(4)?,
                    modules_applied: row.get(5)?,
                })
            })
            .ok();
        Ok(result)
    }
}

/// Legacy run record for the v1 YAML module pipeline.
#[derive(Debug, Clone)]
pub struct LegacyRunRecord {
    pub id: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub status: String,
    pub modules_planned: i32,
    pub modules_applied: i32,
}
