//! The SQLite database: opening, schema migrations, and raw access to the cache tables.
//!
//! This layer only stores and loads rows. Whether a row may be served (freshness, finality)
//! is decided by [`crate::CachedBitcoin`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ct_core::{AddressSummary, OutputSpend, UtxoTx};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::StoreError;

/// Schema migrations, applied in order. `PRAGMA user_version` holds how many have run.
/// Never change a migration that has been released; append a new one.
const MIGRATIONS: &[&str] = &[
    // 1: Bitcoin cache. `json` holds a serialised `ct-core` type; `fetched_at` is Unix ms.
    "CREATE TABLE btc_tx (
        txid       TEXT PRIMARY KEY NOT NULL,
        json       TEXT NOT NULL,
        fetched_at INTEGER NOT NULL
    );
    CREATE TABLE btc_outspends (
        txid       TEXT PRIMARY KEY NOT NULL,
        json       TEXT NOT NULL,
        fetched_at INTEGER NOT NULL
    );
    CREATE TABLE btc_address_summary (
        address    TEXT PRIMARY KEY NOT NULL,
        json       TEXT NOT NULL,
        fetched_at INTEGER NOT NULL
    );
    -- One page of an address's confirmed history: a JSON array of txids, newest first.
    -- after_txid is '' for the newest page. The txs themselves live in btc_tx.
    CREATE TABLE btc_address_txs (
        address    TEXT NOT NULL,
        after_txid TEXT NOT NULL,
        txids      TEXT NOT NULL,
        fetched_at INTEGER NOT NULL,
        PRIMARY KEY (address, after_txid)
    );",
];

/// Handle to the local database. Cheap to clone; clones share one connection.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

/// A stored value and when it was fetched from the network.
pub(crate) struct Cached<T> {
    pub(crate) value: T,
    /// Unix milliseconds.
    pub(crate) fetched_at: i64,
}

impl<T> Cached<T> {
    /// Whether the value is younger than `ttl`. A timestamp in the future (the clock moved
    /// backwards) counts as stale.
    pub(crate) fn is_fresh(&self, ttl: Duration) -> bool {
        let age = now_ms() - self.fetched_at;
        age >= 0 && (age as u128) < ttl.as_millis()
    }
}

impl Store {
    /// Open (or create) the database at `path`, creating parent directories as needed.
    /// Uses WAL mode and brings the schema up to date.
    pub fn open(path: &Path) -> Result<Store, StoreError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        // Another process (CLI and app) may hold the write lock briefly.
        conn.busy_timeout(Duration::from_secs(5))?;
        let mode: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            tracing::warn!(mode, "SQLite WAL mode unavailable for the cache");
        }
        // Safe with WAL: a power cut can lose the last writes, never corrupt the file.
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    /// A private in-memory database, discarded when the last clone is dropped.
    pub fn open_in_memory() -> Result<Store, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    /// `<platform local data dir>/chaintrail/cache.sqlite`, e.g.
    /// `~/.local/share/chaintrail/cache.sqlite` on Linux. `None` if the platform directory
    /// cannot be determined.
    pub fn default_path() -> Option<PathBuf> {
        Some(data_local_dir()?.join("chaintrail").join("cache.sqlite"))
    }

    fn init(mut conn: Connection) -> Result<Store, StoreError> {
        migrate(&mut conn)?;
        Ok(Store {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Run blocking database work on Tokio's blocking thread pool.
    pub(crate) async fn run<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        F: FnOnce(&Store) -> Result<T, StoreError> + Send + 'static,
        T: Send + 'static,
    {
        let store = self.clone();
        tokio::task::spawn_blocking(move || f(&store)).await?
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock cannot leave SQLite inconsistent (an open
        // transaction is rolled back when dropped), so a poisoned lock is still usable.
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn get_tx(&self, txid: &str) -> Result<Option<UtxoTx>, StoreError> {
        let row = load(&self.conn(), "btc_tx", "txid", txid)?;
        Ok(row.map(|r| r.value))
    }

    pub(crate) fn put_tx(&self, txid: &str, tx: &UtxoTx) -> Result<(), StoreError> {
        save(&self.conn(), "btc_tx", "txid", txid, tx)
    }

    pub(crate) fn get_outspends(
        &self,
        txid: &str,
    ) -> Result<Option<Cached<Vec<OutputSpend>>>, StoreError> {
        load(&self.conn(), "btc_outspends", "txid", txid)
    }

    pub(crate) fn put_outspends(
        &self,
        txid: &str,
        spends: &[OutputSpend],
    ) -> Result<(), StoreError> {
        save(&self.conn(), "btc_outspends", "txid", txid, spends)
    }

    pub(crate) fn get_address_summary(
        &self,
        address: &str,
    ) -> Result<Option<Cached<AddressSummary>>, StoreError> {
        load(&self.conn(), "btc_address_summary", "address", address)
    }

    pub(crate) fn put_address_summary(
        &self,
        address: &str,
        summary: &AddressSummary,
    ) -> Result<(), StoreError> {
        save(
            &self.conn(),
            "btc_address_summary",
            "address",
            address,
            summary,
        )
    }

    /// The txids of a cached history page (`after_txid` is `""` for the newest page).
    pub(crate) fn get_address_page(
        &self,
        address: &str,
        after_txid: &str,
    ) -> Result<Option<Cached<Vec<String>>>, StoreError> {
        let row: Option<(String, i64)> = self
            .conn()
            .query_row(
                "SELECT txids, fetched_at FROM btc_address_txs
                 WHERE address = ?1 AND after_txid = ?2",
                params![address, after_txid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(json, fetched_at)| decode(&json, fetched_at))
            .transpose()
    }

    /// Store a history page and each of its transactions (keyed by txid) atomically.
    pub(crate) fn put_address_page(
        &self,
        address: &str,
        after_txid: &str,
        txs: &[(String, UtxoTx)],
    ) -> Result<(), StoreError> {
        let mut conn = self.conn();
        let db = conn.transaction()?;
        for (txid, tx) in txs {
            save(&db, "btc_tx", "txid", txid, tx)?;
        }
        let txids: Vec<&str> = txs.iter().map(|(id, _)| id.as_str()).collect();
        db.execute(
            "INSERT OR REPLACE INTO btc_address_txs (address, after_txid, txids, fetched_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                address,
                after_txid,
                serde_json::to_string(&txids)?,
                now_ms()
            ],
        )?;
        db.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn execute_for_test(&self, sql: &str) {
        self.conn().execute_batch(sql).expect("test SQL failed");
    }
}

/// Current time as Unix milliseconds.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn decode<T: DeserializeOwned>(json: &str, fetched_at: i64) -> Result<Cached<T>, StoreError> {
    Ok(Cached {
        value: serde_json::from_str(json)?,
        fetched_at,
    })
}

/// Load a `(key, json, fetched_at)` row. `table` and `key_col` are trusted identifiers.
fn load<T: DeserializeOwned>(
    conn: &Connection,
    table: &str,
    key_col: &str,
    key: &str,
) -> Result<Option<Cached<T>>, StoreError> {
    let row: Option<(String, i64)> = conn
        .query_row(
            &format!("SELECT json, fetched_at FROM {table} WHERE {key_col} = ?1"),
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    row.map(|(json, fetched_at)| decode(&json, fetched_at))
        .transpose()
}

/// Insert or replace a `(key, json, fetched_at)` row, stamped with the current time.
fn save<T: Serialize + ?Sized>(
    conn: &Connection,
    table: &str,
    key_col: &str,
    key: &str,
    value: &T,
) -> Result<(), StoreError> {
    conn.execute(
        &format!(
            "INSERT OR REPLACE INTO {table} ({key_col}, json, fetched_at) VALUES (?1, ?2, ?3)"
        ),
        params![key, serde_json::to_string(value)?, now_ms()],
    )?;
    Ok(())
}

/// Bring the schema up to date. Runs in one IMMEDIATE transaction so two processes opening
/// the same file cannot both apply a migration.
fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let supported = MIGRATIONS.len() as i64;
    let db = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let found: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if found > supported {
        return Err(StoreError::SchemaTooNew { found, supported });
    }
    if found < supported {
        for sql in &MIGRATIONS[found.max(0) as usize..] {
            db.execute_batch(sql)?;
        }
        db.pragma_update(None, "user_version", supported)?;
    }
    db.commit()?;
    Ok(())
}

// The platform's local application-data directory, at the same locations as
// `dirs::data_local_dir()`. Written out here because `dirs` pulls in an MPL-2.0 crate
// (`option-ext`), and dependencies must be MIT/Apache/BSD-style.

#[cfg(windows)]
fn data_local_dir() -> Option<PathBuf> {
    absolute(std::env::var_os("LOCALAPPDATA")?)
}

#[cfg(target_os = "macos")]
fn data_local_dir() -> Option<PathBuf> {
    Some(absolute(std::env::var_os("HOME")?)?.join("Library/Application Support"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn data_local_dir() -> Option<PathBuf> {
    xdg_data_home(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"))
}

#[cfg(not(any(windows, unix)))]
fn data_local_dir() -> Option<PathBuf> {
    None
}

/// XDG base directory rule: `$XDG_DATA_HOME` if absolute, else `$HOME/.local/share`.
#[cfg(all(unix, not(target_os = "macos")))]
fn xdg_data_home(
    xdg_data_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    xdg_data_home
        .and_then(absolute)
        .or_else(|| Some(absolute(home?)?.join(".local/share")))
}

#[cfg(any(unix, windows))]
fn absolute(p: std::ffi::OsString) -> Option<PathBuf> {
    let p = PathBuf::from(p);
    p.is_absolute().then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_version(store: &Store) -> i64 {
        store
            .conn()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn migrations_set_user_version() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(user_version(&store), MIGRATIONS.len() as i64);
        // Running again is a no-op.
        migrate(&mut store.conn()).unwrap();
        assert_eq!(user_version(&store), MIGRATIONS.len() as i64);
    }

    #[test]
    fn newer_schema_rejected() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
        assert!(matches!(
            migrate(&mut conn),
            Err(StoreError::SchemaTooNew { found: 999, .. })
        ));
    }

    #[test]
    fn freshness() {
        let row = |fetched_at| Cached {
            value: (),
            fetched_at,
        };
        let now = now_ms();
        assert!(row(now).is_fresh(Duration::from_secs(60)));
        assert!(!row(now).is_fresh(Duration::ZERO));
        assert!(!row(now - 120_000).is_fresh(Duration::from_secs(60)));
        // Clock moved backwards: treat as stale.
        assert!(!row(now + 3_600_000).is_fresh(Duration::from_secs(60)));
    }

    #[test]
    fn default_path_layout() {
        if let Some(p) = Store::default_path() {
            assert!(p.is_absolute());
            assert!(p.ends_with("chaintrail/cache.sqlite"));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn xdg_rules() {
        let s = |v: &str| Some(std::ffi::OsString::from(v));
        assert_eq!(
            xdg_data_home(s("/data"), s("/home/u")),
            Some(PathBuf::from("/data"))
        );
        // Relative XDG_DATA_HOME is ignored, as the spec requires.
        assert_eq!(
            xdg_data_home(s("rel"), s("/home/u")),
            Some(PathBuf::from("/home/u/.local/share"))
        );
        assert_eq!(
            xdg_data_home(None, s("/home/u")),
            Some(PathBuf::from("/home/u/.local/share"))
        );
        assert_eq!(xdg_data_home(None, None), None);
        assert_eq!(xdg_data_home(None, s("")), None);
    }
}
