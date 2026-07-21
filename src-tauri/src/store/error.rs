use thiserror::Error;

/// Errors surfaced by the store. Kept small and specific so callers fail
/// loudly with useful context rather than falling back silently.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("no row found with id {id}")]
    NotFound { id: i64 },
}
