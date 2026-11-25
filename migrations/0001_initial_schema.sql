-- 0001_initial_schema.sql
-- KURAL: Bu dosya yayınlandıktan sonra ASLA düzenlenmez. Şema değişiklikleri
-- yeni numaralı dosya olarak eklenir (0002_<konu>.sql). Migrasyon yürütücüsü
-- (core-engine) her dosyayı tek bir işlem içinde çalıştırır, ardından
-- schema_migrations'a satır ekler ve PRAGMA user_version = <numara> yazar.
-- WAL modu (PRAGMA journal_mode = WAL) bağlantı açılışında ayarlanır; bir
-- işlem içinde değiştirilemeyeceği için burada yer almaz.
--
-- Durum metinleri shared_types::TransactionStatus / StepStatus::as_str() ile
-- birebir aynıdır; CHECK kısıtları bunları depolama sözleşmesi olarak kilitler.

CREATE TABLE schema_migrations (
    version         INTEGER PRIMARY KEY,
    name            TEXT    NOT NULL,
    checksum_sha256 TEXT    NOT NULL,          -- yayınlanmış dosyanın değişmediğini doğrular
    applied_at_ms   INTEGER NOT NULL
) STRICT;

CREATE TABLE transactions (
    id                TEXT    PRIMARY KEY,     -- TransactionId (ör. UUIDv7)
    created_at_ms     INTEGER NOT NULL,
    updated_at_ms     INTEGER NOT NULL,
    status            TEXT    NOT NULL CHECK (status IN
                        ('PENDING','APPLYING','COMMITTED','ROLLING_BACK','ROLLED_BACK','FAILED')),
    kind              TEXT    NOT NULL DEFAULT 'apply' CHECK (kind IN ('apply','rollback')),
    -- kind = 'rollback' ise geri alınan işlemin kimliği
    reverts_transaction_id TEXT REFERENCES transactions(id),
    profile           TEXT,                    -- shared_types::Profile (snake_case) veya NULL
    tweak_ids_json    TEXT    NOT NULL,        -- JSON dizi: ["privacy.disable-telemetry", ...]
    windows_build     INTEGER NOT NULL,
    windows_ubr       INTEGER NOT NULL,
    app_version       TEXT    NOT NULL,
    error_json        TEXT                     -- serileştirilmiş AppError veya NULL
) STRICT;

CREATE INDEX idx_transactions_status  ON transactions(status);
CREATE INDEX idx_transactions_created ON transactions(created_at_ms);

-- Her eylem, sisteme dokunulmadan ÖNCE anlık görüntüsüyle birlikte PENDING
-- olarak yazılır; provider apply başarılı olunca APPLIED işaretlenir.
-- Geri alma, APPLIED adımları seq DESC sırasıyla dolaşır.
CREATE TABLE action_snapshots (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id    TEXT    NOT NULL REFERENCES transactions(id) ON DELETE RESTRICT,
    seq               INTEGER NOT NULL,        -- işlem içindeki 0 tabanlı uygulama sırası
    tweak_id          TEXT    NOT NULL,
    provider          TEXT    NOT NULL,        -- ProviderKind::as_str()
    resource_id       TEXT    NOT NULL,        -- ActionSpec::resource_id()
    action_json       TEXT    NOT NULL,        -- serileştirilmiş ActionSpec (revert için gerekir)
    existed           INTEGER NOT NULL CHECK (existed IN (0, 1)),
    prior_state_json  TEXT,                    -- ResourceState; existed = 0 ise NULL
    security_descriptor_sddl TEXT,
    status            TEXT    NOT NULL CHECK (status IN
                        ('PENDING','APPLIED','REVERTED','REVERT_FAILED')),
    error_json        TEXT,
    updated_at_ms     INTEGER NOT NULL,
    UNIQUE (transaction_id, seq),
    CHECK ((existed = 1) = (prior_state_json IS NOT NULL))
) STRICT;

CREATE INDEX idx_action_snapshots_resource ON action_snapshots(resource_id);

-- Önce/Sonra metrikleri (system-probe). transaction_id NULL ise bağımsız ölçüm.
CREATE TABLE metric_baselines (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id    TEXT    REFERENCES transactions(id) ON DELETE RESTRICT,
    phase             TEXT    NOT NULL CHECK (phase IN ('before','after')),
    metric            TEXT    NOT NULL,        -- MetricKind::as_str()
    value             REAL    NOT NULL,
    captured_at_ms    INTEGER NOT NULL
) STRICT;

CREATE INDEX idx_metric_baselines_tx ON metric_baselines(transaction_id, phase);
