use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("config base directory unavailable")]
    ConfigBaseDirUnavailable,
    #[error("data base directory unavailable")]
    DataBaseDirUnavailable,
    #[error("home directory unavailable")]
    HomeDirUnavailable,
    #[error("unsupported operation: {0}")]
    Unsupported(&'static str),
    #[error("failed to read config from {path}: {source}")]
    ReadConfig {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config from {path}: {source}")]
    ParseConfig {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("invalid session slug: {0}")]
    InvalidSlug(String),
    #[error("invalid session tag: {0}")]
    InvalidTag(String),
    #[error("invalid GitHub repository reference: {0}")]
    InvalidGitHubRepo(String),
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("revision not found: session={session} revision={revision}")]
    RevisionNotFound { session: String, revision: u32 },
    #[error("no recent session")]
    NoRecentSession,
    #[error("invalid command usage: {0}")]
    InvalidCommandUsage(&'static str),
    #[error("todo not found: {0}")]
    TodoNotFound(i64),
    #[error("todo {todo_id} does not belong to session {session}")]
    TodoSessionMismatch { todo_id: i64, session: String },
    #[error("historical revision is read-only")]
    HistoricalRevisionReadOnly,
    #[error("cannot start timer because another timer is active")]
    ActivePomodoroExists,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
