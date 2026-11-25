//! Forward-only SQLite migration runner.
//!
//! Every migration file under `migrations/` is embedded with `include_str!`
//! and applied inside a single transaction together with its
//! `schema_migrations` row and the `PRAGMA user_version` bump. A published
//! migration whose SHA-256 differs from the recorded one is rejected.

use std::fmt::Write as _;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use shared_types::{AppError, AppResult};

/// One embedded, numbered migration script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

impl Migration {
    /// Lower-case hex SHA-256 of the script text.
    pub fn checksum(&self) -> String {
        let digest = Sha256::digest(self.sql.as_bytes());
        let mut out = String::with_capacity(64);
        for byte in digest {
            // Writing to a String cannot fail.
            let _ = write!(out, "{byte:02x}");
        }
        out
    }
}

/// All migrations shipped with this build, ordered by version.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial_schema",
    sql: include_str!("../../../../migrations/0001_initial_schema.sql"),
}];

fn migration_err(version: u32, err: impl std::fmt::Display) -> AppError {
    AppError::Migration {
        version,
        message: err.to_string(),
    }
}

/// Applies every not-yet-applied migration of `migrations` (which must be
/// sorted by strictly increasing version) and verifies the checksum of the
/// already-applied ones. Idempotent.
pub fn migrate(conn: &mut Connection, migrations: &[Migration], now_ms: i64) -> AppResult<()> {
    if migrations
        .iter()
        .zip(migrations.iter().skip(1))
        .any(|(a, b)| a.version >= b.version)
    {
        return Err(migration_err(
            0,
            "migration list is not strictly ordered by version",
        ));
    }
    for m in migrations {
        apply_one(conn, m, now_ms)?;
    }
    check_no_unknown_versions(conn, migrations)
}

fn table_exists(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
        [],
        |_| Ok(()),
    )
    .optional()
    .map(|o| o.is_some())
}

fn apply_one(conn: &mut Connection, m: &Migration, now_ms: i64) -> AppResult<()> {
    let version = m.version;
    let checksum = m.checksum();
    // IMMEDIATE: take the write lock up-front so two processes cannot both
    // decide the migration is pending.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| migration_err(version, e))?;

    let recorded: Option<String> = if table_exists(&tx).map_err(|e| migration_err(version, e))? {
        tx.query_row(
            "SELECT checksum_sha256 FROM schema_migrations WHERE version = ?1",
            [version],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| migration_err(version, e))?
    } else {
        None
    };

    if let Some(recorded) = recorded {
        return if recorded == checksum {
            Ok(())
        } else {
            Err(migration_err(
                version,
                format!(
                    "checksum mismatch for `{}`: recorded {recorded}, embedded {checksum}",
                    m.name
                ),
            ))
        };
    }

    tx.execute_batch(m.sql)
        .map_err(|e| migration_err(version, e))?;
    tx.execute(
        "INSERT INTO schema_migrations (version, name, checksum_sha256, applied_at_ms) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![version, m.name, checksum, now_ms],
    )
    .map_err(|e| migration_err(version, e))?;
    // PRAGMA does not accept bound parameters; `version` is a u32.
    tx.execute_batch(&format!("PRAGMA user_version = {version};"))
        .map_err(|e| migration_err(version, e))?;
    tx.commit().map_err(|e| migration_err(version, e))
}

/// A database written by a newer build must not be silently opened.
fn check_no_unknown_versions(conn: &Connection, migrations: &[Migration]) -> AppResult<()> {
    let mut stmt = conn
        .prepare("SELECT version FROM schema_migrations ORDER BY version")
        .map_err(|e| migration_err(0, e))?;
    let versions = stmt
        .query_map([], |r| r.get::<_, u32>(0))
        .map_err(|e| migration_err(0, e))?;
    for v in versions {
        let v = v.map_err(|e| migration_err(0, e))?;
        if !migrations.iter().any(|m| m.version == v) {
            return Err(migration_err(
                v,
                "database contains a migration unknown to this build (downgrade?)",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_version(conn: &Connection) -> u32 {
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn applies_and_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, MIGRATIONS, 1).unwrap();
        migrate(&mut conn, MIGRATIONS, 2).unwrap();
        assert_eq!(user_version(&conn), 1);
        let (n, sum, at): (u32, String, i64) = conn
            .query_row(
                "SELECT count(*), max(checksum_sha256), max(applied_at_ms) FROM schema_migrations",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(sum, MIGRATIONS[0].checksum());
        assert_eq!(at, 1, "second run must not rewrite the row");
    }

    #[test]
    fn detects_tampered_migration() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, MIGRATIONS, 1).unwrap();
        let tampered = [Migration {
            sql: "CREATE TABLE evil (x INTEGER);",
            ..MIGRATIONS[0]
        }];
        let err = migrate(&mut conn, &tampered, 2).unwrap_err();
        assert!(
            matches!(err, AppError::Migration { version: 1, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn failed_migration_rolls_back_atomically() {
        let mut conn = Connection::open_in_memory().unwrap();
        let bad = [Migration {
            version: 1,
            name: "bad",
            sql: "CREATE TABLE schema_migrations (version INTEGER); SELECT * FROM nope;",
        }];
        assert!(migrate(&mut conn, &bad, 1).is_err());
        assert!(!table_exists(&conn).unwrap());
        assert_eq!(user_version(&conn), 0);
    }

    #[test]
    fn rejects_database_from_newer_build() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, MIGRATIONS, 1).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations VALUES (99, 'future', 'x', 0)",
            [],
        )
        .unwrap();
        assert!(matches!(
            migrate(&mut conn, MIGRATIONS, 2),
            Err(AppError::Migration { version: 99, .. })
        ));
    }
}
