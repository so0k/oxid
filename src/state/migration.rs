use anyhow::Result;
use rusqlite::Connection;

use super::schema;

/// Check and apply migrations for SQLite backend.
pub fn check_and_migrate(conn: &Connection) -> Result<()> {
    // Check if schema_version table exists
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_version'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !table_exists {
        // Fresh install — apply full schema
        conn.execute_batch(schema::CREATE_TABLES_SQL)?;
        conn.execute_batch(schema::CREATE_INDEXES_SQL)?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO schema_version (version, applied_at, description) VALUES (?1, ?2, ?3)",
            rusqlite::params![schema::SCHEMA_VERSION, now, "Initial schema"],
        )?;
        return Ok(());
    }

    let current_version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if current_version < schema::SCHEMA_VERSION {
        apply_migrations(conn, current_version)?;
    }

    Ok(())
}

fn apply_migrations(conn: &Connection, from_version: i32) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();

    // Future migrations go here
    // Each migration bumps the version and applies incremental DDL
    if from_version < 1 {
        // Migration 0 -> 1: Initial schema
        conn.execute_batch(schema::CREATE_TABLES_SQL)?;
        conn.execute_batch(schema::CREATE_INDEXES_SQL)?;
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (version, applied_at, description) VALUES (?1, ?2, ?3)",
            rusqlite::params![1, now, "Initial resource-level schema"],
        )?;
    }

    // Migration 1 -> 2 would go here when schema changes
    // if from_version < 2 {
    //     conn.execute_batch("ALTER TABLE resources ADD COLUMN new_col TEXT;")?;
    //     conn.execute("INSERT INTO schema_version ...", params![2, now, "Add new_col"])?;
    // }

    Ok(())
}
