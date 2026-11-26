#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cache database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Creating the directory that holds the database failed.
    #[error("cache directory error: {0}")]
    Io(#[from] std::io::Error),
    /// A stored object could not be encoded or decoded (e.g. a corrupt row).
    #[error("cache entry could not be (de)serialised: {0}")]
    Json(#[from] serde_json::Error),
    /// The database was written by a newer version of ChainTrail.
    #[error(
        "cache database has schema version {found}, but this version of ChainTrail only \
         supports up to {supported}"
    )]
    SchemaTooNew { found: i64, supported: i64 },
    /// The background thread running a database call panicked or was cancelled.
    #[error("cache task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}
