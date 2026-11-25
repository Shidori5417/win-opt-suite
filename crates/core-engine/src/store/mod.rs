//! SQLite state store: connection set-up, migrations and typed repository
//! functions for `transactions`, `action_snapshots` and `metric_baselines`.

pub mod migrations;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use shared_types::{
    ActionSpec, AppError, AppResult, MetricKind, MetricPhase, MetricSample, Profile, ProviderKind,
    ResourceId, StateSnapshot, StepStatus, TransactionId, TransactionStatus, TransactionSummary,
    TweakId,
};

use crate::clock::system_now_ms;

/// How long a connection waits on a locked database before giving up.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Error type used inside the store; converted to [`AppError`] at the boundary.
#[derive(Debug)]
enum StoreError {
    Sql(rusqlite::Error),
    App(AppError),
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sql(e)
    }
}

impl From<AppError> for StoreError {
    fn from(e: AppError) -> Self {
        StoreError::App(e)
    }
}

impl From<StoreError> for AppError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Sql(e) => AppError::Storage {
                message: e.to_string(),
            },
            StoreError::App(e) => e,
        }
    }
}

type StoreResult<T> = Result<T, StoreError>;

fn corrupt(message: impl Into<String>) -> StoreError {
    StoreError::App(AppError::Storage {
        message: message.into(),
    })
}

fn to_json<T: Serialize>(value: &T) -> StoreResult<String> {
    serde_json::to_string(value).map_err(|e| {
        StoreError::App(AppError::Serialization {
            message: e.to_string(),
        })
    })
}

fn from_json<T: DeserializeOwned>(text: &str) -> StoreResult<T> {
    serde_json::from_str(text).map_err(|e| {
        StoreError::App(AppError::Serialization {
            message: e.to_string(),
        })
    })
}

/// Serializes a unit-like enum (e.g. [`Profile`]) to its bare string form.
fn enum_to_string<T: Serialize>(value: &T) -> StoreResult<String> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => Ok(s),
        Ok(other) => Err(corrupt(format!("expected string enum, got {other}"))),
        Err(e) => Err(StoreError::App(AppError::Serialization {
            message: e.to_string(),
        })),
    }
}

fn enum_from_string<T: DeserializeOwned>(text: &str) -> StoreResult<T> {
    from_json(&serde_json::Value::String(text.to_owned()).to_string())
}

/// Data needed to create a transaction row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTransaction {
    pub id: TransactionId,
    pub created_at_ms: i64,
    pub status: TransactionStatus,
    pub profile: Option<Profile>,
    pub tweak_ids: Vec<TweakId>,
    pub windows_build: u32,
    pub windows_ubr: u32,
    pub app_version: String,
}

/// A stored `transactions` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionRecord {
    pub id: TransactionId,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub status: TransactionStatus,
    pub profile: Option<Profile>,
    pub tweak_ids: Vec<TweakId>,
    pub windows_build: u32,
    pub windows_ubr: u32,
    pub app_version: String,
    pub error: Option<AppError>,
}

/// Data needed to create an `action_snapshots` row (always written `PENDING`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewStep {
    pub transaction_id: TransactionId,
    pub seq: u32,
    pub tweak_id: TweakId,
    pub action: ActionSpec,
    pub snapshot: StateSnapshot,
    pub updated_at_ms: i64,
}

/// A stored `action_snapshots` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    pub transaction_id: TransactionId,
    pub seq: u32,
    pub tweak_id: TweakId,
    pub provider: ProviderKind,
    pub resource: ResourceId,
    pub action: ActionSpec,
    pub snapshot: StateSnapshot,
    pub status: StepStatus,
    pub error: Option<AppError>,
    pub updated_at_ms: i64,
}

/// Handle to the SQLite database. Cheap to share behind a reference; the
/// connection is serialised by an internal mutex.
#[derive(Debug)]
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Opens (creating if needed) the database file, configures it
    /// (`WAL`, `synchronous=FULL`, `foreign_keys=ON`, busy timeout) and runs
    /// all pending migrations. Opening an existing database is idempotent.
    pub fn open(path: impl AsRef<Path>) -> AppResult<Self> {
        let conn = Connection::open(path).map_err(storage)?;
        Self::init(conn, true)
    }

    /// Opens a private in-memory database (tests, dry runs).
    pub fn open_in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory().map_err(storage)?;
        Self::init(conn, false)
    }

    fn init(mut conn: Connection, wal: bool) -> AppResult<Self> {
        conn.busy_timeout(BUSY_TIMEOUT).map_err(storage)?;
        if wal {
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get::<_, String>(0))
                .map_err(storage)?;
        }
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(storage)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(storage)?;
        migrations::migrate(&mut conn, migrations::MIGRATIONS, system_now_ms())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> StoreResult<T>) -> AppResult<T> {
        let guard: MutexGuard<'_, Connection> =
            self.conn.lock().map_err(|_| AppError::Storage {
                message: "database connection mutex poisoned".to_owned(),
            })?;
        f(&guard).map_err(AppError::from)
    }

    /// Current schema version (`PRAGMA user_version`), i.e. the number of the
    /// latest applied migration.
    pub fn schema_version(&self) -> AppResult<u32> {
        self.with_conn(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))?))
    }

    // ------------------------------------------------------- transactions --

    /// Inserts a new transaction row.
    pub fn insert_transaction(&self, tx: &NewTransaction) -> AppResult<()> {
        self.with_conn(|c| {
            let profile = tx.profile.as_ref().map(enum_to_string).transpose()?;
            c.execute(
                "INSERT INTO transactions (id, created_at_ms, updated_at_ms, status, kind, \
                 profile, tweak_ids_json, windows_build, windows_ubr, app_version) \
                 VALUES (?1, ?2, ?2, ?3, 'apply', ?4, ?5, ?6, ?7, ?8)",
                params![
                    tx.id.as_str(),
                    tx.created_at_ms,
                    tx.status.as_str(),
                    profile,
                    to_json(&tx.tweak_ids)?,
                    tx.windows_build,
                    tx.windows_ubr,
                    tx.app_version,
                ],
            )?;
            Ok(())
        })
    }

    /// Updates a transaction's status (and, when given, its error).
    pub fn set_transaction_status(
        &self,
        id: &TransactionId,
        status: TransactionStatus,
        updated_at_ms: i64,
        error: Option<&AppError>,
    ) -> AppResult<()> {
        self.with_conn(|c| {
            let error_json = error.map(to_json).transpose()?;
            let changed = c.execute(
                "UPDATE transactions SET status = ?2, updated_at_ms = ?3, \
                 error_json = COALESCE(?4, error_json) WHERE id = ?1",
                params![id.as_str(), status.as_str(), updated_at_ms, error_json],
            )?;
            if changed == 0 {
                return Err(StoreError::App(AppError::NotFound {
                    resource: format!("transaction {id}"),
                }));
            }
            Ok(())
        })
    }

    /// Loads one transaction.
    pub fn get_transaction(&self, id: &TransactionId) -> AppResult<Option<TransactionRecord>> {
        self.with_conn(|c| {
            c.query_row(
                &format!("SELECT {TX_COLUMNS} FROM transactions WHERE id = ?1"),
                [id.as_str()],
                tx_row,
            )
            .optional()?
            .map(TxRow::into_record)
            .transpose()
        })
    }

    /// Transactions in one of `statuses`, oldest first.
    pub fn transactions_with_status(
        &self,
        statuses: &[TransactionStatus],
    ) -> AppResult<Vec<TransactionRecord>> {
        let mut all = self.with_conn(|c| load_transactions(c, "created_at_ms ASC, rowid ASC"))?;
        all.retain(|t| statuses.contains(&t.status));
        Ok(all)
    }

    /// Summaries of all transactions, newest first.
    pub fn summaries(&self) -> AppResult<Vec<TransactionSummary>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {TX_COLUMNS}, \
                 (SELECT count(*) FROM action_snapshots s WHERE s.transaction_id = transactions.id) \
                 FROM transactions ORDER BY created_at_ms DESC, rowid DESC"
            ))?;
            let rows = stmt.query_map([], |r| Ok((tx_row(r)?, r.get::<_, u32>(10)?)))?;
            let mut out = Vec::new();
            for row in rows {
                let (row, step_count) = row?;
                let rec = row.into_record()?;
                out.push(TransactionSummary {
                    id: rec.id,
                    created_at_unix_ms: rec.created_at_ms,
                    status: rec.status,
                    profile: rec.profile,
                    tweak_ids: rec.tweak_ids,
                    step_count,
                });
            }
            Ok(out)
        })
    }

    // -------------------------------------------------------------- steps --

    /// Inserts a step (snapshot) in `PENDING` state. Durable on return.
    pub fn insert_step(&self, step: &NewStep) -> AppResult<()> {
        self.with_conn(|c| {
            let prior = step.snapshot.state.as_ref().map(to_json).transpose()?;
            c.execute(
                "INSERT INTO action_snapshots (transaction_id, seq, tweak_id, provider, \
                 resource_id, action_json, existed, prior_state_json, \
                 security_descriptor_sddl, status, updated_at_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'PENDING', ?10)",
                params![
                    step.transaction_id.as_str(),
                    step.seq,
                    step.tweak_id.as_str(),
                    step.action.provider_kind().as_str(),
                    step.action.resource_id().as_str(),
                    to_json(&step.action)?,
                    step.snapshot.existed,
                    prior,
                    step.snapshot.security_descriptor_sddl,
                    step.updated_at_ms,
                ],
            )?;
            Ok(())
        })
    }

    /// Updates one step's status (and, when given, its error).
    pub fn set_step_status(
        &self,
        id: &TransactionId,
        seq: u32,
        status: StepStatus,
        updated_at_ms: i64,
        error: Option<&AppError>,
    ) -> AppResult<()> {
        self.with_conn(|c| {
            let error_json = error.map(to_json).transpose()?;
            let changed = c.execute(
                "UPDATE action_snapshots SET status = ?3, updated_at_ms = ?4, \
                 error_json = ?5 WHERE transaction_id = ?1 AND seq = ?2",
                params![id.as_str(), seq, status.as_str(), updated_at_ms, error_json],
            )?;
            if changed == 0 {
                return Err(StoreError::App(AppError::NotFound {
                    resource: format!("step {seq} of transaction {id}"),
                }));
            }
            Ok(())
        })
    }

    /// All steps of a transaction ordered by ascending `seq`.
    pub fn steps(&self, id: &TransactionId) -> AppResult<Vec<StepRecord>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT transaction_id, seq, tweak_id, provider, resource_id, action_json, \
                 existed, prior_state_json, security_descriptor_sddl, status, error_json, \
                 updated_at_ms FROM action_snapshots WHERE transaction_id = ?1 ORDER BY seq ASC",
            )?;
            let rows = stmt.query_map([id.as_str()], StepRow::read)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?.into_record()?);
            }
            Ok(out)
        })
    }

    // ------------------------------------------------------------ metrics --

    /// Stores a metric sample, optionally tied to a transaction.
    pub fn insert_metric(
        &self,
        transaction_id: Option<&TransactionId>,
        sample: &MetricSample,
    ) -> AppResult<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO metric_baselines (transaction_id, phase, metric, value, \
                 captured_at_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    transaction_id.map(TransactionId::as_str),
                    sample.phase.as_str(),
                    sample.kind.as_str(),
                    sample.value,
                    sample.captured_at_unix_ms,
                ],
            )?;
            Ok(())
        })
    }

    /// Metric samples of a transaction (or the standalone ones for `None`),
    /// in insertion order.
    pub fn metrics(&self, transaction_id: Option<&TransactionId>) -> AppResult<Vec<MetricSample>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT phase, metric, value, captured_at_ms FROM metric_baselines \
                 WHERE transaction_id IS ?1 ORDER BY id ASC",
            )?;
            let rows = stmt.query_map([transaction_id.map(TransactionId::as_str)], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (phase, metric, value, captured_at_unix_ms) = row?;
                out.push(MetricSample {
                    kind: enum_from_string::<MetricKind>(&metric)?,
                    phase: enum_from_string::<MetricPhase>(&phase)?,
                    value,
                    captured_at_unix_ms,
                });
            }
            Ok(out)
        })
    }
}

fn storage(e: rusqlite::Error) -> AppError {
    AppError::Storage {
        message: e.to_string(),
    }
}

const TX_COLUMNS: &str = "id, created_at_ms, updated_at_ms, status, profile, tweak_ids_json, \
                          windows_build, windows_ubr, app_version, error_json";

/// Raw column values of a `transactions` row (converted outside the rusqlite
/// closure so that domain errors keep their type).
struct TxRow {
    id: String,
    created_at_ms: i64,
    updated_at_ms: i64,
    status: String,
    profile: Option<String>,
    tweak_ids_json: String,
    windows_build: u32,
    windows_ubr: u32,
    app_version: String,
    error_json: Option<String>,
}

fn tx_row(r: &Row<'_>) -> rusqlite::Result<TxRow> {
    Ok(TxRow {
        id: r.get(0)?,
        created_at_ms: r.get(1)?,
        updated_at_ms: r.get(2)?,
        status: r.get(3)?,
        profile: r.get(4)?,
        tweak_ids_json: r.get(5)?,
        windows_build: r.get(6)?,
        windows_ubr: r.get(7)?,
        app_version: r.get(8)?,
        error_json: r.get(9)?,
    })
}

impl TxRow {
    fn into_record(self) -> StoreResult<TransactionRecord> {
        Ok(TransactionRecord {
            status: TransactionStatus::parse(&self.status)
                .ok_or_else(|| corrupt(format!("unknown transaction status `{}`", self.status)))?,
            profile: self
                .profile
                .as_deref()
                .map(enum_from_string::<Profile>)
                .transpose()?,
            tweak_ids: from_json(&self.tweak_ids_json)?,
            error: self.error_json.as_deref().map(from_json).transpose()?,
            id: TransactionId::new(self.id),
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
            windows_build: self.windows_build,
            windows_ubr: self.windows_ubr,
            app_version: self.app_version,
        })
    }
}

fn load_transactions(c: &Connection, order_by: &str) -> StoreResult<Vec<TransactionRecord>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {TX_COLUMNS} FROM transactions ORDER BY {order_by}"
    ))?;
    let rows = stmt.query_map([], tx_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?.into_record()?);
    }
    Ok(out)
}

struct StepRow {
    transaction_id: String,
    seq: u32,
    tweak_id: String,
    provider: String,
    resource_id: String,
    action_json: String,
    existed: bool,
    prior_state_json: Option<String>,
    sddl: Option<String>,
    status: String,
    error_json: Option<String>,
    updated_at_ms: i64,
}

impl StepRow {
    fn read(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            transaction_id: r.get(0)?,
            seq: r.get(1)?,
            tweak_id: r.get(2)?,
            provider: r.get(3)?,
            resource_id: r.get(4)?,
            action_json: r.get(5)?,
            existed: r.get(6)?,
            prior_state_json: r.get(7)?,
            sddl: r.get(8)?,
            status: r.get(9)?,
            error_json: r.get(10)?,
            updated_at_ms: r.get(11)?,
        })
    }

    fn into_record(self) -> StoreResult<StepRecord> {
        let provider = ProviderKind::parse(&self.provider)
            .ok_or_else(|| corrupt(format!("unknown provider `{}`", self.provider)))?;
        let resource = ResourceId::from_stored(self.resource_id);
        let snapshot = StateSnapshot {
            resource: resource.clone(),
            provider,
            existed: self.existed,
            state: self
                .prior_state_json
                .as_deref()
                .map(from_json)
                .transpose()?,
            security_descriptor_sddl: self.sddl,
        };
        Ok(StepRecord {
            transaction_id: TransactionId::new(self.transaction_id),
            seq: self.seq,
            tweak_id: TweakId::new(self.tweak_id),
            provider,
            resource,
            action: from_json(&self.action_json)?,
            snapshot,
            status: StepStatus::parse(&self.status)
                .ok_or_else(|| corrupt(format!("unknown step status `{}`", self.status)))?,
            error: self.error_json.as_deref().map(from_json).transpose()?,
            updated_at_ms: self.updated_at_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use shared_types::{RegistryAction, RegistryHive, RegistryValue, ResourceState};

    use super::*;

    #[test]
    fn schema_version_matches_latest_migration() {
        let store = Store::open_in_memory().unwrap();
        let latest = migrations::MIGRATIONS.iter().map(|m| m.version).max();
        assert_eq!(Some(store.schema_version().unwrap()), latest);
    }

    fn action() -> ActionSpec {
        ActionSpec::Registry(RegistryAction {
            hive: RegistryHive::LocalMachine,
            key: r"SOFTWARE\X".into(),
            value_name: "V".into(),
            value: Some(RegistryValue::Dword(1)),
            delete: false,
        })
    }

    fn new_tx(id: &str, at: i64) -> NewTransaction {
        NewTransaction {
            id: TransactionId::new(id),
            created_at_ms: at,
            status: TransactionStatus::Pending,
            profile: Some(Profile::Gaming),
            tweak_ids: vec![TweakId::new("a.b")],
            windows_build: 26100,
            windows_ubr: 7,
            app_version: "0.1.0".into(),
        }
    }

    #[test]
    fn transaction_roundtrip_and_ordering() {
        let s = Store::open_in_memory().unwrap();
        s.insert_transaction(&new_tx("t1", 10)).unwrap();
        s.insert_transaction(&new_tx("t2", 20)).unwrap();
        let err = AppError::LockUnavailable;
        s.set_transaction_status(
            &TransactionId::new("t1"),
            TransactionStatus::Failed,
            30,
            Some(&err),
        )
        .unwrap();
        let rec = s
            .get_transaction(&TransactionId::new("t1"))
            .unwrap()
            .unwrap();
        assert_eq!(rec.status, TransactionStatus::Failed);
        assert_eq!(rec.profile, Some(Profile::Gaming));
        assert_eq!(rec.error, Some(err));
        assert_eq!(rec.updated_at_ms, 30);
        let ids: Vec<_> = s
            .summaries()
            .unwrap()
            .into_iter()
            .map(|t| t.id.to_string())
            .collect();
        assert_eq!(ids, ["t2", "t1"]);
        assert!(
            s.set_transaction_status(
                &TransactionId::new("nope"),
                TransactionStatus::Failed,
                1,
                None
            )
            .is_err()
        );
    }

    #[test]
    fn step_roundtrip_present_and_absent() {
        let s = Store::open_in_memory().unwrap();
        s.insert_transaction(&new_tx("t1", 1)).unwrap();
        let a = action();
        let present = StateSnapshot::present(
            a.resource_id(),
            ProviderKind::Registry,
            ResourceState::RegistryValue {
                value: RegistryValue::Dword(9),
            },
        )
        .with_sddl("D:(A;;GA;;;WD)");
        let absent = StateSnapshot::absent(a.resource_id(), ProviderKind::Registry);
        for (seq, snap) in [(0, present.clone()), (1, absent.clone())] {
            s.insert_step(&NewStep {
                transaction_id: TransactionId::new("t1"),
                seq,
                tweak_id: TweakId::new("a.b"),
                action: a.clone(),
                snapshot: snap,
                updated_at_ms: 5,
            })
            .unwrap();
        }
        s.set_step_status(
            &TransactionId::new("t1"),
            1,
            StepStatus::RevertFailed,
            6,
            Some(&AppError::LockUnavailable),
        )
        .unwrap();
        let steps = s.steps(&TransactionId::new("t1")).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].snapshot, present);
        assert_eq!(steps[0].status, StepStatus::Pending);
        assert_eq!(steps[1].snapshot, absent);
        assert_eq!(steps[1].status, StepStatus::RevertFailed);
        assert_eq!(steps[1].action, a);
        assert_eq!(s.summaries().unwrap()[0].step_count, 2);
    }

    #[test]
    fn step_requires_existing_transaction() {
        let s = Store::open_in_memory().unwrap();
        let a = action();
        let res = s.insert_step(&NewStep {
            transaction_id: TransactionId::new("missing"),
            seq: 0,
            tweak_id: TweakId::new("a.b"),
            snapshot: StateSnapshot::absent(a.resource_id(), ProviderKind::Registry),
            action: a,
            updated_at_ms: 1,
        });
        assert!(matches!(res, Err(AppError::Storage { .. })));
    }

    #[test]
    fn metrics_roundtrip() {
        let s = Store::open_in_memory().unwrap();
        s.insert_transaction(&new_tx("t1", 1)).unwrap();
        let sample = MetricSample {
            kind: MetricKind::RunningServices,
            phase: MetricPhase::After,
            value: 42.5,
            captured_at_unix_ms: 9,
        };
        let id = TransactionId::new("t1");
        s.insert_metric(Some(&id), &sample).unwrap();
        s.insert_metric(None, &sample).unwrap();
        assert_eq!(s.metrics(Some(&id)).unwrap(), vec![sample.clone()]);
        assert_eq!(s.metrics(None).unwrap(), vec![sample]);
    }

    #[test]
    fn file_database_uses_wal_and_reopens() {
        let dir = std::env::temp_dir().join(format!("core-engine-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.db");
        {
            let s = Store::open(&path).unwrap();
            s.insert_transaction(&new_tx("t1", 1)).unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.summaries().unwrap().len(), 1);
        let mode: String = s
            .with_conn(|c| Ok(c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(mode, "wal");
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
